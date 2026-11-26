#[cfg(target_os = "macos")]
#[path = "setup_macos.rs"]
mod macos;
mod support;
#[cfg(windows)]
#[path = "setup_windows.rs"]
mod windows;
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use support::{McpClient, Result, Suite};
fn binary() -> PathBuf {
    std::env::var_os("PLUMIVOX_NATIVE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_plumivox")))
}
fn clean(command: &mut tokio::process::Command) {
    for key in [
        "PLUMIVOX_SECRETS_FILE",
        "PLUMIVOX_DEFAULT_ACCOUNT",
        "PLUMIVOX_STATE_DIR",
        "PLUMIVOX_TEST_HOOKS",
    ] {
        command.env_remove(key);
    }
    // 人が読む文言を照合するケースがあるため、表示言語を固定する（OS・CI の言語に依存させない）。
    // ケースごとの env で別の言語を指定した場合はそちらが優先される。
    command.env("PLUMIVOX_LANGUAGE", "ja");
    command.stdin(Stdio::null()).kill_on_drop(true);
}
async fn execute(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new(program);
    clean(&mut command);
    // テストの子プロセスごとにコンソール窓が点滅しないようにする。
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command.args(args).envs(env.iter().cloned());
    Ok(tokio::time::timeout(Duration::from_secs(35), command.output()).await??)
}
fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
fn no_leak(out: &str, secrets: &[String]) -> Result<()> {
    ensure!(
        !out.contains("PRIVATE KEY") && secrets.iter().all(|s| !out.contains(s)),
        "Setup output exposed a credential or private key"
    );
    Ok(())
}
fn document(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
async fn listed(client: &McpClient) -> Result<(bool, Value)> {
    let raw = client.call_tool("list_accounts", json!({})).await?;
    let value = raw
        .get("structuredContent")
        .cloned()
        .or_else(|| {
            raw["content"]
                .as_array()?
                .iter()
                .find_map(|v| serde_json::from_str(v["text"].as_str()?).ok())
        })
        .context("Missing list_accounts content")?;
    Ok((raw["isError"] == true, value))
}
async fn selected(
    explicit: Option<&Path>,
    environment: Option<&Path>,
    env: &[(String, String)],
    state: &Path,
) -> Result<McpClient> {
    let mut env = env.to_vec();
    if let Some(path) = environment {
        env.push(("PLUMIVOX_SECRETS_FILE".into(), path.display().to_string()));
    }
    if let Some(path) = explicit {
        McpClient::spawn_with_env(
            &[
                "http://127.0.0.1:1".into(),
                path.display().to_string(),
                state.display().to_string(),
                "250".into(),
            ],
            &env,
        )
        .await
    } else {
        McpClient::spawn_command(&binary(), &[], &env).await
    }
}
async fn names(
    explicit: Option<&Path>,
    environment: Option<&Path>,
    env: &[(String, String)],
    state: &Path,
) -> Result<Vec<String>> {
    let mut client = selected(explicit, environment, env, state).await?;
    let result = listed(&client).await;
    client.close().await?;
    let (failed, value) = result?;
    ensure!(!failed, "List accounts failed");
    Ok(value["accounts"]
        .as_array()
        .context("Missing accounts")?
        .iter()
        .filter_map(|v| v["account"].as_str().map(String::from))
        .collect())
}
async fn frozen_selection(suite: &Suite) -> Result<()> {
    let home = suite.output().join("home");
    let local = suite.output().join("local");
    fs::create_dir_all(home.join("dev/Secret"))?;
    let legacy = home.join("dev/Secret/secrets.json");
    let portable = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Kagayoi/Plumivox/settings.json")
    } else {
        local.join("Kagayoi/Plumivox/settings.json")
    };
    fs::create_dir_all(portable.parent().unwrap())?;
    let explicit = suite.output().join("explicit-native-settings.json");
    let doc = |name: &str| json!({"twitterapi_io":{name:{}}}).to_string();
    fs::write(&portable, doc("portable"))?;
    fs::write(&explicit, doc("explicit"))?;
    let state = suite.output().join("native-selection-state");
    let env = vec![
        ("USERPROFILE".into(), home.display().to_string()),
        ("HOME".into(), home.display().to_string()),
        ("LOCALAPPDATA".into(), local.display().to_string()),
        ("PLUMIVOX_STATE_DIR".into(), state.display().to_string()),
    ];
    let mut frozen = selected(None, None, &env, &state).await?;
    let check: Result<()> = async {
        ensure!(listed(&frozen).await?.1["accounts"][0]["account"] == "portable");
        fs::write(&legacy, doc("legacy"))?;
        ensure!(listed(&frozen).await?.1["accounts"][0]["account"] == "portable");
        fs::write(&portable, "invalid JSON")?;
        let (failed, value) = listed(&frozen).await?;
        ensure!(failed && value["error"]["code"] == "CONFIG_UNAVAILABLE");
        Ok(())
    }
    .await;
    frozen.close().await?;
    check?;
    ensure!(names(None, None, &env, &state).await? == ["legacy"]);
    ensure!(names(Some(&explicit), None, &env, &state).await? == ["explicit"]);
    fs::write(&portable, doc("portable"))?;
    ensure!(names(Some(&explicit), Some(&portable), &env, &state).await? == ["portable"]);
    fs::write(&legacy, "invalid JSON")?;
    let mut invalid = selected(None, None, &env, &state).await?;
    let checked = listed(&invalid).await;
    invalid.close().await?;
    let (failed, value) = checked?;
    ensure!(failed && value["error"]["code"] == "CONFIG_UNAVAILABLE");
    Ok(())
}

