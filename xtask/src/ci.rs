//! CI固有の処理。checkout前の最小bootstrapだけはworkflow内のRustで行う。
use crate::{Result, build, build_vps, cargo_run, root};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::{env, fs, io::Write, process::Command};

fn required(name: &str) -> Result<String> {
    let value = env::var(name).map_err(|_| format!("{name}が必要です"))?;
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(format!("{name}が不正です").into());
    }
    Ok(value)
}

fn output(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new(program).args(args).output()?;
    if !result.status.success() {
        // security等の生エラーに資格情報を含めない。
        return Err(format!("CIコマンドが失敗しました: {program}").into());
    }
    Ok(result.stdout)
}

fn architecture_set(bytes: &[u8]) -> bool {
    let mut arches = String::from_utf8_lossy(bytes)
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    arches.sort();
    arches == ["arm64", "x86_64"]
}

fn validate_identity(values: &[String]) -> Result {
    if values.len() != 4 {
        return Err("CI成果物identityの項目数が不正です".into());
    }
    if values[0].len() != 40
        || !values[0]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || values[1..3].iter().any(|v| {
            v.is_empty()
                || v.len() > 20
                || v.starts_with('0')
                || !v.bytes().all(|b| b.is_ascii_digit())
        })
        || !matches!(
            values[3].as_str(),
            "windows-x64" | "macos-universal" | "vps-linux-x86_64" | "vps-linux-aarch64"
        )
    {
        return Err("CI成果物identityが不正です".into());
    }
    Ok(())
}

fn identity() -> Result<Vec<String>> {
    let values = [
        "ARTIFACT_COMMIT",
        "ARTIFACT_RUN",
        "ARTIFACT_ATTEMPT",
        "ARTIFACT_TARGET",
    ]
    .map(required)
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    validate_identity(&values)?;
    Ok(values)
}

fn vps_arch(target: &str) -> Result<&'static str> {
    let arch = match target {
        "vps-linux-x86_64" => "x86_64",
        "vps-linux-aarch64" => "aarch64",
        _ => return Err("不正なVPS配布対象です".into()),
    };
    if env::consts::OS != "linux" || env::consts::ARCH != arch {
        return Err("VPS配布検証には対象CPUのLinux runnerが必要です".into());
    }
    Ok(arch)
}

