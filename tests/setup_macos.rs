use super::*;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::{
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    time::Instant,
};

fn terminal_wizard(
    binary: &Path,
    target: &Path,
    key: &Path,
    answers: &[(String, String)],
) -> Result<(String, usize, std::process::ExitStatus)> {
    let (mut master, mut slave) = (-1, -1);
    // openptyの両端を直ちにFileへ所有させ、失敗・期限超過でも必ず閉じる。
    ensure!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0,
        "Cannot allocate setup PTY"
    );
    let mut master = unsafe { fs::File::from_raw_fd(master) };
    let slave = unsafe { fs::File::from_raw_fd(slave) };
    let mut command = std::process::Command::new(binary);
    command
        .args(["--setup", "--settings-path"])
        .arg(target)
        .arg("--private-key-path")
        .arg(key)
        .args(["--account", "wizard"]);
    for name in [
        "PLUMIVOX_SECRETS_FILE",
        "PLUMIVOX_DEFAULT_ACCOUNT",
        "PLUMIVOX_STATE_DIR",
        "PLUMIVOX_TEST_HOOKS",
    ] {
        command.env_remove(name);
    }
    command
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    // pre_exec内はasync-signal-safeなOS呼出しだけに限定する。
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(command.spawn()?);
    drop(command);
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut output = Vec::new();
    let mut pending = Vec::new();
    let mut index = 0;
    let mut buffer = [0u8; 65536];
    loop {
        ensure!(
            Instant::now() < deadline,
            "Setup terminal deadline exceeded"
        );
        let mut poll = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 100) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if ready == 0 {
            if child.0.try_wait()?.is_some() {
                break;
            }
            continue;
        }
        let length = match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        output.extend_from_slice(&buffer[..length]);
        pending.extend_from_slice(&buffer[..length]);
        ensure!(
            output.len() <= 4 * 1024 * 1024,
            "Setup terminal output limit exceeded"
        );
        while index < answers.len() {
            let (prompt, answer) = &answers[index];
            let Some(at) = pending
                .windows(prompt.len())
                .position(|window| window == prompt.as_bytes())
            else {
                break;
            };
            pending.drain(..at + prompt.len());
            if index >= 9 {
                loop {
                    ensure!(
                        Instant::now() < deadline,
                        "Secret prompt did not disable terminal echo"
                    );
                    let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
                    ensure!(
                        unsafe { libc::tcgetattr(master.as_raw_fd(), term.as_mut_ptr()) } == 0,
                        "Cannot inspect PTY echo"
                    );
                    if unsafe { term.assume_init() }.c_lflag & libc::ECHO == 0 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            master.write_all(answer.as_bytes())?;
            master.write_all(b"\n")?;
            index += 1;
        }
    }
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        ensure!(Instant::now() < deadline, "Setup process did not exit");
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok((String::from_utf8_lossy(&output).into_owned(), index, status))
}
fn owned_metadata(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir()),
        "Private path must be a regular file or directory without links"
    );
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() },
        "Private path must belong to the current user"
    );
    Ok(metadata)
}
fn private(path: &Path) -> Result<()> {
    ensure!(
        owned_metadata(path)?.permissions().mode() & 0o077 == 0,
        "Private path is publicly accessible"
    );
    Ok(())
}
fn set_owned_file_mode(path: &Path, mode: u32) -> Result<()> {
    let before = owned_metadata(path)?;
    ensure!(
        before.is_file(),
        "Permission fixture must be a regular file"
    );
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let opened = file.metadata()?;
    ensure!(
        opened.is_file()
            && opened.uid() == unsafe { libc::geteuid() }
            && opened.dev() == before.dev()
            && opened.ino() == before.ino(),
        "Permission fixture changed before opening"
    );
    // 検査した本人所有のファイル記述子だけを変更し、置換されたパスを辿らない。
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    let after = owned_metadata(path)?;
    ensure!(
        after.dev() == opened.dev()
            && after.ino() == opened.ino()
            && after.permissions().mode() & 0o777 == mode,
        "Permission fixture changed during update"
    );
    Ok(())
}
async fn cli(args: &[String], expected: i32, secrets: &[String]) -> Result<String> {
    let mut all = vec!["--setup".into()];
    all.extend_from_slice(args);
    let out = execute(&binary(), &all, &[]).await?;
    let output = text(&out);
    no_leak(&output, secrets)?;
    ensure!(
        out.status.code() == Some(expected),
        "Native setup unexpected status: {output}"
    );
    Ok(output)
}
pub async fn run() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_NATIVE_SETUP_E2E_OUTPUT_DIR", "native-setup-macos")?;
    private(suite.output())?;
    let secrets = [
        "setup-synthetic-api-never-display",
        "setup-synthetic-email-never-display",
        "setup-synthetic-password-never-display",
        "setup-synthetic-proxy-never-display",
    ]
    .map(String::from)
    .to_vec();
    for secret in &secrets {
        suite.add_secret(secret.clone());
    }
    let key = suite.output().join("keys/plumivox_ed25519");
    let settings = suite.output().join("settings.json");
    let fingerprint = format!("SHA256:{}", "A".repeat(43));
    let profile = |user: &str| json!({"user_name":user,"api_key":secrets[0],"email":secrets[1],"password":secrets[2],"proxy_mode":"managed","proxy":format!("http://plumivox:{}@example.invalid:3128",secrets[3])});
    let initial = json!({"unrelated":{"preserve":["keep",7]},"twitterapi_io":{"primary":profile("Primary"),"secondary":profile("Secondary"),"default_account":"primary"},"plumivox":{"managed_proxy":{"ssh_host":"example.invalid","ssh_user":"plumivox","ssh_port":22,"local_port":43128,"remote_port":43129,"private_key_file":key,"host_key_sha256":fingerprint}}});
    macro_rules! case {($name:expr,$body:block)=>{{let result:Result<()>=async $body.await;suite.record($name,result);}};}
    case!("native-prepare-key-and-refuse-replacement", {
        ensure!(
            cli(
                &[
                    "--prepare-key".into(),
                    "--private-key-path".into(),
                    key.display().to_string()
                ],
                0,
                &secrets
            )
            .await?
            .contains("ssh-ed25519 ")
        );
        for path in [
            key.parent().unwrap().to_path_buf(),
            key.clone(),
            key.with_extension("pub"),
        ] {
            private(&path)?;
        }
        let before = fs::read(&key)?;
        let public = fs::read(key.with_extension("pub"))?;
        ensure!(
            cli(
                &[
                    "--prepare-key".into(),
                    "--private-key-path".into(),
                    key.display().to_string()
                ],
                1,
                &secrets
            )
            .await?
            .contains("KEY_EXISTS")
        );
        ensure!(fs::read(&key)? == before && fs::read(key.with_extension("pub"))? == public);
        Ok(())
    });
    fs::write(&settings, initial.to_string())?;
    set_owned_file_mode(&settings, 0o600)?;
    case!("native-check-valid-settings-is-readonly", {
        let before = fs::read(&settings)?;
        ensure!(
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                    "--account".into(),
                    "primary".into()
                ],
                0,
                &secrets
            )
            .await?
            .contains("ネットワーク通信は実行していません")
        );
        ensure!(fs::read(&settings)? == before);
        Ok(())
    });
    case!(
        "native-default-only-preserves-accounts-relay-and-unrelated-data",
        {
            cli(
                &[
                    "--settings-path".into(),
                    settings.display().to_string(),
                    "--set-default-account".into(),
                    "secondary".into(),
                ],
                0,
                &secrets,
            )
            .await?;
            let mut expected = initial.clone();
            expected["twitterapi_io"]["default_account"] = json!("secondary");
            ensure!(document(&settings)? == expected);
            private(&settings)?;
            let before = fs::read(&settings)?;
            ensure!(
                cli(
                    &[
                        "--settings-path".into(),
                        settings.display().to_string(),
                        "--set-default-account".into(),
                        "missing".into()
                    ],
                    1,
                    &secrets
                )
                .await?
                .contains("DEFAULT_ACCOUNT_INVALID")
            );
            ensure!(fs::read(&settings)? == before);
            Ok(())
        }
    );
    case!("native-wizard-refuses-same-account-and-shared-relay", {
        let before = fs::read(&settings)?;
        for account in ["primary", "new_account"] {
            ensure!(
                cli(
                    &[
                        "--settings-path".into(),
                        settings.display().to_string(),
                        "--private-key-path".into(),
                        key.display().to_string(),
                        "--account".into(),
                        account.into()
                    ],
                    1,
                    &secrets
                )
                .await?
                .contains("SETTINGS_EXISTS")
            );
            ensure!(fs::read(&settings)? == before);
        }
        Ok(())
    });
    case!(
        "native-check-rejects-links-and-public-private-key-permissions",
        {
            let linked = suite.output().join("linked-settings.json");
            symlink(&settings, &linked)?;
            ensure!(
                cli(
                    &[
                        "--check".into(),
                        "--settings-path".into(),
                        linked.display().to_string(),
                        "--account".into(),
                        "primary".into()
                    ],
                    1,
                    &secrets
                )
                .await?
                .contains("PATH_INVALID")
            );
            set_owned_file_mode(&key, 0o644)?;
            let result = cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                    "--account".into(),
                    "primary".into(),
                ],
                1,
                &secrets,
            )
            .await;
            set_owned_file_mode(&key, 0o600)?;
            ensure!(result?.contains("ACL_UNSAFE"));
            Ok(())
        }
    );
    case!(
        "native-wizard-registers-with-hidden-terminal-secrets-and-first-default",
        {
            let target = suite.output().join("wizard-settings.json");
            let answers = json!([
                ["Xユーザー名 (@なし): ", "Wizard"],
                ["VPSホスト名/IP: ", "example.invalid"],
                ["SSHユーザー [plumivox]: ", ""],
                ["SSHポート [22]: ", ""],
                ["PC内部ポート [43128]: ", ""],
                ["VPS内部ポート [43129]: ", ""],
                ["コンソールで確認済みのSHA256指紋: ", fingerprint],
                ["公開プロキシポート [3128]: ", ""],
                ["プロキシユーザー [plumivox]: ", ""],
                ["twitterapi.io APIキー: ", secrets[0]],
                ["Xログインメール: ", secrets[1]],
                ["Xパスワード: ", secrets[2]],
                ["Base32の2FA秘密鍵 (未使用なら空欄): ", ""],
                ["プロキシパスワード: ", secrets[3]]
            ]);
            // 標準PTYをRustで駆動し、回答を引数や平文ファイルへ保存しない。
            let answers: Vec<(String, String)> = answers
                .as_array()
                .context("Missing answers")?
                .iter()
                .map(|row| {
                    Ok((
                        row[0].as_str().context("Missing prompt")?.into(),
                        row[1].as_str().context("Missing answer")?.into(),
                    ))
                })
                .collect::<Result<_>>()?;
            let (program, terminal_target, terminal_key) = (binary(), target.clone(), key.clone());
            let (transcript, answered, status) = tokio::task::spawn_blocking(move || {
                terminal_wizard(&program, &terminal_target, &terminal_key, &answers)
            })
            .await??;
            no_leak(&transcript, &secrets)?;
            ensure!(answered == 14 && status.success(), "Wizard terminal failed");
            let doc = document(&target)?;
            ensure!(
                doc["twitterapi_io"]["default_account"] == "wizard"
                    && doc["twitterapi_io"]["wizard"]["api_key"] == secrets[0]
                    && doc["twitterapi_io"]["wizard"]["password"] == secrets[2]
            );
            ensure!(
                url::Url::parse(doc["twitterapi_io"]["wizard"]["proxy"].as_str().unwrap())?
                    .password()
                    == Some(secrets[3].as_str())
            );
            private(&target)?;
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    target.display().to_string(),
                    "--account".into(),
                    "wizard".into(),
                ],
                0,
                &secrets,
            )
            .await?;
            Ok(())
        }
    );
    suite.finish()
}