/// 人が読む CLI の文言が実行時の言語設定で切り替わることを、実際の実行ファイルで確かめる（全OS共通）。
/// PLUMIVOX_LANGUAGE が最優先で、空なら LC_ALL → LC_MESSAGES → LANG、未対応の言語は英語。
/// エラーコード（MODE_INVALID）は言語によらず英字のまま。
async fn localized_cli() -> Result<()> {
    let bogus = vec!["--setup".to_string(), "--no-such-option".to_string()];
    let language = |tag: &str| vec![("PLUMIVOX_LANGUAGE".to_string(), tag.to_string())];
    for (env, expected) in [
        (language("en"), "MODE_INVALID: Check the setup options"),
        (language("ja"), "MODE_INVALID: セットアップのオプション"),
        (
            language("de"),
            "MODE_INVALID: Prüfen Sie die Setup-Optionen",
        ),
        (language("zh-Hant-TW"), "MODE_INVALID: 請檢查設定選項"),
        (language("pt_PT.UTF-8"), "MODE_INVALID: Verifique as opções"),
        // 未対応の言語は英語。
        (language("it"), "MODE_INVALID: Check the setup options"),
        // 明示の上書きが空なら POSIX のロケール変数（空・C は飛ばして LANG）で決まる。
        (
            vec![
                ("PLUMIVOX_LANGUAGE".into(), String::new()),
                ("LC_ALL".into(), "C".into()),
                ("LC_MESSAGES".into(), String::new()),
                ("LANG".into(), "ko_KR.UTF-8".into()),
            ],
            "MODE_INVALID: 설정 옵션",
        ),
    ] {
        let out = execute(&binary(), &bogus, &env).await?;
        let text = text(&out);
        ensure!(
            out.status.code() == Some(1) && text.contains(expected),
            "Localized setup error mismatch for {env:?}: {text}"
        );
    }
    // ヘルプは標準出力へ利用者の言語で出し、終了コードは 0。
    let out = execute(&binary(), &["--help".to_string()], &language("fr")).await?;
    let help = String::from_utf8_lossy(&out.stdout);
    ensure!(
        out.status.success() && help.contains("Utilisation : plumivox") && help.contains("--setup"),
        "Localized help mismatch: {help}"
    );
    Ok(())
}

