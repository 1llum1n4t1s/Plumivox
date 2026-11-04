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
    command.stdin(Stdio::null()).kill_on_drop(true);
}
async fn execute(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new(program);
    clean(&mut command);
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

#[tokio::test]
async fn platform_setup_contract() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_SETUP_E2E_OUTPUT_DIR", "setup")?;
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
