#[cfg(target_os = "macos")]
#[path = "setup_macos.rs"]
mod macos;
mod support;
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

#[cfg(windows)]
mod windows {
    use super::*;
    fn quote(value: impl AsRef<str>) -> String {
        format!("'{}'", value.as_ref().replace('\'', "''"))
    }
    fn qpath(value: &Path) -> String {
        quote(value.to_string_lossy())
    }
    struct Ps {
        output: PathBuf,
        local: PathBuf,
        key: PathBuf,
        settings: PathBuf,
        script: PathBuf,
        secrets: Vec<String>,
        sequence: usize,
    }
    impl Ps {
        fn invoke(&self) -> String {
            format!(
                "Invoke-PlumivoxSetup -SettingsPath {} -PrivateKeyPath {}",
                qpath(&self.settings),
                qpath(&self.key)
            )
        }
        fn valid(&self) -> Vec<String> {
            let mut values = vec![
                "example_user".into(),
                "example.invalid".into(),
                "".into(),
                "".into(),
                "".into(),
                "".into(),
                format!("SHA256:{}", "A".repeat(43)),
                "".into(),
                "".into(),
            ];
            values.extend(self.secrets[..3].iter().cloned());
            values.push("".into());
            values.push(self.secrets[3].clone());
            values
        }
        async fn run(
            &mut self,
            body: &str,
            values: &[String],
            hook: &str,
        ) -> Result<std::process::Output> {
            self.sequence += 1;
            let wrapper = self.output.join(format!("invoke-{}.ps1", self.sequence));
            let input = self.output.join(format!("input-{}.json", self.sequence));
            fs::write(&input, serde_json::to_vec(values)?)?;
            fs::write(
                &wrapper,
                format!(
                    "$ErrorActionPreference='Stop'\n. {}\n$script:answers=[Collections.Generic.Queue[string]]::new()\n(Get-Content -LiteralPath {} -Raw | ConvertFrom-Json) | ForEach-Object {{ $script:answers.Enqueue($_) }}\nfunction Read-Host {{ param([string]$Prompt,[switch]$AsSecureString)\n{hook}\n$v=$script:answers.Dequeue()\nif($AsSecureString){{ if($v -eq '') {{ return [Security.SecureString]::new() }}; return ConvertTo-SecureString $v -AsPlainText -Force }}; return $v\n}}\ntry {{ {body}\n}} catch {{ Write-Host 'SETUP_FAILED'; exit 1 }}\n",
                    qpath(&self.script),
                    qpath(&input)
                ),
            )?;
            let out = execute(
                Path::new("pwsh"),
                &[
                    "-NoProfile".into(),
                    "-File".into(),
                    wrapper.display().to_string(),
                ],
                &[
                    ("LOCALAPPDATA".into(), self.local.display().to_string()),
                    ("PLUMIVOX_SECRETS_FILE".into(), "".into()),
                ],
            )
            .await?;
            no_leak(&text(&out), &self.secrets)?;
            Ok(out)
        }
        async fn good(&mut self, body: &str, values: &[String]) -> Result<String> {
            let out = self.run(body, values, "").await?;
            ensure!(
                out.status.success(),
                "PowerShell setup failed: {}",
                text(&out)
            );
            Ok(text(&out))
        }
        async fn bad(&mut self, body: &str, values: &[String]) -> Result<()> {
            ensure!(
                self.run(body, values, "").await?.status.code() == Some(1),
                "Setup should reject input"
            );
            Ok(())
        }
        fn target_invoke(&self, path: &Path) -> String {
            format!(
                "Invoke-PlumivoxSetup -SettingsPath {} -PrivateKeyPath {}",
                qpath(path),
                qpath(&self.key)
            )
        }
    }
    pub async fn run(suite: &mut Suite) -> Result<()> {
        let mut ps = Ps {
            output: suite.output().into(),
            local: suite.output().join("local"),
            key: suite
                .output()
                .join("local/Kagayoi/Plumivox/keys/plumivox_ed25519"),
            settings: suite.output().join("settings.json"),
            script: Path::new(env!("CARGO_MANIFEST_DIR")).join("setup/Setup-Plumivox.ps1"),
            secrets: [
                "dummy-api-secret",
                "dummy-email-secret",
                "dummy-x-password",
                "dummy-proxy-password",
            ]
            .map(String::from)
            .to_vec(),
            sequence: 0,
        };
        for secret in &ps.secrets {
            suite.add_secret(secret.clone());
        }
        macro_rules! case {($name:expr,$body:block)=>{{let result:Result<()>=async $body.await;suite.record($name,result);}};}
        case!("prepare-dedicated-key-and-refuse-replacement", {
            ensure!(
                ps.good(
                    "$PSNativeCommandArgumentPassing='Legacy'; Invoke-PlumivoxSetup -PrepareKey",
                    &[]
                )
                .await?
                .contains("ssh-ed25519 ")
            );
            let command = [
                ps.key.parent().unwrap().to_path_buf(),
                ps.key.clone(),
                ps.key.with_extension("pub"),
            ]
            .iter()
            .map(|p| format!("Assert-PlumivoxPrivateAcl {}", qpath(p)))
            .collect::<Vec<_>>()
            .join("; ");
            ps.good(&command, &[]).await?;
            ps.bad("Invoke-PlumivoxSetup -PrepareKey", &[]).await?;
            let out = execute(
                Path::new("pwsh"),
                &[
                    "-NoProfile".into(),
                    "-File".into(),
                    ps.script.display().to_string(),
                    "-PrepareKey".into(),
                ],
                &[("LOCALAPPDATA".into(), ps.local.display().to_string())],
            )
            .await?;
            no_leak(&text(&out), &ps.secrets)?;
            ensure!(out.status.code() == Some(1) && text(&out).contains("既存鍵をそのまま再利用"));
            Ok(())
        });
        case!("wizard-preserves-unrelated-config-and-check-is-readonly", {
            fs::write(&ps.settings,json!({"unrelated":{"value":["keep",7]},"twitterapi_io":{"other":{"user_name":"other"}}}).to_string())?;
            let mut values = vec!["other".into()];
            values.extend(ps.valid());
            ps.good(&ps.invoke(), &values).await?;
            let raw = fs::read(&ps.settings)?;
            let doc = document(&ps.settings)?;
            ensure!(
                doc["unrelated"] == json!({"value":["keep",7]})
                    && doc["twitterapi_io"]["other"] == json!({"user_name":"other"})
                    && doc["twitterapi_io"]["main"]["proxy_mode"] == "managed"
                    && doc["twitterapi_io"]["default_account"] == "other"
            );
            ensure!(
                ps.good(
                    &format!(
                        "$PSNativeCommandArgumentPassing='Legacy'; {} -Check",
                        ps.invoke()
                    ),
                    &[]
                )
                .await?
                .contains("既定アカウント: other")
            );
            ensure!(fs::read(&ps.settings)? == raw);
            ps.bad(&ps.invoke(), &ps.valid()).await?;
            ensure!(fs::read(&ps.settings)? == raw);
            Ok(())
        });
        case!(
            "wizard-default-first-account-preserve-and-explicit-selection",
            {
                let long = format!("profile-{}", "p".repeat(65));
                for (name, existing, option, prompt, expected) in [
                    ("first", json!({}), "", false, "main"),
                    (
                        "preserve",
                        json!({"other":{"user_name":"other"},"default_account":"other"}),
                        "",
                        false,
                        "other",
                    ),
                    (
                        "preserve-long",
                        json!({long.clone():{"user_name":"other"},"default_account":long}),
                        "",
                        false,
                        long.as_str(),
                    ),
                    (
                        "explicit",
                        json!({"other":{"user_name":"other"},"default_account":"other"}),
                        "-DefaultAccount main",
                        false,
                        "main",
                    ),
                    (
                        "prompt-main",
                        json!({"other":{"user_name":"other"}}),
                        "",
                        true,
                        "main",
                    ),
                ] {
                    let target = ps.output.join(format!("default-{name}.json"));
                    fs::write(&target, json!({"twitterapi_io":existing}).to_string())?;
                    let mut values = ps.valid();
                    if prompt {
                        values.insert(0, "main".into());
                    }
                    ps.good(&format!("{} {option}", ps.target_invoke(&target)), &values)
                        .await?;
                    ensure!(document(&target)?["twitterapi_io"]["default_account"] == expected);
                }
                for (index, choice) in ["", "missing", "OTHER"].iter().enumerate() {
                    let target = ps.output.join(format!("default-invalid-{index}.json"));
                    let raw = json!({"twitterapi_io":{"other":{"user_name":"other"}}}).to_string();
                    fs::write(&target, &raw)?;
                    let mut values = ps.valid();
                    values.insert(0, (*choice).into());
                    ps.bad(&ps.target_invoke(&target), &values).await?;
                    ensure!(fs::read_to_string(&target)? == raw);
                }
                Ok(())
            }
        );
        case!(
            "set-default-only-preserves-profiles-relay-and-rechecks-settings",
            {
                let target = ps.output.join("default-only.json");
                let mut doc = document(&ps.settings)?;
                fs::write(&target, doc.to_string())?;
                let change = format!(
                    "Invoke-PlumivoxSetup -SettingsPath {} -SetDefaultAccount main",
                    qpath(&target)
                );
                ps.good(
                    &format!("Set-PlumivoxPrivateAcl {}; {change}", qpath(&target)),
                    &[],
                )
                .await?;
                doc["twitterapi_io"]["default_account"] = json!("main");
                ensure!(document(&target)? == doc);
                let long = format!("profile-{}", "p".repeat(65));
                doc["twitterapi_io"][&long] = json!({"user_name":"existing"});
                doc["twitterapi_io"]["DEFAULT_ACCOUNT"] = json!({"user_name":"uppercase"});
                fs::write(&target, doc.to_string())?;
                for choice in [long.as_str(), "DEFAULT_ACCOUNT", "main"] {
                    ps.good(
                        &format!(
                            "Invoke-PlumivoxSetup -SettingsPath {} -SetDefaultAccount {}",
                            qpath(&target),
                            quote(choice)
                        ),
                        &[],
                    )
                    .await?;
                    doc["twitterapi_io"]["default_account"] = json!(choice);
                    ensure!(document(&target)? == doc);
                }
                let raw = fs::read(&target)?;
                for choice in ["", "missing", "MAIN"] {
                    ps.bad(
                        &format!(
                            "Invoke-PlumivoxSetup -SettingsPath {} -SetDefaultAccount {}",
                            qpath(&target),
                            quote(choice)
                        ),
                        &[],
                    )
                    .await?;
                    ensure!(fs::read(&target)? == raw);
                }
                let changed = "{\"unrelated\":\"changed-during-default-save\"}";
                let body = format!(
                    "function Assert-PlumivoxPath {{ param([string]$Path) $script:pathChecks++; if ($script:pathChecks -eq 2) {{ [IO.File]::WriteAllText({}, {}) }}; return $Path }}; $script:pathChecks=0; {change}",
                    qpath(&target),
                    quote(changed)
                );
                ps.bad(&body, &[]).await?;
                ensure!(fs::read_to_string(&target)? == changed);
                Ok(())
            }
        );
        case!("check-refuses-invalid-default-without-mutating-settings", {
            for (i, value) in [
                Value::Null,
                json!(12),
                json!({}),
                json!("missing"),
                json!("MAIN"),
            ]
            .into_iter()
            .enumerate()
            {
                let target = ps.output.join(format!("check-default-{i}.json"));
                let mut doc = document(&ps.settings)?;
                doc["twitterapi_io"]["default_account"] = value;
                let raw = doc.to_string();
                fs::write(&target, &raw)?;
                ps.bad(
                    &format!(
                        "Set-PlumivoxPrivateAcl {}; Invoke-PlumivoxSetup -Check -SettingsPath {}",
                        qpath(&target),
                        qpath(&target)
                    ),
                    &[],
                )
                .await?;
                ensure!(fs::read_to_string(&target)? == raw);
            }
            Ok(())
        });
        case!("invalid-fingerprint-port-totp-and-missing-key-refused", {
            for (index, value) in [(6, "SHA256:not-verified"), (3, "0"), (12, "123456")] {
                let target = ps.output.join(format!("invalid-{index}.json"));
                let mut values = ps.valid();
                values[index] = value.into();
                ps.bad(&ps.target_invoke(&target), &values).await?;
                ensure!(!target.exists());
            }
            ps.bad(
                &format!(
                    "Invoke-PlumivoxSetup -SettingsPath {} -PrivateKeyPath {}",
                    qpath(&ps.output.join("missing.json")),
                    qpath(&ps.output.join("absent-key"))
                ),
                &ps.valid(),
            )
            .await?;
            Ok(())
        });
        case!("production-cli-errors-do-not-echo-secret-input", {
            let out = execute(
                Path::new("pwsh"),
                &[
                    "-NoProfile".into(),
                    "-File".into(),
                    ps.script.display().to_string(),
                    "-SettingsPath".into(),
                    ps.settings.display().to_string(),
                ],
                &[],
            )
            .await?;
            no_leak(&text(&out), &ps.secrets)?;
            ensure!(out.status.code() == Some(1));
            let out = execute(
                &binary(),
                &[
                    "--setup".into(),
                    "--check".into(),
                    "--settings-path".into(),
                    ps.output
                        .join("missing-native-settings.json")
                        .display()
                        .to_string(),
                ],
                &[
                    ("LOCALAPPDATA".into(), ps.local.display().to_string()),
                    ("PSExecutionPolicyPreference".into(), "Restricted".into()),
                ],
            )
            .await?;
            let text = text(&out);
            no_leak(&text, &ps.secrets)?;
            ensure!(
                out.status.code() == Some(1)
                    && text.contains("指定されたアカウント設定がありません")
                    && !["SecurityError", "PSSecurityException", "UnauthorizedAccess"]
                        .iter()
                        .any(|s| text.contains(s))
            );
            Ok(())
        });
        case!("observed-concurrent-settings-change-is-preserved", {
            let target = ps.output.join("race.json");
            fs::write(&target, "{\"unrelated\":\"original\"}")?;
            let changed = "{\"unrelated\":\"concurrent-change\"}";
            let hook = format!(
                "if ($Prompt -eq 'プロキシパスワード') {{ [IO.File]::WriteAllText({}, {}) }}",
                qpath(&target),
                quote(changed)
            );
            ensure!(
                ps.run(&ps.target_invoke(&target), &ps.valid(), &hook)
                    .await?
                    .status
                    .code()
                    == Some(1)
            );
            ensure!(fs::read_to_string(&target)? == changed);
            Ok(())
        });
        case!("check-refuses-invalid-proxy-and-broad-private-key-acl", {
            let mut doc = document(&ps.settings)?;
            doc["twitterapi_io"]["main"]["proxy"] = json!("http://example.invalid:3128");
            let target = ps.output.join("bad-proxy.json");
            fs::write(&target, doc.to_string())?;
            ps.bad(
                &format!(
                    "Set-PlumivoxPrivateAcl {}; Invoke-PlumivoxSetup -Check -SettingsPath {}",
                    qpath(&target),
                    qpath(&target)
                ),
                &[],
            )
            .await?;
            let out = execute(
                Path::new("pwsh"),
                &[
                    "-NoProfile".into(),
                    "-File".into(),
                    ps.script.display().to_string(),
                    "-Check".into(),
                    "-SettingsPath".into(),
                    target.display().to_string(),
                ],
                &[("LOCALAPPDATA".into(), ps.local.display().to_string())],
            )
            .await?;
            no_leak(&text(&out), &ps.secrets)?;
            ensure!(out.status.code() == Some(1) && text(&out).contains("認証付きプロキシURL"));
            let body = format!(
                "$acl=Get-Acl -LiteralPath {}\n$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'Read', 'Allow'))\n[IO.FileSystemAclExtensions]::SetAccessControl([IO.FileInfo]::new({}), $acl)\ntry {{ {} -Check }} finally {{ Set-PlumivoxPrivateAcl {} }}",
                qpath(&ps.key),
                qpath(&ps.key),
                ps.invoke(),
                qpath(&ps.key)
            );
            ps.bad(&body, &[]).await?;
            Ok(())
        });
        Ok(())
    }
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
        &json!({"notRun":"Windows PowerShell and macOS native setup require their actual OS"}),
    )?;
    suite.finish()
}