/// `--rejection-log` の往復を、実際の実行ファイルと隔離した状態フォルダーで確かめる（全OS共通）。
/// 本物の `%LOCALAPPDATA%` などには触れない。終了コードとファイルの有無で判定する。
async fn rejection_log_roundtrip(suite: &Suite) -> Result<()> {
    let home = suite.output().join("rejection-home");
    let state = suite.output().join("rejection-state");
    fs::create_dir_all(&home)?;
    let env = vec![
        (
            "PLUMIVOX_STATE_DIR".to_string(),
            state.display().to_string(),
        ),
        ("HOME".into(), home.display().to_string()),
        ("USERPROFILE".into(), home.display().to_string()),
        (
            "LOCALAPPDATA".into(),
            home.join("local").display().to_string(),
        ),
        ("APPDATA".into(), home.join("roaming").display().to_string()),
    ];
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let env = env.clone();
        async move {
            let out = execute(&binary(), &args, &env).await?;
            Ok::<_, anyhow::Error>((
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                text(&out),
            ))
        }
    };
    let diagnostics = state.join("diagnostics");
    let setting = diagnostics.join("rejection-log.json");
    let log = diagnostics.join("provider-rejections.jsonl");
    let now = chrono::Utc::now();
    let stamp = |age_days: i64| {
        (now - chrono::Duration::days(age_days))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    // 既定: status は成功し、フォルダーを作らない。
    let (code, _, off_status) = run(&["--rejection-log", "status"]).await?;
    ensure!(
        code == Some(0) && !diagnostics.exists(),
        "status created state"
    );
    // 引数の誤りは終了コード 2、何も作らない。
    for bad in [
        vec!["--rejection-log", "on", "--days", "0"],
        vec!["--rejection-log", "on", "--days", "31"],
        vec!["--rejection-log", "on", "--days", "x"],
        vec!["--rejection-log", "bogus"],
    ] {
        let (code, _, all) = run(&bad).await?;
        ensure!(code == Some(2) && !diagnostics.exists(), "{bad:?}: {all}");
    }
    // on --days 1: 期限は1日後で、保存先を表示する。
    let (code, shown, all) = run(&["--rejection-log", "on", "--days", "1"]).await?;
    ensure!(
        code == Some(0) && shown.contains("provider-rejections.jsonl"),
        "{all}"
    );
    let value: Value = serde_json::from_slice(&fs::read(&setting)?)?;
    let (until, at) = (
        value["enabled_until"].as_u64(),
        value["enabled_at"].as_u64(),
    );
    ensure!(
        until.zip(at).is_some_and(|(u, a)| u - a == 86_400),
        "Unexpected setting: {value}"
    );
    let (code, _, on_status) = run(&["--rejection-log", "status"]).await?;
    ensure!(
        code == Some(0) && on_status != off_status,
        "status did not change"
    );
    // show: 新しい行だけを標準出力へ出し、14日を超えた行は消す。
    let fresh = json!({"schema":1,"recordedAt":stamp(0),"tool":"post_tweet","marker":"fresh-line"});
    let old = json!({"schema":1,"recordedAt":stamp(15),"tool":"post_tweet","marker":"old-line"});
    fs::write(&log, format!("{old}\n{fresh}\n"))?;
    let (code, shown, all) = run(&["--rejection-log", "show"]).await?;
    ensure!(
        code == Some(0) && shown.trim_end() == fresh.to_string() && !all.contains("old-line"),
        "show output: {all}"
    );
    ensure!(
        !fs::read_to_string(&log)?.contains("old-line"),
        "old line not pruned"
    );
    // off: 記録は残し、有効化だけ止める。
    let (code, _, _) = run(&["--rejection-log", "off"]).await?;
    ensure!(code == Some(0) && log.exists(), "off removed the log");
    let value: Value = serde_json::from_slice(&fs::read(&setting)?)?;
    ensure!(
        value["enabled_until"]
            .as_u64()
            .is_some_and(|u| u <= now.timestamp() as u64 + 5)
    );
    let (code, _, status) = run(&["--rejection-log", "status"]).await?;
    ensure!(
        code == Some(0) && status != on_status,
        "status still on after off"
    );
    // clear: 記録ファイルだけを消す。
    let (code, _, _) = run(&["--rejection-log", "clear"]).await?;
    ensure!(
        code == Some(0) && !log.exists() && setting.exists(),
        "clear result"
    );
    let (code, shown, _) = run(&["--rejection-log", "show"]).await?;
    ensure!(code == Some(0) && shown.is_empty(), "show after clear");
    // 本物の状態フォルダーへは書かない: 隔離した HOME の外に diagnostics を作らない。
    ensure!(
        !home.join("local").join("Kagayoi").exists(),
        "Wrote to the OS default state folder"
    );
    Ok(())
}

#[tokio::test]
async fn rejection_log_cli_roundtrip() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_SETUP_E2E_OUTPUT_DIR", "rejection-log-cli")?;
    let result = rejection_log_roundtrip(&suite).await;
    suite.record("rejection-log-cli-roundtrip", result);
    suite.finish()
}

#[tokio::test]
async fn platform_setup_contract() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_SETUP_E2E_OUTPUT_DIR", "setup")?;
    let result = localized_cli().await;
    suite.record("cli-text-follows-language-settings", result);
    let result = frozen_selection(&suite).await;
    suite.record("portable-source-priority-and-frozen-selection", result);
    #[cfg(windows)]
    windows::run(&mut suite).await?;
    #[cfg(target_os = "macos")]
    macos::run().await?;
    #[cfg(not(any(windows, target_os = "macos")))]
    suite.write_json(
        "limitations.json",
        &json!({"notRun":"Windows and macOS native setup require their actual OS"}),
    )?;
    suite.finish()
}
