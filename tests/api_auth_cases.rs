use super::*;
use crate::support::McpClient;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::fs;

async fn seed(h: &Harness) -> Result<()> {
    if !h.own_session().exists() {
        h.ok("refresh_session", json!({"account":"auth"})).await?;
    }
    Ok(())
}
async fn pending_paths(directory: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    let mut entries = fs::read_dir(directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy().ends_with(".pending") {
            paths.insert(entry.path());
        }
    }
    Ok(paths)
}
async fn single(h: &Harness) -> Result<PathBuf> {
    let p = h.output.join("synthetic-single-account-secrets.json");
    fs::write(
        &p,
        json!({"twitterapi_io":{"auth":h.config["twitterapi_io"]["auth"]}}).to_string(),
    )
    .await?;
    Ok(p)
}
fn metadata(v: &Value, reused: bool) -> Result<()> {
    login_metadata(v, reused)?;
    ensure!(
        v["account"] == "auth"
            && v["userName"] == "AuthUser"
            && v["sessionSource"] == session_source()
    );
    Ok(())
}
async fn call_pending(
    h: &Harness,
    p: crate::support::PendingCall,
    code: Option<&str>,
) -> Result<Value> {
    let raw = p.wait().await?;
    let data = raw.get("structuredContent").cloned().unwrap_or_else(|| {
        serde_json::from_str(raw["content"][0]["text"].as_str().unwrap()).unwrap()
    });
    if let Some(code) = code {
        ensure!(
            raw["isError"] == true && data["error"]["code"] == code,
            "Unexpected pending authentication result: {data}"
        );
    } else {
        ensure!(raw["isError"] != true);
    }
    let text = raw.to_string();
    for s in [KEY, COOKIE, PASSWORD, SEED, PROXY] {
        ensure!(!text.contains(s), "Secret leaked");
    }
    let _ = h;
    Ok(data)
}
pub async fn run(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "Configuration accepts exact byte limit and rejects one extra byte",
        {
            let config = h.output.join("bounded-config.json");
            let root = h.output.join("bounded-config-state");
            let mut bytes = serde_json::to_vec(&json!({"twitterapi_io":{"auth":{}}}))?;
            bytes.resize(2 * 1024 * 1024, b' ');
            fs::write(&config, &bytes).await?;
            let mut c = h.spawn(&config, &root, 250, None, &[]).await?;
            h.call_on(&c, "list_accounts", json!({}), false, true)
                .await?;
            bytes.push(b' ');
            fs::write(&config, &bytes).await?;
            ensure!(
                h.call_on(&c, "list_accounts", json!({}), true, true)
                    .await?["error"]["code"]
                    == "CONFIG_UNAVAILABLE"
            );
            checked_close(&mut c).await?;
        }
    );
    for (label, tool, args) in [
        (
            "Login ambiguous account rejected before HTTP",
            "login_account",
            json!({}),
        ),
        (
            "Refresh ambiguous account rejected before HTTP",
            "refresh_session",
            json!({}),
        ),
        (
            "Login unknown account rejected before HTTP",
            "login_account",
            json!({"account":"unknown"}),
        ),
        (
            "Login missing API key rejected before HTTP",
            "login_account",
            json!({"account":"broken"}),
        ),
        (
            "Login missing credentials rejected before HTTP",
            "login_account",
            json!({"account":"noCredentials"}),
        ),
        (
            "Invalid TOTP seed rejected before HTTP",
            "login_account",
            json!({"account":"badSeed"}),
        ),
    ] {
        case!(h, label, {
            h.fail(tool, args, true).await?;
        });
    }
    case!(h, "Configured cookie login reuses without HTTP", {
        let before = h.count();
        let v = h.ok("login_account", json!({"account":"posting"})).await?;
        login_metadata(&v, true)?;
        ensure!(v["account"] == "posting" && h.count() == before);
    });
    case!(
        h,
        "Reused login inspects only its selected account after configuration reread",
        {
            let root = h.output.join("account-inspection-state");
            let config = h.output.join("account-inspection.json");
            let hooks = h.output.join("account-inspection-hooks.json");
            let counts = h.output.join("account-inspection-counts.json");
            let mut profiles = json!({"default_account":"selected"});
            for name in ["selected", "other1", "other2"] {
                profiles[name] = merge(
                    h.config["twitterapi_io"]["posting"].clone(),
                    json!({"user_name":name}),
                );
                if name != "selected" {
                    let directory = root.join(sha(format!("{name}\n{name}")));
                    fs::create_dir_all(&directory).await?;
                    fs::write(
                        directory.join(session_name()),
                        b"invalid synthetic encrypted session",
                    )
                    .await?;
                }
            }
            fs::write(&config, json!({"twitterapi_io":profiles}).to_string()).await?;
            fs::write(
                &hooks,
                json!({"counts_path":counts,"mutate_default_after_first_read":"other1"})
                    .to_string(),
            )
            .await?;
            let mut c = h
                .spawn(
                    &config,
                    &root,
                    250,
                    None,
                    &[("PLUMIVOX_TEST_HOOKS".into(), hooks.to_string_lossy().into())],
                )
                .await?;
            let before = h.count();
            let v = h
                .call_on(&c, "login_account", json!({}), false, false)
                .await?;
            login_metadata(&v, true)?;
            ensure!(v["account"] == "selected" && h.count() == before);
            for (tool, args) in [
                ("get_my_posts", json!({})),
                ("get_tweets", json!({"tweetIds":["502"]})),
                ("get_following_feed", json!({})),
                ("search_tweets", json!({"keyword":"needle"})),
            ] {
                ensure!(
                    h.call_on(
                        &c,
                        tool,
                        merge(json!({"account":"other1"}), args),
                        false,
                        false
                    )
                    .await?["account"]
                        == "other1"
                );
            }
            let list = h
                .call_on(&c, "list_accounts", json!({}), false, false)
                .await?;
            ensure!(
                list["accounts"].as_array().unwrap().len() == 3
                    && list["accounts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|a| a["isDefault"] == true)
                        .map(|a| a["account"].clone())
                        .collect::<Vec<_>>()
                        == vec![json!("other1")]
            );
            let mut expected = vec![
                json!({"name":"login_account","configReads":2,"sessionChecks":1,"sessionReads":0,"decryptSpawns":0}),
            ];
            for tool in [
                "get_my_posts",
                "get_tweets",
                "get_following_feed",
                "search_tweets",
            ] {
                expected.push(json!({"name":tool,"configReads":1,"sessionChecks":0,"sessionReads":0,"decryptSpawns":0}));
            }
            expected.push(json!({"name":"list_accounts","configReads":1,"sessionChecks":3,"sessionReads":2,"decryptSpawns":2}));
            ensure!(wait_json(&counts).await? == json!(expected));
            checked_close(&mut c).await?;
        }
    );
    case!(h, "Native decrypted output byte limit preserves UTF8", {
        let config = h.output.join("session-output.json");
        let root = h.output.join("session-output-state");
        let hooks = h.output.join("session-output-hooks.json");
        let mut profiles = json!({});
        for (name, offset) in [("below", -1), ("exact", 0), ("over", 1)] {
            profiles[name] = merge(
                h.config["twitterapi_io"]["posting"].clone(),
                json!({"user_name":name}),
            );
            let d = root.join(sha(format!("{name}\n{name}")));
            fs::create_dir_all(&d).await?;
            fs::write(
                d.join(session_name()),
                json!({"name":name,"bytes":2*1024*1024+offset}).to_string(),
            )
            .await?;
        }
        fs::write(&config, json!({"twitterapi_io":profiles}).to_string()).await?;
        fs::write(
            &hooks,
            json!({"decrypted_output_fixture":true,"binding":sha(format!("{KEY}\n{PROXY}"))})
                .to_string(),
        )
        .await?;
        let mut c = h
            .spawn(
                &config,
                &root,
                250,
                None,
                &[("PLUMIVOX_TEST_HOOKS".into(), hooks.to_string_lossy().into())],
            )
            .await?;
        let v = h
            .call_on(&c, "list_accounts", json!({}), false, false)
            .await?;
        let rows=v["accounts"].as_array().unwrap().iter().map(|v|json!({"account":v["account"],"postConfigured":v["postConfigured"],"sessionSource":v["sessionSource"]})).collect::<Vec<_>>();
        ensure!(
            rows == vec![
                json!({"account":"below","postConfigured":true,"sessionSource":session_source()}),
                json!({"account":"exact","postConfigured":true,"sessionSource":session_source()}),
                json!({"account":"over","postConfigured":false,"sessionSource":null})
            ]
        );
        checked_close(&mut c).await?;
    });
    case!(
        h,
        "Login saves OS-protected session and passes exact Base32 seed once",
        {
            let before = h.count();
            let v = h.ok("login_account", json!({"account":"auth"})).await?;
            metadata(&v, false)?;
            ensure!(
                h.count() - before == 1
                    && h.trace().last().unwrap()["loginPayloadValid"] == true
                    && h.trace().last().unwrap()["totpSeedExact"] == true
                    && h.trace().last().unwrap()["loginPayloadKeys"]
                        == json!(["email", "password", "proxy", "totp_secret", "user_name"])
            );
            let encrypted = fs::read(h.own_session()).await?;
            ensure!(
                encrypted.len() > 100
                    && !String::from_utf8_lossy(&encrypted)
                        .trim_start()
                        .starts_with('{')
            );
            for s in [KEY, COOKIE, PASSWORD, SEED, PROXY] {
                ensure!(!String::from_utf8_lossy(&encrypted).contains(s));
            }
            ensure!(fs::read(&h.secrets).await? == h.initial);
        }
    );
    case!(
        h,
        "Restart reloads encrypted session and reuses without HTTP",
        {
            seed(h).await?;
            h.restart(250).await?;
            let before = h.count();
            let v = h.ok("login_account", json!({"account":"auth"})).await?;
            metadata(&v, true)?;
            ensure!(
                h.count() == before
                    && chrono::DateTime::parse_from_rfc3339(v["acquiredAt"].as_str().unwrap())
                        .is_ok()
            );
            let list = h.ok("list_accounts", json!({})).await?;
            ensure!(
                list["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["account"] == "auth")
                    .unwrap()["postConfigured"]
                    == true
            );
        }
    );
    case!(
        h,
        "Single configured account is selected when login account is omitted",
        {
            seed(h).await?;
            let config = single(h).await?;
            let mut c = h.spawn(&config, &h.root, 250, None, &[]).await?;
            let before = h.count();
            metadata(
                &h.call_on(&c, "login_account", json!({}), false, false)
                    .await?,
                true,
            )?;
            ensure!(h.count() == before);
            checked_close(&mut c).await?;
        }
    );
    case!(
        h,
        "Explicit refresh logs in once and accepts plural cookie",
        {
            h.state.scenario("login-plural-cookie");
            let before = h.count();
            metadata(
                &h.ok("refresh_session", json!({"account":"auth"})).await?,
                false,
            )?;
            ensure!(
                h.count() - before == 1
                    && h.trace().last().unwrap()["path"] == "/twitter/user_login_v2"
                    && h.trace().last().unwrap()["totpSeedExact"] == true
                    && fs::read(&h.secrets).await? == h.initial
            );
        }
    );
    h.state.scenario("normal");
    for problem in [
        "http401",
        "http502",
        "http503",
        "http504",
        "html",
        "jsonerror",
        "login-missing-cookie",
        "timeout",
    ] {
        case!(
            h,
            format!("{problem} login failure retains encrypted session and does not retry"),
            {
                seed(h).await?;
                let previous = fs::read(h.own_session()).await?;
                h.state.scenario(problem);
                let before = h.count();
                h.fail("refresh_session", json!({"account":"auth"}), false)
                    .await?;
                ensure!(h.count() - before == 1 && fs::read(h.own_session()).await? == previous);
                h.state.scenario("normal");
                let before = h.count();
                metadata(
                    &h.ok("login_account", json!({"account":"auth"})).await?,
                    true,
                )?;
                ensure!(h.count() == before);
            }
        );
        h.state.scenario("normal");
    }
    case!(
        h,
        "Posting authentication failure retains saved cookie and does not retry",
        {
            seed(h).await?;
            let previous = fs::read(h.own_session()).await?;
            h.state.scenario("http401");
            let before = h.count();
            h.fail(
                "post_tweet",
                json!({"account":"auth","text":"authentication failure probe"}),
                false,
            )
            .await?;
            ensure!(h.count() - before == 1 && fs::read(h.own_session()).await? == previous);
            h.state.scenario("normal");
            let before = h.count();
            metadata(
                &h.ok("login_account", json!({"account":"auth"})).await?,
                true,
            )?;
            ensure!(h.count() == before);
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Session refresh after rejected post never resends the post",
        {
            let before = h.count();
            metadata(
                &h.ok("refresh_session", json!({"account":"auth"})).await?,
                false,
            )?;
            ensure!(
                h.count() - before == 1 && h.trace()[before]["path"] == "/twitter/user_login_v2"
            );
        }
    );
    case!(
        h,
        "Invalid login cookies reject safely without changing saved session",
        {
            seed(h).await?;
            let previous = fs::read(h.own_session()).await?;
            for s in [
                "login-cookie-newline",
                "login-cookie-nul",
                "login-cookie-utf8-over",
            ] {
                h.state.scenario(s);
                let before = h.count();
                ensure!(
                    h.fail("refresh_session", json!({"account":"auth"}), false)
                        .await?["error"]["code"]
                        == "LOGIN_FAILED"
                );
                ensure!(h.count() - before == 1 && fs::read(h.own_session()).await? == previous);
            }
        }
    );
    h.state.scenario("normal");
    gates(h).await?;
    storage_and_locks(h).await?;
    case!(
        h,
        "Changed API binding rejects saved cookie before posting without destroying original",
        {
            seed(h).await?;
            let previous = fs::read(h.own_session()).await?;
            let mut changed = h.config.clone();
            changed["twitterapi_io"]["auth"]["api_key"] = json!("synthetic-mismatched-api-key");
            let p = h.output.join("synthetic-mismatched-secrets.json");
            fs::write(&p, changed.to_string()).await?;
            let mut c = h.spawn(&p, &h.root, 250, None, &[]).await?;
            let mismatched = h
                .call_on(&c, "list_accounts", json!({}), false, true)
                .await?;
            ensure!(
                mismatched["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["account"] == "auth")
                    .unwrap()["postConfigured"]
                    == false,
                "Mismatched API binding was reported as posting-ready"
            );
            h.call_on(
                &c,
                "post_tweet",
                json!({"account":"auth","text":"binding mismatch"}),
                true,
                true,
            )
            .await?;
            ensure!(fs::read(h.own_session()).await? == previous);
            checked_close(&mut c).await?;
            let list = h.ok("list_accounts", json!({})).await?;
            ensure!(
                list["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["account"] == "auth")
                    .unwrap()["postConfigured"]
                    == true
            );
            let before = h.count();
            metadata(
                &h.ok("login_account", json!({"account":"auth"})).await?,
                true,
            )?;
            ensure!(
                h.count() == before,
                "Restored binding did not reuse the original session"
            );
        }
    );
    case!(
        h,
        "Invalid own session refuses legacy fallback while reading remains available",
        {
            let root = h.output.join("corrupted-auth-state");
            let mut c = h.spawn(&h.secrets, &root, 250, None, &[]).await?;
            let list = h
                .call_on(&c, "list_accounts", json!({}), false, true)
                .await?;
            ensure!(
                list["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["account"] == "posting")
                    .unwrap()["postConfigured"]
                    == false,
                "Corrupted own session was reported as posting-ready"
            );
            h.call_on(
                &c,
                "post_tweet",
                json!({"account":"posting","text":"corrupted own session"}),
                true,
                true,
            )
            .await?;
            let posts = h
                .call_on(
                    &c,
                    "get_my_posts",
                    json!({"account":"posting"}),
                    false,
                    false,
                )
                .await?;
            ensure!(posts["account"] == "posting" && posts["tweets"][0]["id"] == "501");
            checked_close(&mut c).await?;
        }
    );
    Ok(())
}

async fn gates(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "Cancellation while login HTTP waits reports unchanged session before commit",
        {
            seed(h).await?;
            let previous = fs::read(h.own_session()).await?;
            let mut g = h.gate("login").await?;
            h.state.scenario("interprocess-login");
            let before = h.count();
            let p = g
                .client
                .start_request(
                    "tools/call",
                    json!({"name":"refresh_session","arguments":{"account":"auth"}}),
                )
                .await?;
            h.state.accepted().await?;
            g.client.cancel(p.id).await?;
            ensure!(p.wait_timeout(Duration::from_millis(300)).await.is_err());
            ensure!(
                g.wait_result().await?["code"] == "AUTH_CANCELLED"
                    && fs::read(h.own_session()).await? == previous
                    && h.count() - before == 1
            );
            h.state.release.notify_waiters();
            checked_close(&mut g.client).await?;
        }
    );
    h.state.scenario("normal");
    for (label, phase, cancel, lose, expected) in [
        (
            "Cancellation during encryption reports unchanged session before commit",
            "encrypt",
            true,
            false,
            "AUTH_CANCELLED",
        ),
        (
            "Lease loss during encryption reports unchanged session before commit",
            "encrypt",
            false,
            true,
            "AUTH_LOCK_LOST",
        ),
        (
            "Cancellation during partial ciphertext write removes incomplete pending",
            "writing",
            true,
            false,
            "AUTH_CANCELLED",
        ),
        (
            "Lease loss during partial ciphertext write removes incomplete pending",
            "writing",
            false,
            true,
            "AUTH_LOCK_LOST",
        ),
        (
            "Cancellation after staging removes incomplete commit without retaining pending",
            "staged",
            true,
            false,
            "AUTH_CANCELLED",
        ),
        (
            "Lease loss after staging removes incomplete commit without retaining pending",
            "staged",
            false,
            true,
            "AUTH_LOCK_LOST",
        ),
        (
            "Lost saved acknowledgement reports uncertainty while committed session remains usable",
            "saved",
            false,
            true,
            "AUTH_SAVE_UNCERTAIN",
        ),
        (
            "Cancellation after commit reports uncertainty without undoing saved session",
            "saved",
            true,
            false,
            "AUTH_SAVE_UNCERTAIN",
        ),
    ] {
        case!(h, label, {
            seed(h).await?;
            let previous = fs::read(h.own_session()).await?;
            let mut g = h.gate(phase).await?;
            let pending_before = pending_paths(h.own_session().parent().unwrap()).await?;
            let before = h.count();
            let p = g
                .client
                .start_request(
                    "tools/call",
                    json!({"name":"refresh_session","arguments":{"account":"auth"}}),
                )
                .await?;
            ensure!(g.wait().await?["phase"] == phase);
            if phase == "writing" {
                let current = pending_paths(h.own_session().parent().unwrap()).await?;
                let created: Vec<_> = current.difference(&pending_before).collect();
                ensure!(created.len() == 1);
                // Windowsでは開いた書込みhandleのサイズがdirectory metadataへ
                // 反映されない場合があるため、別handleで実際のバイトを読む。
                ensure!(!fs::read(created[0]).await?.is_empty());
            }
            let current = fs::read(h.own_session()).await?;
            ensure!((current == previous) == (phase != "saved"));
            if lose {
                g.lose().await?;
            }
            if cancel {
                g.client.cancel(p.id).await?;
                ensure!(p.wait_timeout(Duration::from_millis(300)).await.is_err());
                ensure!(g.wait_result().await?["code"] == expected);
            } else {
                if phase != "saved" {
                    g.resume().await?;
                }
                call_pending(h, p, Some(expected)).await?;
            }
            if phase != "saved" {
                g.resume().await?;
            }
            ensure!(fs::read(h.own_session()).await? == current && h.count() - before == 1);
            checked_close(&mut g.client).await?;
            if phase == "staged" || phase == "writing" {
                ensure!(pending_paths(h.own_session().parent().unwrap()).await? == pending_before);
            }
            if phase == "saved" {
                let before = h.count();
                metadata(
                    &h.ok("login_account", json!({"account":"auth"})).await?,
                    true,
                )?;
                ensure!(h.count() == before);
            }
        });
    }
    Ok(())
}

async fn symlink_dir(target: &Path, alias: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, alias)?;
    }
    #[cfg(windows)]
    {
        #[path = "windows_junction.rs"]
        mod fixture;
        fixture::directory_junction(alias, target)?;
    }
    Ok(())
}
async fn storage_and_locks(h: &mut Harness) -> Result<()> {
    #[cfg(windows)]
    case!(
        h,
        "Failed replacement retains complete encrypted pending and previous session",
        {
            use std::os::windows::fs::OpenOptionsExt;
            let config = single(h).await?;
            let root = h.output.join("failed-replacement-state");
            let session = root.join(sha("auth\nauthuser")).join(session_name());
            let mut c = h.spawn(&config, &root, 15000, None, &[]).await?;
            metadata(
                &h.call_on(&c, "login_account", json!({"account":"auth"}), false, false)
                    .await?,
                false,
            )?;
            let previous = fs::read(&session).await?;
            // 読取り・書込みは許可し、置換に必要な削除共有だけを拒否する。
            let held = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(&session)?;
            ensure!(
                h.call_on(
                    &c,
                    "refresh_session",
                    json!({"account":"auth"}),
                    true,
                    false
                )
                .await?["error"]["code"]
                    == "AUTH_SAVE_UNCERTAIN"
            );
            drop(held);
            ensure!(fs::read(&session).await? == previous);
            let mut pending = Vec::new();
            let mut backups = Vec::new();
            let mut entries = fs::read_dir(session.parent().unwrap()).await?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_name().to_string_lossy().ends_with(".pending") {
                    pending.push(entry.path());
                }
                if entry.file_name().to_string_lossy().contains(".backup.") {
                    backups.push(entry.path());
                }
            }
            ensure!(pending.len() == 1 && backups.len() == 1);
            ensure!(fs::read(&backups[0]).await? == previous);
            checked_close(&mut c).await?;
            let mut c = h.spawn(&config, &root, 15000, None, &[]).await?;
            let before = h.count();
            metadata(
                &h.call_on(&c, "login_account", json!({"account":"auth"}), false, true)
                    .await?,
                true,
            )?;
            ensure!(h.count() == before && fs::read(&session).await? == previous);
            checked_close(&mut c).await?;
            // 保持した暗号文は同じアカウントへ復元すると本体で再利用できる。
            let pending_bytes = fs::read(&pending[0]).await?;
            ensure!(!pending_bytes.is_empty() && pending_bytes != previous);
            fs::write(&session, &pending_bytes).await?;
            let mut c = h.spawn(&config, &root, 15000, None, &[]).await?;
            let before = h.count();
            metadata(
                &h.call_on(&c, "login_account", json!({"account":"auth"}), false, true)
                    .await?,
                true,
            )?;
            ensure!(h.count() == before);
            checked_close(&mut c).await?;
        }
    );
    case!(
        h,
        "Actual storage failure retains storage error classification",
        {
            let blocked = h.output.join("auth-storage-blocked-file");
            fs::write(&blocked, b"This regular file cannot be a session directory").await?;
            let mut c = h.spawn(&h.secrets, &blocked, 250, None, &[]).await?;
            ensure!(
                h.call_on(&c, "refresh_session", json!({"account":"auth"}), true, true)
                    .await?["error"]["code"]
                    == "SESSION_STORAGE_UNAVAILABLE"
            );
            checked_close(&mut c).await?;
        }
    );
    case!(
        h,
        "Safe parent alias preserves encrypted session and native lock identity",
        {
            let real = h
                .output
                .join(format!("safe-session-parent-{}", "x".repeat(140)));
            let alias = h.output.join("safe-session-parent-alias");
            fs::create_dir(&real).await?;
            let config = single(h).await?;
            let real_state = real.join("state");
            let alias_state = alias.join("state");
            let mut direct = h.spawn(&config, &real_state, 15000, None, &[]).await?;
            metadata(
                &h.call_on(
                    &direct,
                    "login_account",
                    json!({"account":"auth"}),
                    false,
                    false,
                )
                .await?,
                false,
            )?;
            let relative = PathBuf::from(sha("auth\nauthuser")).join(session_name());
            #[cfg(windows)]
            ensure!(real_state.join(&relative).as_os_str().len() > 260);
            let previous = fs::read(real_state.join(&relative)).await?;
            symlink_dir(&real, &alias).await?;
            let mut aliased = h.spawn(&config, &alias_state, 15000, None, &[]).await?;
            let before = h.count();
            metadata(
                &h.call_on(
                    &aliased,
                    "login_account",
                    json!({"account":"auth"}),
                    false,
                    false,
                )
                .await?,
                true,
            )?;
            ensure!(
                h.count() == before && fs::read(alias_state.join(&relative)).await? == previous
            );
            h.state.scenario("interprocess-login");
            let before = h.count();
            let p = aliased
                .start_request(
                    "tools/call",
                    json!({"name":"refresh_session","arguments":{"account":"auth"}}),
                )
                .await?;
            h.state.accepted().await?;
            ensure!(
                h.call_on(
                    &direct,
                    "refresh_session",
                    json!({"account":"auth"}),
                    true,
                    true
                )
                .await?["error"]["code"]
                    == "AUTH_BUSY"
            );
            ensure!(h.count() - before == 1);
            h.state.release.notify_waiters();
            metadata(&call_pending(h, p, None).await?, false)?;
            let replacement = fs::read(real_state.join(&relative)).await?;
            ensure!(
                replacement != previous
                    && fs::read(alias_state.join(&relative)).await? == replacement
            );
            checked_close(&mut direct).await?;
            checked_close(&mut aliased).await?;
        }
    );
    h.state.release.notify_waiters();
    h.state.scenario("normal");
    case!(
        h,
        "Relocated plugin runtime refuses session storage inside its installation",
        {
            let plugin = h.output.join("relocated-plugin");
            let dist = plugin.join("dist");
            fs::create_dir_all(&dist).await?;
            let source = std::env::var_os("PLUMIVOX_NATIVE_FIXTURE")
                .or_else(|| option_env!("CARGO_BIN_EXE_plumivox-fixture").map(Into::into))
                .ok_or_else(|| anyhow::anyhow!("Missing fixture binary"))?;
            let source = PathBuf::from(source);
            let executable = dist.join(source.file_name().unwrap());
            fs::copy(source, &executable).await?;
            fs::write(plugin.join("plugin.json"), b"{}").await?;
            fs::write(plugin.join("mcp.json"), b"{}").await?;
            let config = single(h).await?;
            let root = plugin.join("state");
            let args = vec![
                h.server.base_url.clone(),
                config.to_string_lossy().into(),
                root.to_string_lossy().into(),
                "1500".into(),
            ];
            let mut c = McpClient::spawn_command(&executable, &args, &[]).await?;
            ensure!(
                h.call_on(&c, "login_account", json!({"account":"auth"}), true, true)
                    .await?["error"]["code"]
                    == "SESSION_STORAGE_UNAVAILABLE"
                    && !root.exists()
            );
            checked_close(&mut c).await?;
        }
    );
    case!(
        h,
        "Session storage refuses Codex and Claude user directories",
        {
            // Codex専用ではないため、Claude Codeの既定名・環境変数の移動先も拒否する。
            let executable = PathBuf::from(
                std::env::var_os("PLUMIVOX_NATIVE_FIXTURE")
                    .or_else(|| option_env!("CARGO_BIN_EXE_plumivox-fixture").map(Into::into))
                    .ok_or_else(|| anyhow::anyhow!("Missing fixture binary"))?,
            );
            let config = single(h).await?;
            let claude_home = h.output.join("relocated-claude-config");
            fs::create_dir_all(&claude_home).await?;
            let env = [(
                "CLAUDE_CONFIG_DIR".to_string(),
                claude_home.to_string_lossy().into_owned(),
            )];
            for root in [
                h.output.join(".codex").join("state"),
                h.output.join(".claude").join("state"),
                claude_home.join("state"),
            ] {
                let args = vec![
                    h.server.base_url.clone(),
                    config.to_string_lossy().into(),
                    root.to_string_lossy().into(),
                    "1500".into(),
                ];
                let mut c = McpClient::spawn_command(&executable, &args, &env).await?;
                ensure!(
                    h.call_on(&c, "login_account", json!({"account":"auth"}), true, true)
                        .await?["error"]["code"]
                        == "SESSION_STORAGE_UNAVAILABLE"
                        && !root.exists(),
                    "Session storage was accepted inside an agent user directory"
                );
                checked_close(&mut c).await?;
            }
        }
    );
    case!(
        h,
        "Concurrent same-account refresh refuses second login before HTTP",
        {
            h.state.scenario("login-delay");
            let p = h
                .client
                .start_request(
                    "tools/call",
                    json!({"name":"refresh_session","arguments":{"account":"auth"}}),
                )
                .await?;
            h.state.accepted().await?;
            ensure!(
                h.fail("refresh_session", json!({"account":"auth"}), true)
                    .await?["error"]["code"]
                    == "AUTH_BUSY"
            );
            metadata(&call_pending(h, p, None).await?, false)?;
        }
    );
    h.state.scenario("normal");
    for (label, action) in [
        (
            "Independent MCP processes serialize same-account refresh before login API",
            "contend",
        ),
        (
            "Stopping mutex owner process permits another MCP process to refresh",
            "stop",
        ),
        (
            "Losing mutex child while MCP survives prevents stale session overwrite",
            "lose",
        ),
    ] {
        case!(h, label, {
            seed(h).await?;
            let previous = fs::read(h.own_session()).await?;
            let hook = h.output.join(format!(
                "native-owner-hooks-{}.json",
                rand::random::<u128>()
            ));
            let lose = hook.with_extension("lose-lock");
            fs::write(&hook, json!({"lose_lock_path":lose}).to_string()).await?;
            let mut owner = h
                .spawn(
                    &h.secrets,
                    &h.root,
                    15000,
                    None,
                    &[("PLUMIVOX_TEST_HOOKS".into(), hook.to_string_lossy().into())],
                )
                .await?;
            let mut contender = h.spawn(&h.secrets, &h.root, 15000, None, &[]).await?;
            h.state.scenario("interprocess-login");
            let before = h.count();
            let p = owner
                .start_request(
                    "tools/call",
                    json!({"name":"refresh_session","arguments":{"account":"auth"}}),
                )
                .await?;
            h.state.accepted().await?;
            let owner_id = owner
                .process_id()
                .ok_or_else(|| anyhow::anyhow!("Owner process is not running"))?;
            ensure!(owner_id > 0);
            if action == "contend" {
                ensure!(
                    h.call_on(
                        &contender,
                        "refresh_session",
                        json!({"account":"auth"}),
                        true,
                        true
                    )
                    .await?["error"]["code"]
                        == "AUTH_BUSY"
                );
                ensure!(h.state.mock.lock().unwrap().peak_logins == 1);
                h.state.release.notify_waiters();
                metadata(&call_pending(h, p, None).await?, false)?;
                ensure!(h.count() - before == 1);
            } else {
                if action == "stop" {
                    checked_close(&mut owner).await?;
                    ensure!(
                        owner.process_id().is_none(),
                        "Closed owner process is still present"
                    );
                    match p.wait().await {
                        Ok(v) => {
                            let d = v.get("structuredContent").cloned().unwrap_or_else(|| {
                                serde_json::from_str(v["content"][0]["text"].as_str().unwrap())
                                    .unwrap()
                            });
                            ensure!(v["isError"] == true && d["error"]["code"] == "AUTH_CANCELLED");
                        }
                        Err(_) => {}
                    }
                } else {
                    fs::write(&lose, b"lose").await?;
                    call_pending(h, p, Some("AUTH_LOCK_LOST")).await?;
                    ensure!(
                        owner.process_id() == Some(owner_id),
                        "MCP parent did not survive lock loss"
                    );
                    ensure!(
                        owner.request("tools/list", json!({})).await?["tools"]
                            .as_array()
                            .unwrap()
                            .len()
                            == 11
                            && fs::read(h.own_session()).await? == previous
                    );
                }
                h.state.scenario("normal");
                metadata(
                    &h.call_on(
                        &contender,
                        "refresh_session",
                        json!({"account":"auth"}),
                        false,
                        false,
                    )
                    .await?,
                    false,
                )?;
                let replacement = fs::read(h.own_session()).await?;
                ensure!(replacement != previous);
                h.state.release.notify_waiters();
                tokio::time::sleep(Duration::from_millis(30)).await;
                ensure!(
                    fs::read(h.own_session()).await? == replacement
                        && h.trace()[before..]
                            .iter()
                            .filter(|r| r["path"] == "/twitter/user_login_v2")
                            .count()
                            == 2
                        && h.trace()[before..]
                            .iter()
                            .all(|r| r["path"] != "/twitter/create_tweet_v2")
                );
            }
            checked_close(&mut owner).await?;
            checked_close(&mut contender).await?;
        });
        h.state.release.notify_waiters();
        h.state.scenario("normal");
    }
    Ok(())
}
