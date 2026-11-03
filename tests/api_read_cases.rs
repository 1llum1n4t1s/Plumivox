use super::*;
#[cfg(windows)]
use anyhow::Context;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::time::Duration;

fn issued_feed_cursor(h: &Harness) -> Result<String> {
    if let Some(cursor) = h.cursor.as_ref().filter(|cursor| !cursor.is_empty()) {
        return Ok(cursor.clone());
    }
    ensure!(
        h.case_filter.is_some(),
        "The full suite must continue with the cursor issued by the feed case"
    );
    Ok(signed("plumivox-feed.", &feed_data()))
}

#[cfg(windows)]
async fn windows_acl_evidence(script: String) -> Result<Value> {
    let script = format!("$ErrorActionPreference='Stop'\n{script}");
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let output = tokio::process::Command::new("pwsh")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await?;
    ensure!(
        output.status.success(),
        "Synthetic Windows ACL operation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub async fn basics(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "Evidence path containment distinguishes dot-prefixed children",
        {
            for root in [
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                dirs::home_dir().unwrap().join(".codex"),
            ] {
                ensure!(root.join(".hidden").starts_with(&root));
                ensure!(
                    !root
                        .with_file_name(format!(
                            "{}-other",
                            root.file_name().unwrap().to_string_lossy()
                        ))
                        .starts_with(&root)
                );
            }
            let filesystem_root = h.output.ancestors().last().unwrap().to_path_buf();
            let root_error = crate::support::Suite::new_at(filesystem_root, "synthetic-root-guard")
                .err()
                .ok_or_else(|| {
                    anyhow::anyhow!("Filesystem root was accepted as evidence output")
                })?;
            ensure!(
                root_error.to_string().contains("filesystem root"),
                "Filesystem root was not rejected by the root guard: {root_error}"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                let public = h.output.join("synthetic-suite-public-root");
                std::fs::create_dir(&public)?;
                std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755))?;
                let before = std::fs::metadata(&public)?;
                ensure!(
                    before.uid() == unsafe { libc::geteuid() } && before.mode() & 0o777 == 0o755
                );
                ensure!(
                    crate::support::Suite::new_at(public.clone(), "synthetic-public-root").is_err(),
                    "Public evidence directory was accepted"
                );
                ensure!(
                    std::fs::metadata(&public)?.mode() == before.mode(),
                    "Rejected evidence directory permissions were changed"
                );
                ensure!(
                    std::fs::read_dir(&public)?.next().is_none(),
                    "Rejected evidence directory was written before validation"
                );
                let private = h.output.join("synthetic-suite-private-root");
                std::fs::create_dir(&private)?;
                std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))?;
                let metadata = std::fs::metadata(&private)?;
                ensure!(
                    metadata.uid() == unsafe { libc::geteuid() }
                        && metadata.mode() & 0o777 == 0o700
                );
                let accepted = crate::support::Suite::new_at(private, "synthetic-private-root")?;
                accepted.write_json("synthetic.json", &json!({"synthetic":true}))?;
            }
            #[cfg(windows)]
            {
                let shared = h.output.join("synthetic-suite-shared-root");
                let _initial = crate::support::Suite::new_at(
                    shared.clone(),
                    "synthetic-windows-private-root",
                )?;
                ensure!(std::fs::read_dir(&shared)?.next().is_none());
                drop(_initial);
                let quoted_shared = format!("'{}'", shared.to_string_lossy().replace('\'', "''"));
                // 全祖先がprivateの専用空dirだけに合成の共有ACEを追加する。
                // 所有者を先に照合し、所有者の変更や既存ACLの修復は行わない。
                let before = windows_acl_evidence(format!(r#"
$path={quoted_shared}
$user=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$acl=Get-Acl -LiteralPath $path
if($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $user){{throw 'Synthetic directory owner differs from current user'}}
$rule=[Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-1-0'),([Security.AccessControl.FileSystemRights]::Modify -bor [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles),([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow)
[void]$acl.AddAccessRule($rule)
[IO.FileSystemAclExtensions]::SetAccessControl([IO.DirectoryInfo]::new($path),$acl)
$acl=Get-Acl -LiteralPath $path
$shared=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])|Where-Object {{$_.IdentityReference.Value -eq 'S-1-1-0' -and $_.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and ($_.FileSystemRights -band [Security.AccessControl.FileSystemRights]::Modify) -eq [Security.AccessControl.FileSystemRights]::Modify}}).Count -gt 0
$deleteChild=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])|Where-Object {{$_.IdentityReference.Value -eq 'S-1-1-0' -and $_.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and ($_.FileSystemRights -band [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles) -ne 0}}).Count -gt 0
[pscustomobject]@{{sddl=$acl.Sddl;shared=$shared;deleteChild=$deleteChild;ownerMatches=($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -eq $user)}}|ConvertTo-Json -Compress
"#)).await?;
                ensure!(
                    before["ownerMatches"] == true
                        && before["shared"] == true
                        && before["deleteChild"] == true,
                    "Synthetic sharing ACE was not installed"
                );
                let rejected = crate::support::Suite::new_at(
                    shared.clone(),
                    "synthetic-windows-reject-shared",
                )
                .err()
                .ok_or_else(|| anyhow::anyhow!("Shared Windows evidence directory was accepted"))?;
                ensure!(
                    rejected.to_string().contains("DACL")
                        && rejected.to_string().contains("shared"),
                    "Shared directory was not rejected at its DACL boundary: {rejected}"
                );
                let after=windows_acl_evidence(format!("$acl=Get-Acl -LiteralPath {quoted_shared}\n[pscustomobject]@{{sddl=$acl.Sddl}}|ConvertTo-Json -Compress")).await?;
                ensure!(
                    after["sddl"] == before["sddl"],
                    "Rejected shared directory DACL was modified"
                );
                ensure!(
                    std::fs::read_dir(&shared)?.next().is_none(),
                    "Rejected shared directory was written"
                );
                let child = shared.join("synthetic-private-child");
                let accepted = crate::support::Suite::new_at(
                    child.clone(),
                    "synthetic-windows-new-private-child",
                )?;
                let quoted_child = format!("'{}'", child.to_string_lossy().replace('\'', "''"));
                let child_acl=windows_acl_evidence(format!(r#"
$user=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$acl=Get-Acl -LiteralPath {quoted_child}
$rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
$onlyOwner=$rules.Count -eq 1 -and $rules[0].IdentityReference.Value -eq $user -and $rules[0].AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and ($rules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::FullControl) -eq [Security.AccessControl.FileSystemRights]::FullControl
[pscustomobject]@{{ownerMatches=($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -eq $user);protected=$acl.AreAccessRulesProtected;onlyOwner=$onlyOwner;parentSddl=(Get-Acl -LiteralPath {quoted_shared}).Sddl}}|ConvertTo-Json -Compress
"#)).await?;
                ensure!(
                    child_acl["ownerMatches"] == true
                        && child_acl["protected"] == true
                        && child_acl["onlyOwner"] == true,
                    "New Windows evidence directory did not receive a protected owner-only DACL"
                );
                ensure!(
                    child_acl["parentSddl"] == before["sddl"],
                    "Creating a private child modified its shared parent DACL"
                );
                accepted.write_json("synthetic.json", &json!({"synthetic":true}))?;
                let moved_child = shared.join("synthetic-private-child-renamed");
                let moved_parent = h.output.join("synthetic-suite-shared-root-renamed");
                for (source, destination) in [(&child, &moved_child), (&shared, &moved_parent)] {
                    let error = std::fs::rename(source, destination).err().ok_or_else(|| {
                        anyhow::anyhow!("Suite lease allowed evidence directory replacement")
                    })?;
                    // Windowsは開いたディレクトリの改名をACCESS_DENIEDでも拒否する。
                    // 後段で同じ経路の改名がdrop後に成功することまで確認する。
                    ensure!(
                        matches!(error.raw_os_error(), Some(5 | 32)),
                        "Suite lease produced an unexpected rename failure for {}: {error}",
                        source.display()
                    );
                    ensure!(
                        source.exists() && !destination.exists(),
                        "Failed lease rename changed evidence paths"
                    );
                }
                drop(accepted);
                std::fs::rename(&child, &moved_child)
                    .context("Dropping Suite did not release child directory lease")?;
                std::fs::rename(&shared, &moved_parent)
                    .context("Dropping Suite did not release parent rename")?;
                ensure!(
                    moved_parent
                        .join("synthetic-private-child-renamed/synthetic.json")
                        .is_file()
                );
            }
        }
    );
    case!(h, "MCP discovery", {
        let tools = h.client.request("tools/list", json!({})).await?;
        let mut names = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        ensure!(
            names
                == vec![
                    "get_following_feed",
                    "get_my_posts",
                    "get_proxy_status",
                    "get_tweets",
                    "list_accounts",
                    "login_account",
                    "post_tweet",
                    "refresh_session",
                    "search_tweets",
                    "start_proxy",
                    "stop_proxy"
                ]
        );
    });
    case!(h, "Account metadata without secrets", {
        let v = h.ok("list_accounts", json!({})).await?;
        ensure!(
            v["accounts"].as_array().unwrap().len()
                == h.config["twitterapi_io"].as_object().unwrap().len()
        );
        let r = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["account"] == "reading")
            .unwrap();
        ensure!(r["readConfigured"] == true && r["postConfigured"] == false);
    });
    case!(
        h,
        "Omitted account without a configured default fails before HTTP",
        {
            ensure!(
                h.fail("search_tweets", json!({"keyword":"default account"}), true)
                    .await?["error"]["code"]
                    == "ACCOUNT_REQUIRED"
            );
        }
    );
    case!(
        h,
        "Persisted default routes omitted reads, searches, posts and reused login",
        {
            let mut c = h.config.clone();
            c["twitterapi_io"]["default_account"] = json!("posting");
            h.write_config(&c).await?;
            h.restart(250).await?;
            let v = h.ok("list_accounts", json!({})).await?;
            ensure!(
                v["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| a["isDefault"] == true)
                    .map(|a| a["account"].clone())
                    .collect::<Vec<_>>()
                    == vec![json!("posting")]
            );
            for (tool, args) in [
                ("get_my_posts", json!({})),
                ("search_tweets", json!({"keyword":"default account"})),
                ("post_tweet", json!({"text":"既定アカウントの検証"})),
                ("login_account", json!({})),
            ] {
                let v = h.ok(tool, args).await?;
                ensure!(v["account"] == "posting");
                if tool == "login_account" {
                    ensure!(v["reused"] == true);
                }
            }
            ensure!(
                h.ok("get_my_posts", json!({"account":"reading"})).await?["account"] == "reading"
            );
            ensure!(
                h.fail(
                    "post_tweet",
                    json!({"account":"reading","text":"明示先を既定へ置換しない"}),
                    true
                )
                .await?["error"]["code"]
                    == "POSTING_UNAVAILABLE"
            );
            c["twitterapi_io"]["default_account"] = json!("reading");
            h.write_config(&c).await?;
            ensure!(
                h.ok("search_tweets", json!({"keyword":"updated default"}))
                    .await?["account"]
                    == "reading"
            );
            h.restore().await?;
            h.restart(250).await?;
        }
    );
    h.restore().await?;
    case!(
        h,
        "Environment, embedded profile and explicit selection preserve default precedence",
        {
            for (env, expected) in [(Some("reading"), "reading"), (None, "posting")] {
                let mut c = h.config.clone();
                c["twitterapi_io"]["default_account"] = json!("auth");
                h.write_config(&c).await?;
                let environment = env
                    .map(|s| vec![("PLUMIVOX_DEFAULT_ACCOUNT".into(), s.into())])
                    .unwrap_or_default();
                let mut client = h
                    .spawn(&h.secrets, &h.root, 250, Some("posting"), &environment)
                    .await?;
                ensure!(
                    h.call_on(&client, "get_my_posts", json!({}), false, false)
                        .await?["account"]
                        == expected
                );
                ensure!(
                    h.call_on(
                        &client,
                        "get_my_posts",
                        json!({"account":"auth"}),
                        false,
                        false
                    )
                    .await?["account"]
                        == "auth"
                );
                let v = h
                    .call_on(&client, "list_accounts", json!({}), false, false)
                    .await?;
                ensure!(
                    v["accounts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|a| a["isDefault"] == true)
                        .map(|a| a["account"].clone())
                        .collect::<Vec<_>>()
                        == vec![json!(expected)]
                );
                client.close().await?;
            }
            h.restore().await?;
        }
    );
    h.restore().await?;
    case!(
        h,
        "Invalid configured defaults never fall back and explicit accounts still work",
        {
            for invalid in [
                json!(""),
                json!("missing"),
                Value::Null,
                json!(42),
                json!({"user_name":"NotAnAccount","api_key":KEY}),
            ] {
                h.write_config(&json!({"twitterapi_io":{"posting":h.config["twitterapi_io"]["posting"],"default_account":invalid}})).await?;
                let v = h.ok("list_accounts", json!({})).await?;
                ensure!(
                    v["accounts"].as_array().unwrap().len() == 1
                        && v["accounts"][0]["isDefault"] == false
                );
                ensure!(
                    h.fail("search_tweets", json!({"keyword":"invalid default"}), true)
                        .await?["error"]["code"]
                        == "ACCOUNT_DEFAULT_INVALID"
                );
                ensure!(
                    h.ok("get_my_posts", json!({"account":"posting"})).await?["account"]
                        == "posting"
                );
            }
            h.write_config(
                &json!({"twitterapi_io":{"posting":h.config["twitterapi_io"]["posting"]}}),
            )
            .await?;
            ensure!(h.ok("get_my_posts", json!({})).await?["account"] == "posting");
            h.restore().await?;
        }
    );
    h.restore().await?;
    case!(h, "Listed long account name can be selected through MCP", {
        let name = format!("profile-{}", "p".repeat(65));
        let list = h.ok("list_accounts", json!({})).await?;
        ensure!(
            list["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["account"] == name)
        );
        ensure!(
            h.ok("get_tweets", json!({"account":name,"tweetIds":["502"]}))
                .await?["account"]
                == name
        );
    });
    for (name, tool, args) in [
        (
            "Ambiguous account rejected before HTTP",
            "post_tweet",
            json!({"text":"hello"}),
        ),
        (
            "Missing API key rejected before HTTP",
            "get_my_posts",
            json!({"account":"broken"}),
        ),
    ] {
        case!(h, name, {
            h.fail(tool, args, true).await?;
        });
    }
    case!(h, "Cookie-free read and timeline pagination", {
        let v = h
            .ok(
                "get_my_posts",
                json!({"account":"reading","includeReplies":false}),
            )
            .await?;
        ensure!(
            v["account"] == "reading"
                && v["tweets"][0]["id"] == "501"
                && v["nextCursor"] == "timeline-next"
                && v["hasNextPage"] == true
        );
        h.ok(
            "get_my_posts",
            json!({"account":"reading","cursor":v["nextCursor"]}),
        )
        .await?;
        ensure!(h.trace().last().unwrap()["query"]["cursor"] == "timeline-next");
    });
    case!(
        h,
        "Nested real timeline response preserves top-level pagination",
        {
            h.state.scenario("timeline-nested");
            let before = h.count();
            let first = h.ok("get_my_posts", json!({"account":"reading"})).await?;
            ensure!(
                ids(&first) == ["504"]
                    && first["hasNextPage"] == true
                    && first["nextCursor"] == "nested-timeline-next"
            );
            let next = h
                .ok(
                    "get_my_posts",
                    json!({"account":"reading","includeReplies":true,"cursor":first["nextCursor"]}),
                )
                .await?;
            ensure!(
                ids(&next) == ["505"]
                    && next["hasNextPage"] == false
                    && next["nextCursor"].is_null()
            );
            let t = h.trace();
            ensure!(
                t.len() - before == 4
                    && t.last().unwrap()["query"]["includeParentTweet"] == "false"
                    && t.last().unwrap()["query"]["includeReplies"] == "true"
            );
        }
    );
    h.state.scenario("normal");
    for problem in [
        "timeline-nested-invalid",
        "timeline-nested-missing",
        "timeline-nested-array",
        "timeline-invalid-top",
    ] {
        case!(h, format!("{problem} response rejected"), {
            h.state.scenario(problem);
            ensure!(
                h.fail("get_my_posts", json!({"account":"reading"}), false)
                    .await?["error"]["code"]
                    == "INVALID_RESPONSE"
            );
        });
    }
    h.state.scenario("normal");
    case!(h, "Tweet lookup", {
        let ids20 = (502..522).map(|n| n.to_string()).collect::<Vec<_>>();
        for ids in [
            vec!["502".into()],
            ids20.clone(),
            [ids20[..19].to_vec(), vec!["502".into()]].concat(),
        ] {
            let before = h.count();
            let v = h
                .ok("get_tweets", json!({"account":"reading","tweetIds":ids}))
                .await?;
            ensure!(v["tweets"][0]["id"] == "502" && h.count() - before == 1);
            let mut seen = std::collections::HashSet::new();
            let unique = ids
                .iter()
                .filter(|s| seen.insert(*s))
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            ensure!(h.trace()[before]["query"]["tweet_ids"] == unique);
        }
    });
    case!(
        h,
        "UTF-8 response split within an emoji decodes correctly",
        {
            h.state.scenario("split-utf8");
            ensure!(
                h.ok(
                    "get_tweets",
                    json!({"account":"reading","tweetIds":["506"]})
                )
                .await?["tweets"][0]["text"]
                    == "🫱🏻‍🫲🏽 日本語"
            );
        }
    );
    h.state.scenario("normal");
    for operation in ["read", "post"] {
        case!(
            h,
            format!("Oversized {operation} response stops receiving without retry"),
            {
                h.state.scenario("oversized-response");
                let before = h.count();
                let v = h
                    .fail(
                        if operation == "read" {
                            "get_tweets"
                        } else {
                            "post_tweet"
                        },
                        if operation == "read" {
                            json!({"account":"reading","tweetIds":["502"]})
                        } else {
                            json!({"account":"posting","text":"bounded response probe"})
                        },
                        false,
                    )
                    .await?;
                ensure!(
                    v["error"]["code"]
                        == if operation == "read" {
                            "INVALID_RESPONSE"
                        } else {
                            "POST_UNCERTAIN"
                        }
                );
                ensure!(h.count() - before == 1);
                let stats = h.state.stats();
                crate::support::wait_until(Duration::from_secs(3), || {
                    let stats = stats.clone();
                    async move { Ok(stats.dropped.load(std::sync::atomic::Ordering::SeqCst)) }
                })
                .await?;
                ensure!(
                    !stats.complete.load(std::sync::atomic::Ordering::SeqCst)
                        && stats.sent.load(std::sync::atomic::Ordering::SeqCst) < 32 * 1024 * 1024
                );
            }
        );
    }
    h.state.scenario("normal");
    case!(h, "Non-numeric tweet ID rejected", {
        for ids in [
            json!(["bad"]),
            json!([(502..521)
                .map(|v| v.to_string())
                .chain(std::iter::once("bad".into()))
                .collect::<Vec<_>>()]),
        ] {
            let ids = if ids[0].is_array() {
                ids[0].clone()
            } else {
                ids
            };
            h.fail(
                "get_tweets",
                json!({"account":"reading","tweetIds":ids}),
                true,
            )
            .await?;
        }
    });
    Ok(())
}

fn assert_query(request: &Value) -> Result<()> {
    ensure!(
        request["path"] == "/twitter/tweet/advanced_search"
            && request["query"]["queryType"] == "Latest"
    );
    let q = request["query"]["query"].as_str().unwrap();
    ensure!(
        q.contains("\"needle phrase\"")
            && q.contains("since_time:1791802800")
            && q.contains("until_time:1791817200"),
        "Search constraints lost: {q}"
    );
    Ok(())
}
pub async fn feed_search(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "Feed date keys preserve invalid epoch negative and equal-date ordering",
        {
            h.state.scenario("feed-date-order");
            ensure!(
                ids(&h
                    .ok(
                        "get_following_feed",
                        json!({"account":"reading","maxAccounts":20})
                    )
                    .await?)
                    == ["70", "070", "60", "50", "40", "30", "20", "10", "80"]
            );
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Following selection scans late duplicates and preserves selected unknown counts",
        {
            h.state.scenario("following-selection");
            for (name, args) in [
                ("get_following_feed", json!({})),
                (
                    "search_tweets",
                    json!({"scope":"following","keyword":"needle phrase"}),
                ),
            ] {
                let before = h.count();
                let v = h
                    .ok(
                        name,
                        merge(json!({"account":"reading","maxAccounts":20}), args),
                    )
                    .await?;
                ensure!(
                    v["coverage"]["accounts"] == 20
                        && v["coverage"]["protectedAccountsExcluded"] == 3
                        && v["coverage"]["protectionStatusUnknownAccounts"] == 17
                );
                let trace = h.trace();
                let q = trace[before + 1]["query"]["query"].as_str().unwrap();
                ensure!(
                    q.contains("from:firstunknown")
                        && q.contains("from:knownfirst")
                        && !q.contains("pastlimit")
                        && !q.contains("from:private")
                );
                let requests = trace[before..]
                    .iter()
                    .filter(|r| r["path"].as_str().unwrap().ends_with("advanced_search"))
                    .collect::<Vec<_>>();
                ensure!(requests.len() == 1 && v["coverage"]["followingHasNextPage"] == true);
                let names = regex::Regex::new(r"from:([a-z0-9_]+)")
                    .unwrap()
                    .captures_iter(q)
                    .map(|c| c[1].to_owned())
                    .collect::<Vec<_>>();
                let expected = [
                    vec!["firstunknown".to_owned(), "knownfirst".to_owned()],
                    (0..18).map(|i| format!("chosen{i}")).collect(),
                ]
                .concat();
                ensure!(names == expected, "Selected account order/limit changed");
            }
        }
    );
    h.state.scenario("normal");
    case!(h, "Feed batches deduplicate and sort with coverage", {
        let before = h.count();
        let v = h
            .ok("get_following_feed", json!({"account":"reading"}))
            .await?;
        ensure!(
            v["mode"] == "reconstructed_following_feed"
                && v["officialHomeEquivalent"] == false
                && ids(&v) == ["602", "601", "603"]
                && v["coverage"]["protectedAccountsExcluded"] == 1
                && v["hasNextPage"] == true
        );
        h.cursor = v["nextCursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned);
        ensure!(
            h.cursor.is_some(),
            "Feed did not issue a nonempty continuation cursor"
        );
        let trace = h.trace();
        let searches = trace[before..]
            .iter()
            .filter(|v| v["path"].as_str().unwrap().ends_with("advanced_search"))
            .collect::<Vec<_>>();
        ensure!(searches.len() == 2);
        for r in searches {
            let q = r["query"]["query"].as_str().unwrap();
            ensure!(
                q.matches("from:").count() <= 20
                    && q.contains("include:nativeretweets")
                    && !q.contains("from:Private")
                    && r["query"]["queryType"] == "Latest"
            );
        }
    });
    case!(h, "Feed cursor account binding rejects before HTTP", {
        let cursor = issued_feed_cursor(h)?;
        h.fail(
            "get_following_feed",
            json!({"account":"posting","cursor":cursor}),
            true,
        )
        .await?;
    });
    case!(
        h,
        "Feed signed cursor rejects payload signature and unsigned legacy tampering",
        {
            let cursor = issued_feed_cursor(h)?;
            let parts = cursor.split('.').collect::<Vec<_>>();
            let mut d = decode(&cursor);
            ensure!(d["version"] == 2);
            d["batches"][0]["users"] = json!(["Outside"]);
            let changed = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&d)?);
            for c in [
                format!("plumivox-feed.{changed}.{}", parts[2]),
                format!("plumivox-feed.{}.a{}", parts[1], &parts[2][1..]),
            ] {
                h.fail(
                    "get_following_feed",
                    json!({"account":"reading","cursor":c}),
                    true,
                )
                .await?;
            }
            d["version"] = json!(1);
            h.fail("get_following_feed",json!({"account":"reading","cursor":URL_SAFE_NO_PAD.encode(serde_json::to_vec(&d)?)}),true).await?;
        }
    );
    case!(
        h,
        "Feed and search distinguish unknown protection flags from known private accounts",
        {
            h.state.scenario("unknown-following");
            for (name, args) in [
                ("get_following_feed", json!({})),
                (
                    "search_tweets",
                    json!({"scope":"following","keyword":"needle phrase"}),
                ),
            ] {
                let before = h.count();
                let v = h
                    .ok(name, merge(json!({"account":"reading"}), args))
                    .await?;
                ensure!(
                    v["coverage"]["protectedAccountsExcluded"] == 1
                        && v["coverage"]["protectionStatusUnknownAccounts"] == 2
                );
                let trace = h.trace();
                let q = trace[before + 1]["query"]["query"].as_str().unwrap();
                for part in [
                    "from:knownpublic",
                    "from:unknownpublic",
                    "from:otherunknown",
                ] {
                    ensure!(q.contains(part));
                }
                ensure!(!q.contains("from:private"));
            }
        }
    );
    h.state.scenario("normal");
    case!(h, "Malformed cursor rejects before HTTP", {
        h.fail(
            "get_following_feed",
            json!({"account":"reading","cursor":"bad-cursor!"}),
            true,
        )
        .await?;
    });
    case!(
        h,
        "Legacy feed and search cursors resume without changing signed wire format",
        {
            let first = h
                .ok(
                    "get_following_feed",
                    json!({"account":"reading","maxAccounts":20}),
                )
                .await?;
            let mut expected = feed_data();
            expected["batches"][0]["users"] =
                json!((0..20).map(|i| format!("public{i}")).collect::<Vec<_>>());
            expected["coverage"]["accounts"] = json!(20);
            expected["coverage"]["followingHasNextPage"] = json!(true);
            expected["coverage"]["protectedAccountsExcluded"] = json!(1);
            ensure!(
                first["nextCursor"] == signed("plumivox-feed.", &expected),
                "Previously issued feed wire format changed"
            );
            let feed = signed("plumivox-feed.", &feed_data());
            let before = h.count();
            h.ok(
                "get_following_feed",
                json!({"account":"reading","cursor":feed}),
            )
            .await?;
            ensure!(
                h.count() - before == 1
                    && h.trace().last().unwrap()["query"]["cursor"] == "search-next"
            );
            h.state.scenario("search-normal");
            let conditions = json!({"account":"reading","userName":"reader","scope":"all","keyword":"needle phrase","startEpoch":null,"endEpoch":null,"maxAccounts":null});
            let data = json!({"version":1,"kind":"tweets","conditions":conditions,"inner":"search-filter-next"});
            let cursor = signed("plumivox-search.", &data);
            let first = h
                .ok(
                    "search_tweets",
                    json!({"account":"reading","keyword":"needle phrase"}),
                )
                .await?;
            ensure!(
                first["nextCursor"] == cursor,
                "Previously issued search wire format changed"
            );
            let next = h
                .ok(
                    "search_tweets",
                    json!({"account":"reading","keyword":"needle phrase","cursor":cursor}),
                )
                .await?;
            ensure!(
                next["hasNextPage"] == false
                    && h.trace().last().unwrap()["query"]["cursor"] == "search-filter-next"
            );
            let mut nested = data;
            nested["conditions"]["scope"] = json!("following");
            nested["conditions"]["maxAccounts"] = json!(100);
            nested["inner"] = json!(signed("plumivox-feed.", &feed_data()));
            h.ok("search_tweets",json!({"account":"reading","keyword":"needle phrase","scope":"following","cursor":signed("plumivox-search.",&nested)})).await?;
            ensure!(h.trace().last().unwrap()["query"]["cursor"] == "search-next");
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Signed cursors reject noncanonical signatures malformed envelopes and wrong versions before HTTP",
        {
            for (tool, prefix, data, message, args) in [
                (
                    "get_following_feed",
                    "plumivox-feed.",
                    feed_data(),
                    "フィードのcursorが不正、または別アカウントのものです。",
                    json!({"account":"reading"}),
                ),
                (
                    "search_tweets",
                    "plumivox-search.",
                    json!({"version":1,"kind":"tweets","conditions":{"account":"reading","userName":"reader","scope":"all","keyword":"needle phrase","startEpoch":null,"endEpoch":null,"maxAccounts":null},"inner":"search-next"}),
                    "検索のcursorが不正、または検索条件・アカウントが一致しません。",
                    json!({"account":"reading","keyword":"needle phrase"}),
                ),
            ] {
                let cursor = signed(prefix, &data);
                let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
                let index = alphabet
                    .iter()
                    .position(|b| Some(b) == cursor.as_bytes().last())
                    .unwrap();
                let noncanonical = format!(
                    "{}{}",
                    &cursor[..cursor.len() - 1],
                    alphabet[index + 1] as char
                );
                let mut wrong_version = data.clone();
                wrong_version["version"] = json!(99);
                for bad in [
                    noncanonical,
                    format!("{cursor}.extra"),
                    cursor[..cursor.len() - 1].to_owned(),
                    signed(prefix, &wrong_version),
                ] {
                    let v = h
                        .fail(tool, merge(args.clone(), json!({"cursor":bad})), true)
                        .await?;
                    ensure!(
                        v["error"]["code"] == "INVALID_CURSOR" && v["error"]["message"] == message
                    );
                }
            }
        }
    );
    case!(h, "Feed continuation", {
        let cursor = issued_feed_cursor(h)?;
        ensure!(
            h.ok(
                "get_following_feed",
                json!({"account":"reading","cursor":cursor})
            )
            .await?["hasNextPage"]
                == false
        );
    });
    case!(h, "Empty following coverage", {
        h.state.scenario("empty");
        let v = h
            .ok("get_following_feed", json!({"account":"reading"}))
            .await?;
        ensure!(ids(&v).is_empty() && v["hasNextPage"] == false);
    });
    h.state.scenario("normal");
    case!(h, "Incomplete following list reported", {
        h.state.scenario("following-page");
        ensure!(
            h.ok("get_following_feed", json!({"account":"reading"}))
                .await?["coverage"]["followingHasNextPage"]
                == true
        );
    });
    h.state.scenario("normal");
    case!(
        h,
        "Following list continuation changes searched users and preserves partial coverage",
        {
            let before = h.count();
            let v = h
                .ok(
                    "get_following_feed",
                    json!({"account":"reading","followingCursor":"following-next"}),
                )
                .await?;
            ensure!(
                ids(&v) == ["604"]
                    && v["coverage"]["accounts"] == 1
                    && v["coverage"]["followingPage"] == "continuation"
                    && v["coverage"]["followingHasNextPage"] == false
                    && v["coverage"]["completeFollowingList"] == false
            );
            let trace = h.trace();
            ensure!(
                trace.len() - before == 2
                    && trace[before]["query"]["cursor"] == "following-next"
                    && trace[before + 1]["query"]["query"]
                        .as_str()
                        .unwrap()
                        .contains("from:laterpublic")
            );
        }
    );
    case!(
        h,
        "Following and search cursors together reject before HTTP",
        {
            let c = issued_feed_cursor(h)?;
            ensure!(
                h.fail(
                    "get_following_feed",
                    json!({"account":"reading","followingCursor":"following-next","cursor":c}),
                    true
                )
                .await?["error"]["code"]
                    == "INVALID_CURSOR"
            );
        }
    );
    cursor_budgets(h).await?;
    search_cases(h).await?;
    Ok(())
}

async fn cursor_budgets(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "Oversized provider cursors retain feed and global following search tweets",
        {
            h.state.scenario("cursor-provider-over");
            for (name, args) in [
                ("get_following_feed", json!({})),
                ("search_tweets", json!({"keyword":"needle phrase"})),
                (
                    "search_tweets",
                    json!({"keyword":"needle phrase","scope":"following"}),
                ),
            ] {
                unavailable(
                    &h.ok(name, merge(json!({"account":"reading"}), args))
                        .await?,
                )?;
            }
        }
    );
    h.state.scenario("normal");
    case!(h, "Oversized timeline cursor retains fetched posts", {
        h.state.scenario("cursor-timeline-over");
        unavailable(&h.ok("get_my_posts", json!({"account":"reading"})).await?)?;
    });
    h.state.scenario("normal");
    for (label, scenario, account) in [
        (
            "Oversized following list cursors retain coverage and tweets",
            "cursor-following-list-over",
            "reading".to_owned(),
        ),
        (
            "Signed following list wrapper over budget retains following search tweets",
            "cursor-following-wrapper-over",
            format!("cursor-profile-{}", "p".repeat(150000)),
        ),
    ] {
        case!(h, label, {
            h.state.scenario(scenario);
            let tools = if scenario == "cursor-following-list-over" {
                vec!["get_following_feed", "search_tweets"]
            } else {
                vec!["search_tweets"]
            };
            for name in tools {
                let args = if name == "get_following_feed" {
                    json!({"account":account})
                } else {
                    json!({"account":account,"keyword":"needle phrase","scope":"following"})
                };
                let v = h.ok(name, args).await?;
                ensure!(
                    !ids(&v).is_empty()
                        && v["coverage"]["followingHasNextPage"] == true
                        && v["coverage"]["followingNextCursor"].is_null()
                        && v["coverage"]["followingContinuationAvailable"] == false
                        && v["coverage"]["followingContinuationUnavailableReason"]
                            == "cursor_too_long"
                );
            }
        });
    }
    h.state.scenario("normal");
    case!(
        h,
        "Aggregate feed cursors over budget retain feed and following search tweets",
        {
            h.state.scenario("cursor-aggregate-over");
            for name in ["get_following_feed", "search_tweets"] {
                let before = h.count();
                let v=h.ok(name,if name=="get_following_feed"{json!({"account":"reading","maxAccounts":160})}else{json!({"account":"reading","maxAccounts":160,"keyword":"needle phrase","scope":"following"})}).await?;
                unavailable(&v)?;
                let t = h.trace();
                let r = t[before..]
                    .iter()
                    .filter(|r| r["path"].as_str().unwrap().ends_with("advanced_search"))
                    .collect::<Vec<_>>();
                ensure!(r.len() == 8 && r.iter().all(|r| r["query"].get("cursor").is_none()));
            }
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Signed search wrapper over budget retains global tweets",
        {
            h.state.scenario("cursor-wrapper-over");
            unavailable(&h.ok("search_tweets",json!({"account":format!("cursor-profile-{}","p".repeat(150000)),"keyword":"needle phrase"})).await?)?;
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Signed following search wrapper expansion retains tweets when inner feed cursor fits",
        {
            h.state.scenario("cursor-wrapper-following");
            let args = json!({"account":format!("feed-cursor-profile-{}","p".repeat(72000)),"maxAccounts":20});
            let feed = h.ok("get_following_feed", args.clone()).await?;
            ensure!(
                feed["nextCursor"]
                    .as_str()
                    .is_some_and(|s| s.len() <= 100000)
            );
            unavailable(
                &h.ok(
                    "search_tweets",
                    merge(args, json!({"scope":"following","keyword":"needle phrase"})),
                )
                .await?,
            )?;
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Provider cursors below budget remain resumable in global search",
        {
            h.state.scenario("cursor-provider-below");
            let v = h
                .ok(
                    "search_tweets",
                    json!({"account":"reading","keyword":"needle phrase"}),
                )
                .await?;
            ensure!(
                v["nextCursor"].is_string()
                    && v["coverage"]["tweetContinuationAvailable"] == true
                    && v["coverage"]["tweetContinuationUnavailableReason"].is_null()
            );
            let next = h
                .ok(
                    "search_tweets",
                    json!({"account":"reading","keyword":"needle phrase","cursor":v["nextCursor"]}),
                )
                .await?;
            ensure!(
                next["hasNextPage"] == false
                    && h.trace().last().unwrap()["query"]["cursor"]
                        .as_str()
                        .unwrap()
                        .len()
                        == 9500
            );
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Aggregate cursors below budget remain resumable in feed and following search",
        {
            h.state.scenario("cursor-aggregate-below");
            for name in ["get_following_feed", "search_tweets"] {
                let args = if name == "get_following_feed" {
                    json!({"account":"reading","maxAccounts":160})
                } else {
                    json!({"account":"reading","maxAccounts":160,"scope":"following","keyword":"needle phrase"})
                };
                let v = h.ok(name, args.clone()).await?;
                ensure!(
                    v["nextCursor"].is_string()
                        && v["coverage"]["tweetContinuationAvailable"] == true
                        && v["coverage"]["tweetContinuationUnavailableReason"].is_null()
                );
                let next = h
                    .ok(name, merge(args, json!({"cursor":v["nextCursor"]})))
                    .await?;
                ensure!(
                    next["hasNextPage"] == false
                        && h.trace().last().unwrap()["query"]["cursor"]
                            .as_str()
                            .unwrap()
                            .len()
                            == 8000
                );
            }
        }
    );
    h.state.scenario("normal");
    Ok(())
}
async fn prepare_search_cursor(h: &mut Harness, kind: &str) -> Result<String> {
    let saved = match kind {
        "global" => &h.search_cursor,
        "following" => &h.following_cursor,
        _ => &h.list_cursor,
    };
    if let Some(s) = saved {
        return Ok(s.clone());
    }
    h.state.scenario(if kind == "list" {
        "search-following-page"
    } else {
        "search-normal"
    });
    let v = h
        .ok(
            "search_tweets",
            merge(
                search_args(),
                if kind == "global" {
                    json!({})
                } else {
                    json!({"scope":"following"})
                },
            ),
        )
        .await?;
    let s = if kind == "list" {
        v["coverage"]["followingNextCursor"].as_str()
    } else {
        v["nextCursor"].as_str()
    }
    .unwrap()
    .to_owned();
    h.state.scenario("normal");
    Ok(s)
}
async fn search_cases(h: &mut Harness) -> Result<()> {
    case!(h, "Search global phrase and UTC period reach provider", {
        h.state.scenario("search-normal");
        let before = h.count();
        let v = h
            .ok(
                "search_tweets",
                merge(search_args(), json!({"keyword":"  needle phrase  "})),
            )
            .await?;
        ensure!(
            v["scope"] == "all"
                && v["keyword"] == "needle phrase"
                && v["startTime"] == "2026-10-12T11:00:00Z"
                && v["endTime"] == "2026-10-12T15:00:00Z"
        );
        let mut got = ids(&v);
        got.sort();
        ensure!(
            got == ["801", "802", "803", "804", "808"]
                && h.count() - before == 1
                && v["apiRequests"] == 1
        );
        assert_query(h.trace().last().unwrap())?;
        ensure!(
            v["hasNextPage"] == true
                && v["nextCursor"].is_string()
                && !h.trace().last().unwrap()["query"]["query"]
                    .as_str()
                    .unwrap()
                    .contains("from:")
        );
        h.search_cursor = v["nextCursor"].as_str().map(str::to_owned);
    });
    h.state.scenario("normal");
    case!(
        h,
        "Search global continuation preserves filters and equivalent offsets",
        {
            let cursor = prepare_search_cursor(h, "global").await?;
            h.state.scenario("search-normal");
            let v = h
            .ok(
                "search_tweets",
                merge(search_args(), json!({"cursor":cursor,"startTime":"2026-10-12T11:00:00Z","endTime":"2026-10-12T15:00:00Z"})),
            )
            .await?;
            ensure!(
                v["hasNextPage"] == false
                    && h.trace().last().unwrap()["query"]["cursor"] == "search-filter-next"
            );
            assert_query(h.trace().last().unwrap())?;
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Search following batches preserve filters deduplicate sort and exclude other authors",
        {
            h.state.scenario("search-normal");
            let before = h.count();
            let v = h
                .ok(
                    "search_tweets",
                    merge(search_args(), json!({"scope":"following"})),
                )
                .await?;
            ensure!(
                ids(&v) == ["802", "801", "803"] && v["coverage"]["protectedAccountsExcluded"] == 1
            );
            ensure!(v["tweets"].as_array().unwrap().iter().all(|t| {
                t["author"]["userName"]
                    .as_str()
                    .unwrap()
                    .starts_with("Public")
            }));
            let trace = h.trace();
            let requests = trace[before..]
                .iter()
                .filter(|r| r["path"].as_str().unwrap().ends_with("advanced_search"))
                .collect::<Vec<_>>();
            ensure!(requests.len() == 2);
            for r in requests {
                assert_query(r)?;
                ensure!(
                    r["query"]["query"]
                        .as_str()
                        .unwrap()
                        .contains("include:nativeretweets")
                        && !r["query"]["query"]
                            .as_str()
                            .unwrap()
                            .contains("from:Private")
                );
            }
            h.following_cursor = v["nextCursor"].as_str().map(str::to_owned);
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Search following tweet continuation preserves both batch conditions",
        {
            let cursor = prepare_search_cursor(h, "following").await?;
            h.state.scenario("search-normal");
            let before = h.count();
            let v = h
                .ok(
                    "search_tweets",
                    merge(search_args(), json!({"scope":"following","cursor":cursor})),
                )
                .await?;
            ensure!(v["hasNextPage"] == false && h.count() - before == 2);
            for r in &h.trace()[before..] {
                assert_query(r)?;
                ensure!(r["query"]["cursor"] == "search-filter-next");
            }
        }
    );
    h.state.scenario("normal");
    case!(h, "Search empty following avoids advanced search", {
        h.state.scenario("search-empty");
        let before = h.count();
        let v = h
            .ok(
                "search_tweets",
                merge(search_args(), json!({"scope":"following"})),
            )
            .await?;
        ensure!(
            ids(&v).is_empty()
                && v["hasNextPage"] == false
                && h.trace()[before..]
                    .iter()
                    .all(|r| !r["path"].as_str().unwrap().ends_with("advanced_search"))
        );
    });
    h.state.scenario("normal");
    case!(
        h,
        "Search incomplete following list exposes bound continuation",
        {
            h.state.scenario("search-following-page");
            let v = h
                .ok(
                    "search_tweets",
                    merge(search_args(), json!({"scope":"following"})),
                )
                .await?;
            ensure!(
                v["coverage"]["followingHasNextPage"] == true
                    && v["coverage"]["followingNextCursor"].is_string()
                    && v["coverage"]["followingNextCursor"] != "following-next"
            );
            h.list_cursor = v["coverage"]["followingNextCursor"]
                .as_str()
                .map(str::to_owned);
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Search following list continuation keeps phrase and period",
        {
            let cursor = prepare_search_cursor(h, "list").await?;
            h.state.scenario("search-normal");
            let before = h.count();
            let v = h
                .ok(
                    "search_tweets",
                    merge(
                        search_args(),
                        json!({"scope":"following","followingCursor":cursor}),
                    ),
                )
                .await?;
            ensure!(ids(&v) == ["808"] && h.trace()[before]["query"]["cursor"] == "following-next");
            assert_query(&h.trace()[before + 1])?;
            ensure!(
                h.trace()[before + 1]["query"]["query"]
                    .as_str()
                    .unwrap()
                    .contains("from:laterpublic")
            );
        }
    );
    h.state.scenario("normal");
    case!(
        h,
        "Search operator text stays a phrase inside following condition",
        {
            h.state.scenario("search-normal");
            let v = h
                .ok(
                    "search_tweets",
                    merge(
                        search_args(),
                        json!({"scope":"following","keyword":"needle OR from:Outside"}),
                    ),
                )
                .await?;
            ensure!(ids(&v) == ["809"]);
            let t = h.trace();
            let q = t.last().unwrap()["query"]["query"].as_str().unwrap();
            ensure!(
                q.contains("\"needle OR from:Outside\"")
                    && q.contains("since_time:1791802800")
                    && q.contains("until_time:1791817200")
            );
        }
    );
    h.state.scenario("normal");
    case!(h, "Search invalid keywords reject without HTTP", {
        for keyword in [
            "".into(),
            "   ".into(),
            "x".repeat(513),
            "quote\" OR from:Outside".into(),
            "back\\slash".into(),
            "line\nbreak".into(),
            "line\u{2028}break".into(),
        ] {
            h.fail(
                "search_tweets",
                merge(search_args(), json!({"keyword":keyword})),
                true,
            )
            .await?;
        }
    });
    case!(h, "Search invalid dates and ranges reject without HTTP", {
        for start in [
            "2026-10-12",
            "2026-02-30T11:00:00Z",
            "2026-10-12T25:00:00Z",
            "2026-10-12T11:00:00",
            "2026-10-12T11:00:00.5Z",
            "2026-10-12T11:00:00+24:00",
        ] {
            h.fail(
                "search_tweets",
                merge(search_args(), json!({"startTime":start})),
                true,
            )
            .await?;
        }
        for end in [
            "2026-10-12T20:00:00+09:00",
            "2026-10-12T10:00:00Z",
            "2026-02-30T15:00:00Z",
        ] {
            h.fail(
                "search_tweets",
                merge(search_args(), json!({"endTime":end})),
                true,
            )
            .await?;
        }
        for max in [json!(19), json!(201), json!(20.5)] {
            h.fail(
                "search_tweets",
                merge(
                    search_args(),
                    json!({"scope":"following","maxAccounts":max}),
                ),
                true,
            )
            .await?;
        }
    });
    case!(
        h,
        "Search all rejects following-only arguments without HTTP",
        {
            for extra in [
                json!({"maxAccounts":100}),
                json!({"followingCursor":"following-next"}),
            ] {
                h.fail("search_tweets", merge(search_args(), extra), true)
                    .await?;
            }
        }
    );
    let changes = [
        json!({"account":"posting"}),
        json!({"scope":"all"}),
        json!({"keyword":"other phrase"}),
        json!({"startTime":"2026-10-12T12:00:00Z"}),
        json!({"endTime":"2026-10-12T16:00:00Z"}),
        json!({"maxAccounts":20}),
    ];
    case!(
        h,
        "Search cursors bind account scope keyword period and following limit",
        {
            let cursor = prepare_search_cursor(h, "following").await?;
            let bound = merge(search_args(), json!({"scope":"following","cursor":cursor}));
            for change in &changes {
                h.fail("search_tweets", merge(bound.clone(), change.clone()), true)
                    .await?;
            }
            h.fail(
                "search_tweets",
                merge(bound, json!({"cursor":format!("a{}",&cursor[1..])})),
                true,
            )
            .await?;
        }
    );
    case!(
        h,
        "Search rejects mixed cursor kinds and old feed cursors",
        {
            let tweets = prepare_search_cursor(h, "following").await?;
            let following = prepare_search_cursor(h, "list").await?;
            let feed = issued_feed_cursor(h)?;
            for extra in [
                json!({"cursor":feed}),
                json!({"cursor":following}),
                json!({"followingCursor":tweets}),
                json!({"followingCursor":following,"cursor":tweets}),
                json!({"followingCursor":"following-next"}),
            ] {
                h.fail(
                    "search_tweets",
                    merge(merge(search_args(), json!({"scope":"following"})), extra),
                    true,
                )
                .await?;
            }
            h.fail(
                "get_following_feed",
                json!({"account":"reading","cursor":tweets}),
                true,
            )
            .await?;
            for change in &changes {
                h.fail(
                    "search_tweets",
                    merge(
                        merge(
                            search_args(),
                            json!({"scope":"following","followingCursor":following}),
                        ),
                        change.clone(),
                    ),
                    true,
                )
                .await?;
            }
        }
    );
    case!(h, "Search provider errors reject safely without retry", {
        for scenario in ["http500", "html", "search-malformed"] {
            h.state.scenario(scenario);
            let before = h.count();
            h.fail("search_tweets", search_args(), false).await?;
            ensure!(h.count() - before == 1);
        }
    });
    h.state.scenario("normal");
    case!(h, "Search cancellation does not resend", {
        h.state.scenario("search-cancel");
        let before = h.count();
        let p = h
            .client
            .start_request(
                "tools/call",
                json!({"name":"search_tweets","arguments":search_args()}),
            )
            .await?;
        h.state.accepted().await?;
        h.client.cancel(p.id).await?;
        ensure!(p.wait_timeout(Duration::from_millis(300)).await.is_err());
        tokio::time::sleep(Duration::from_millis(200)).await;
        ensure!(h.count() - before == 1);
    });
    h.state.scenario("normal");
    Ok(())
}