fn artifact_upload() -> Result {
    let id = identity()?;
    crate::r2::execute(&[
        "artifact-upload".into(),
        "--commit".into(),
        id[0].clone(),
        "--run".into(),
        id[1].clone(),
        "--attempt".into(),
        id[2].clone(),
        "--target".into(),
        id[3].clone(),
    ])?;
    let mut summary = fs::OpenOptions::new()
        .append(true)
        .open(required("GITHUB_STEP_SUMMARY")?)?;
    writeln!(
        summary,
        "### R2 artifact identity\n\n```text\ncargo xtask artifact-download --commit {} --run {} --attempt {} --target {} --output <new-directory>\n```",
        id[0], id[1], id[2], id[3]
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn keychain_root() -> Result<PathBuf> {
    // MCPのfixtureはOSの一時ディレクトリ配下だけを許可する。
    // RUNNER_TEMPはTMPDIRと一致するとは限らないため、実行物の退避先と分ける。
    Ok(env::temp_dir().canonicalize()?.join("plumivox-ci-keychain"))
}

#[cfg(target_os = "macos")]
fn validate_keychain_root(directory: &std::path::Path) -> Result {
    use std::os::unix::fs::MetadataExt;
    let uid: u32 = std::str::from_utf8(&output("/usr/bin/id", &["-u"])?)?
        .trim()
        .parse()?;
    for ancestor in directory.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || (metadata.uid() != uid && metadata.uid() != 0)
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
        {
            return Err("Keychainの保存先または親階層が安全ではありません".into());
        }
        if ancestor == directory && (metadata.uid() != uid || metadata.mode() & 0o077 != 0) {
            return Err("Keychainの保存先は本人限定のディレクトリが必要です".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn keychain_prepare() -> Result {
    use std::os::unix::fs::DirBuilderExt;
    let directory = keychain_root()?;
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    validate_keychain_root(&directory)?;
    let cleanup = root().join("plumivox-ci-cleanup.exe");
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&cleanup)?;
    let mut executable = fs::File::open(env::current_exe()?)?;
    std::io::copy(&mut executable, &mut destination)?;
    use std::os::unix::fs::PermissionsExt;
    destination.set_permissions(fs::Permissions::from_mode(0o700))?;
    let keychain = directory.join("test.keychain-db");
    // 作成前にsnapshotを確定し、作成途中の失敗でもalways stepが清掃できるようにする。
    fs::write(
        directory.join("default.before"),
        output("/usr/bin/security", &["default-keychain", "-d", "user"])?,
    )?;
    fs::write(
        directory.join("search.before"),
        output("/usr/bin/security", &["list-keychains", "-d", "user"])?,
    )?;
    let mut github_env = fs::OpenOptions::new()
        .append(true)
        .open(required("GITHUB_ENV")?)?;
    writeln!(github_env, "PLUMIVOX_TEST_KEYCHAIN={}", keychain.display())?;
    github_env.flush()?;
    let password = String::from_utf8(output("/usr/bin/uuidgen", &[])?)?;
    let keychain = keychain
        .to_str()
        .ok_or("KeychainパスがUTF-8ではありません")?;
    output(
        "/usr/bin/security",
        &["create-keychain", "-p", password.trim(), keychain],
    )?;
    output(
        "/usr/bin/security",
        &["unlock-keychain", "-p", password.trim(), keychain],
    )?;
    output(
        "/usr/bin/security",
        &["set-keychain-settings", "-lut", "21600", keychain],
    )?;
    keychain_compare(&directory)
}

#[cfg(target_os = "macos")]
fn keychain_compare(directory: &std::path::Path) -> Result {
    // 一方の照合が失敗しても両方の状態取得を試みる。
    let default = output("/usr/bin/security", &["default-keychain", "-d", "user"]);
    let search = output("/usr/bin/security", &["list-keychains", "-d", "user"]);
    let default_matches = default? == fs::read(directory.join("default.before"))?;
    let search_matches = search? == fs::read(directory.join("search.before"))?;
    if !default_matches || !search_matches {
        return Err("既定Keychainまたは検索一覧が変更されました".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn keychain_cleanup() -> Result {
    let directory = keychain_root()?;
    validate_keychain_root(&directory)?;
    let expected = directory.join("test.keychain-db");
    if PathBuf::from(required("PLUMIVOX_TEST_KEYCHAIN")?) != expected {
        return Err("清掃対象Keychainが一致しません".into());
    }
    // 削除が検索一覧への誤登録を隠さないよう、削除前にも照合する。
    let before = keychain_compare(&directory);
    let deletion = (|| -> Result {
        if expected.try_exists()? {
            output(
                "/usr/bin/security",
                &[
                    "delete-keychain",
                    expected.to_str().ok_or("不正なKeychainパス")?,
                ],
            )
            .map(|_| ())
        } else {
            Ok(())
        }
    })();
    let comparison = keychain_compare(&directory);
    let errors = [before, deletion, comparison]
        .into_iter()
        .filter_map(|result| result.err().map(|error| error.to_string()))
        .collect::<Vec<_>>();
    if errors.is_empty() {
        // 同じOS一時領域を使う次回実行へ固定名の作業ディレクトリを残さない。
        // 既知のsnapshotだけを削除し、未知の内容がある場合はremove_dirで拒否する。
        fs::remove_file(directory.join("default.before"))?;
        fs::remove_file(directory.join("search.before"))?;
        fs::remove_dir(directory)?;
        Ok(())
    } else {
        Err(errors.join("; ").into())
    }
}

/// workflowの単独cargo呼出しと、退避した清掃実行物の両方から呼ぶ。
pub fn execute(command: &str) -> Result {
    match command {
        "ci-install-targets" => {
            let target = required("ARTIFACT_TARGET")?;
            if target.starts_with("vps-linux-") {
                let triple = format!("{}-unknown-linux-musl", vps_arch(&target)?);
                output(
                    "rustup",
                    &["target", "add", "--toolchain", "1.99.1", &triple],
                )?;
                return Ok(());
            }
            if !matches!(target.as_str(), "windows-x64" | "macos-universal") {
                return Err("不正な配布対象です".into());
            }
            if target == "macos-universal" {
                output(
                    "rustup",
                    &[
                        "target",
                        "add",
                        "--toolchain",
                        "1.99.1",
                        "aarch64-apple-darwin",
                        "x86_64-apple-darwin",
                    ],
                )?;
            }
            Ok(())
        }
        "ci-distribution-build" => {
            let target = required("ARTIFACT_TARGET")?;
            if target.starts_with("vps-linux-") {
                let arch = vps_arch(&target)?;
                build_vps(arch)?;
                // 静的実行物自体を対象CPUで起動し、動的ローダー不要も確認する。
                let name = crate::VPS_BINARIES
                    .iter()
                    .find(|entry| entry.0 == arch)
                    .unwrap()
                    .1;
                output(
                    root()
                        .join("setup/vps")
                        .join(name)
                        .to_str()
                        .ok_or("不正なVPS配布パス")?,
                    &["--help"],
                )?;
                return Ok(());
            }
            if !matches!(
                (env::consts::OS, target.as_str()),
                ("windows", "windows-x64") | ("macos", "macos-universal")
            ) {
                return Err("不正な配布対象です".into());
            }
            build(target == "macos-universal")?;
            if target == "macos-universal"
                && !architecture_set(&output(
                    "/usr/bin/lipo",
                    &[
                        "-archs",
                        root()
                            .join("dist/plumivox")
                            .to_str()
                            .ok_or("不正な配布パス")?,
                    ],
                )?)
            {
                return Err("universal成果物に必要な両アーキテクチャがありません".into());
            }
            Ok(())
        }
        "ci-vps-test" => {
            vps_arch(&required("ARTIFACT_TARGET")?)?;
            // 不足時はskipにせず失敗する。実sshd検証は明示的にignored全件を実行する。
            output("sshd", &["-V"])?;
            // ssh-keygenには成功終了するversion引数がないため、起動可能性を検査する。
            Command::new("ssh-keygen")
                .arg("-?")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            cargo_run(&["test", "--locked", "--package", "plumivox-vps-installer"])?;
            cargo_run(&[
                "test",
                "--locked",
                "--package",
                "plumivox-vps-installer",
                "--",
                "--ignored",
            ])
        }
        "ci-artifact-upload" => artifact_upload(),
        #[cfg(target_os = "macos")]
        "ci-keychain-prepare" => keychain_prepare(),
        #[cfg(target_os = "macos")]
        "ci-keychain-cleanup" => keychain_cleanup(),
        _ => Err("未知または対象OS外のCIコマンドです".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn universal_requires_exact_architecture_pair() {
        assert!(architecture_set(b"x86_64 arm64\n"));
        assert!(architecture_set(b"arm64 x86_64\n"));
        for value in [
            b"arm64".as_slice(),
            b"arm64 arm64",
            b"arm64 x86_64 i386",
            b"",
        ] {
            assert!(!architecture_set(value));
        }
    }

    #[test]
    fn summary_identity_cannot_inject_commands_or_markdown() {
        let valid = [
            "a".repeat(40),
            "12".into(),
            "1".into(),
            "windows-x64".into(),
        ];
        assert!(validate_identity(&valid).is_ok());
        for target in ["macos-universal", "vps-linux-x86_64", "vps-linux-aarch64"] {
            let mut additional = valid.clone();
            additional[3] = target.into();
            assert!(validate_identity(&additional).is_ok());
        }
        for (field, value) in [
            (0, "A".repeat(40)),
            (0, "a".repeat(39)),
            (1, "".into()),
            (1, "0".into()),
            (1, "01".into()),
            (2, "1\n```".into()),
            (3, "linux-x64".into()),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(validate_identity(&invalid).is_err());
        }
    }
}
