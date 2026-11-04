use std::{
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const SECRET: &str = "synthetic-cli-secret-must-not-escape";
const BINARY: &str = env!("CARGO_BIN_EXE_plumivox-vps-setup");

// 実バイナリを使い、入力待ちや意図しない長時間処理も失敗として検出する。
fn cli(args: &[&str]) -> Output {
    let mut child = Command::new(BINARY)
        .args(args)
        .env("PLUMIVOX_PROXY_PASSWORD", SECRET)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLIを起動できません");
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut stream: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            bytes
        })
    };
    let stdout = read(Box::new(stdout));
    let stderr = read(Box::new(stderr));
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("時間切れCLIを停止できません");
            child.wait().unwrap();
            stdout.join().unwrap();
            stderr.join().unwrap();
            panic!("CLIが10秒以内に終了しません");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    };
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains(SECRET));
    }
    output
}

fn rejected(args: &[&str], expected: &str) {
    let output = cli(args);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "失敗時に標準出力を汚染しています");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(expected),
        "想定した拒否理由がありません: {stderr}"
    );
}

#[test]
fn help_describes_native_entry_point_without_installing() {
    let output = cli(&["--help"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    for option in [
        "plumivox-vps-setup",
        "--public-key-file",
        "--public-port",
        "--remote-port",
        "--ssh-user",
        "--proxy-user",
        "--check",
        "--help",
    ] {
        assert!(help.contains(option), "ヘルプに{option}がありません");
    }
}

#[test]
fn unknown_arguments_and_missing_values_are_rejected_without_echoing_input() {
    rejected(&[SECRET], "未対応の引数");
    for option in [
        "--public-key-file",
        "--public-port",
        "--remote-port",
        "--ssh-user",
        "--proxy-user",
    ] {
        rejected(&[option], "値がありません");
    }
    rejected(&[], "--public-key-file");
    rejected(&["--public-key-file", ""], "--public-key-file");
}

#[test]
fn both_ports_reject_out_of_range_and_non_decimal_values() {
    for option in ["--public-port", "--remote-port"] {
        for value in [
            "",
            "0",
            "1",
            "1023",
            "65536",
            "999999999999999999999999",
            "-1",
            "+1024",
            "01024",
            "1024.0",
            "1024 ",
            " 1024",
            "１Ｘ",
            SECRET,
        ] {
            rejected(&[option, value], "1024～65535");
        }
    }
}

#[test]
fn inclusive_port_boundaries_reach_the_next_validation() {
    // 次の名前検証で確実に止め、Linux/rootでも導入へ進まない。
    for option in ["--public-port", "--remote-port"] {
        for value in ["1024", "1025", "65534", "65535"] {
            rejected(&[option, value, "--ssh-user", SECRET], "ユーザー名");
        }
    }
}

#[test]
fn equal_ports_are_rejected_including_default_collisions() {
    for value in ["1024", "3128", "43129", "65535"] {
        rejected(
            &["--public-port", value, "--remote-port", value],
            "公開ポートと逆トンネルポートを分けて",
        );
    }
    rejected(
        &["--public-port", "43129"],
        "公開ポートと逆トンネルポートを分けて",
    );
    rejected(
        &["--remote-port", "3128"],
        "公開ポートと逆トンネルポートを分けて",
    );
}

#[test]
fn user_names_reject_shell_and_configuration_injection() {
    for option in ["--ssh-user", "--proxy-user"] {
        for value in [
            "",
            "Root",
            "0user",
            "-option",
            "user name",
            "user\nroot",
            "a;id",
            "a$(id)",
            "a`id`",
            "a/../root",
            "a:root",
            "a\rroot",
            SECRET,
        ] {
            rejected(&[option, value], "ユーザー名");
        }
    }
}

#[test]
fn invalid_public_key_paths_are_rejected_without_exposing_them() {
    // ディレクトリと存在しない子。fixtureや実VPSのファイルは作成しない。
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let missing = directory.join(SECRET).join("missing.pub");
    for path in [directory, missing.as_path()] {
        for check in [false, true] {
            let mut args = vec!["--public-key-file", path.to_str().unwrap()];
            if check {
                args.push("--check");
            }
            #[cfg(target_os = "linux")]
            rejected(&args, "通常ファイルのEd25519公開鍵");
            #[cfg(not(target_os = "linux"))]
            rejected(&args, "対応するLinux VPS上で実行してください");
        }
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_refuses_both_install_and_check_before_any_host_operation() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    for check in [false, true] {
        let mut args = vec!["--public-key-file", file.to_str().unwrap()];
        if check {
            args.push("--check");
        }
        rejected(&args, "対応するLinux VPS上で実行してください");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn non_root_linux_refuses_before_commands_or_mutations() {
    // rootで導入を試さない。未実行を成功と数えず、一般ユーザーでの再実行を要求する。
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "この系統は非root Linuxでのみ実行してください"
    );
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let targets = [
        "/etc/plumivox-proxy",
        "/var/lib/plumivox-ssh",
        "/etc/systemd/system/plumivox-proxy.service",
        "/etc/ssh/sshd_config",
        "/etc/passwd",
        "/etc/group",
    ];
    let snapshot = || {
        targets.map(|path| {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::symlink_metadata(path)
                .map(|m| {
                    (
                        m.dev(),
                        m.ino(),
                        m.mode(),
                        m.uid(),
                        m.gid(),
                        m.len(),
                        m.mtime(),
                        m.mtime_nsec(),
                    )
                })
                .map_err(|e| e.kind());
            let content = std::fs::read(path).map_err(|e| e.kind());
            (metadata, content)
        })
    };
    let before = snapshot();
    // Cargo.tomlは通常ファイルなので、公開鍵内容の読取りより先の権限拒否を通す。
    for check in [false, true] {
        let mut args = vec!["--public-key-file", file.to_str().unwrap()];
        if check {
            args.push("--check");
        }
        rejected(&args, "sudoで実行してください");
        assert_eq!(snapshot(), before, "保護対象ファイルが変更されています");
    }
}
