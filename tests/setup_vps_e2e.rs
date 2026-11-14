mod support;
use anyhow::{Context, ensure};
use serde_json::json;
use std::{fs, path::Path, process::Stdio, time::Duration};
use support::{Result, Suite};
fn posix(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) && value.as_bytes().get(1) == Some(&b':') {
        format!("/{}{}", value[..1].to_lowercase(), &value[2..])
    } else {
        value
    }
}
async fn bash(program: &str, args: &[String], bin: &Path) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).stdin(Stdio::null());
    // テストの子プロセスごとにコンソール窓が点滅しないようにする。
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let paths = std::iter::once(bin.to_path_buf())
        .chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ))
        .collect::<Vec<_>>();
    command.env("PATH", std::env::join_paths(paths)?);
    Ok(tokio::time::timeout(Duration::from_secs(15), command.output()).await??)
}
#[tokio::test]
async fn vps_cli_safety() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR", "setup-vps")?;
    suite.add_secret("synthetic-secret".into());
    let program = std::env::var("PLUMIVOX_E2E_BASH").unwrap_or_else(|_| {
        if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe".into()
        } else {
            "bash".into()
        }
    });
    let installer = posix(&Path::new(env!("CARGO_MANIFEST_DIR")).join("setup/vps/install.sh"));
    let trace = suite.output().join("mutation-trace.txt");
    let key = suite.output().join("public-key.pub");
    let original =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
    fs::write(&key, original)?;
    let bin = suite.output().join("fake-bin");
    fs::create_dir(&bin)?;
    for name in [
        "apt-get",
        "systemctl",
        "ufw",
        "install",
        "useradd",
        "chmod",
        "rm",
        "htpasswd",
    ] {
        let target = bin.join(name);
        fs::write(
            &target,
            format!(
                "#!/usr/bin/env bash\nprintf '%s\\n' '{name}' >> '{}'\nexit 91\n",
                posix(&trace).replace('\'', "'\\''")
            ),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        }
    }
    let cases = [
        ("Bash構文", vec!["-n".into(), installer.clone()], 0, vec![]),
        (
            "日本語ヘルプ",
            vec![installer.clone(), "--help".into()],
            0,
            vec!["--check", "パスワード"],
        ),
        (
            "パスワード引数を拒否",
            vec![
                installer.clone(),
                "--password".into(),
                "synthetic-secret".into(),
            ],
            1,
            vec!["未対応の引数"],
        ),
        (
            "値欠落を拒否",
            vec![installer.clone(), "--public-port".into()],
            1,
            vec!["値がありません"],
        ),
        (
            "不正ポートを拒否",
            vec![installer.clone(), "--public-port".into(), "65536".into()],
            1,
            vec!["1024～65535"],
        ),
        (
            "同一ポートを拒否",
            vec![installer.clone(), "--public-port".into(), "43129".into()],
            1,
            vec!["ポートを分けて"],
        ),
        (
            "ユーザー名の注入を拒否",
            vec![installer.clone(), "--ssh-user".into(), "a;id".into()],
            1,
            vec!["ユーザー名"],
        ),
        (
            "公開鍵未指定を拒否",
            vec![installer.clone(), "--check".into()],
            1,
            vec!["公開鍵"],
        ),
    ];
    for (name, args, status, patterns) in cases {
        let result: Result<()> = async {
            let out = bash(&program, &args, &bin).await?;
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            ensure!(
                out.status.code() == Some(status),
                "Unexpected Bash exit code"
            );
            for pattern in patterns {
                ensure!(text.contains(pattern), "Missing CLI diagnostic: {pattern}");
            }
            Ok(())
        }
        .await;
        suite.record(name, result);
    }
    let uid = bash(
        &program,
        &["-c".into(), "printf '%s' \"$EUID\"".into()],
        &bin,
    )
    .await?;
    ensure!(uid.status.success(), "Cannot determine Bash uid");
    if uid.stdout != b"0" {
        for (name, check) in [("非rootのcheckを拒否", true), ("非rootの導入を拒否", false)]
        {
            let mut args = vec![installer.clone()];
            if check {
                args.push("--check".into());
            }
            args.extend(["--public-key-file".into(), posix(&key)]);
            let result: Result<()> = async {
                let out = bash(&program, &args, &bin).await?;
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                ensure!(out.status.code() == Some(1) && text.contains("sudoで実行"));
                Ok(())
            }
            .await;
            suite.record(name, result);
        }
    } else {
        suite.write_json("limitations.json",&json!({"notRun":["非rootのcheckを拒否","非rootの導入を拒否"],"reason":"root cannot exercise non-root refusal; installation is never attempted"}))?;
    }
    let mutations = match fs::read_to_string(&trace) {
        Ok(text) => text.lines().map(String::from).collect::<Vec<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e).context("Read mutation trace"),
    };
    suite.record(
        "変更コマンド未呼出し",
        if mutations.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Mutation command reached"))
        },
    );
    suite.record(
        "公開鍵入力を保持",
        if fs::read_to_string(&key)? == original {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Input key changed"))
        },
    );
    suite.write_json(
        "mutation-trace.json",
        &json!({"commands":mutations,"limitation":"Linux root導入・Squid起動・SSH実接続は未検証"}),
    )?;
    suite.finish()
}
