use super::*;
use std::{sync::atomic::Ordering, time::Duration};
use tokio::fs;

async fn invoke(
    h: &Harness,
    scenario: &str,
    uploads: usize,
    posts: usize,
    code: Option<&str>,
    images: Value,
) -> Result<Vec<Value>> {
    h.state.scenario(scenario);
    let before = h.count();
    let args = json!({"account":"posting","text":"upload recovery","images":images});
    let v = if code.is_some() {
        h.fail("post_tweet", args, false).await?
    } else {
        h.ok("post_tweet", args).await?
    };
    let requests = h.trace()[before..].to_vec();
    ensure!(
        requests
            .iter()
            .filter(|r| r["path"].as_str().unwrap().ends_with("upload_media_v2"))
            .count()
            == uploads,
        "Upload request count changed: {scenario}"
    );
    ensure!(
        requests
            .iter()
            .filter(|r| r["path"].as_str().unwrap().ends_with("create_tweet_v2"))
            .count()
            == posts,
        "Post request count changed: {scenario}"
    );
    if let Some(code) = code {
        let e = &v["error"];
        ensure!(
            e["code"] == code
                && e["stage"]
                    == if posts > 0 {
                        "post_creation"
                    } else {
                        "image_upload"
                    },
            "Wrong upload stage/error: {scenario}: {v}"
        );
        ensure!(e["postAttempted"] == (posts > 0) && e["uploadAttempts"] == uploads);
        if scenario == "upload-timeout" || scenario == "upload-network" {
            ensure!(
                e["failureKind"]
                    == if scenario == "upload-timeout" {
                        "timeout"
                    } else {
                        "network"
                    }
            );
        }
    }
    Ok(requests)
}
pub async fn run(h: &mut Harness) -> Result<()> {
    case!(h, "Missing Cookie rejected before HTTP", {
        let v = h
            .fail(
                "post_tweet",
                json!({"account":"reading","text":"hello"}),
                true,
            )
            .await?;
        ensure!(
            v["error"]["code"] == "POSTING_UNAVAILABLE"
                && v["error"]["stage"] == "pre_send"
                && v["error"]["postAttempted"] == false
                && v["error"]["uploadAttempts"] == 0
        );
    });
    case!(h, "Post preparation errors retain safe metadata", {
        for mode in ["--proxy-config-change", "--unknown-proxy-error"] {
            let mut c = h.spawn(&h.secrets, &h.root, 250, Some(mode), &[]).await?;
            let v = h
                .call_on(
                    &c,
                    "post_tweet",
                    json!({"account":"posting","text":"configuration recheck"}),
                    true,
                    true,
                )
                .await?;
            let e = &v["error"];
            if mode == "--proxy-config-change" {
                ensure!(
                    e["code"] == "AUTH_CONFIG_CHANGED"
                        && e["message"]
                            == "中継の準備中に設定が変更されました。再実行してください。"
                        && e["stage"] == "pre_send"
                        && e["postAttempted"] == false
                        && e["uploadAttempts"] == 0
                );
            } else {
                ensure!(e["code"] == "INTERNAL_ERROR");
                for field in ["stage", "postAttempted", "uploadAttempts"] {
                    ensure!(e.get(field).is_none());
                }
            }
            let unrelated = h
                .call_on(
                    &c,
                    "get_tweets",
                    json!({"account":"missing","tweetIds":["502"]}),
                    true,
                    true,
                )
                .await?;
            ensure!(unrelated["error"]["code"] == "ACCOUNT_UNKNOWN");
            for field in ["stage", "postAttempted", "uploadAttempts"] {
                ensure!(unrelated["error"].get(field).is_none());
            }
            checked_close(&mut c).await?;
        }
    });
    for (name, args) in [
        ("Empty text", json!({"text":""})),
        ("Japanese 141 characters", json!({"text":"あ".repeat(141)})),
        (
            "Relative image",
            json!({"text":"image","images":["relative.png"]}),
        ),
        (
            "Invalid image format",
            json!({"text":"image","images":[h.invalid]}),
        ),
        (
            "Invalid second image",
            json!({"text":"image","images":[h.png,h.invalid]}),
        ),
        (
            "Oversized image",
            json!({"text":"image","images":[h.oversized]}),
        ),
        (
            "More than four images",
            json!({"text":"image","images":vec![h.png.clone();5]}),
        ),
    ] {
        case!(h, format!("{name} rejected before HTTP"), {
            h.fail(
                "post_tweet",
                merge(json!({"account":"posting"}), args),
                true,
            )
            .await?;
        });
    }
    for (name, emoji) in [
        ("Mixed skin handshake", "🫱🏻‍🫲🏽"),
        ("Skin tone heart hands", "🫶🏻"),
        ("Head shaking", "🙂‍↔️"),
        ("Phoenix", "🐦‍🔥"),
        ("Direction walking", "🚶‍➡️"),
        ("Sark flag", "🇨🇶"),
    ] {
        case!(
            h,
            format!("{name} emoji reaches 280 weighted characters"),
            {
                ensure!(
                    h.ok(
                        "post_tweet",
                        json!({"account":"posting","text":emoji.repeat(140)})
                    )
                    .await?["weightedLength"]
                        == 280
                );
                ensure!(
                    h.fail(
                        "post_tweet",
                        json!({"account":"posting","text":emoji.repeat(141)}),
                        true
                    )
                    .await?["error"]["code"]
                        == "INVALID_TEXT"
                );
            }
        );
    }
    case!(h, "Modern emoji preserves URL and Japanese weights", {
        ensure!(h.ok("post_tweet",json!({"account":"posting","text":format!("あ🫱🏻‍🫲🏽 https://example.com/{}","a".repeat(400))})).await?["weightedLength"]==28);
    });
    case!(
        h,
        "Modern emoji does not hide invalid control characters",
        {
            ensure!(
                h.fail(
                    "post_tweet",
                    json!({"account":"posting","text":"🫱🏻‍🫲🏽\u{fffe}"}),
                    true
                )
                .await?["error"]["code"]
                    == "INVALID_TEXT"
            );
        }
    );
    case!(
        h,
        "Emoji weighting does not turn an invalid hostname into a URL",
        {
            ensure!(h.fail("post_tweet",json!({"account":"posting","text":format!("https://#️⃣.com/{}","a".repeat(300))}),true).await?["error"]["code"]=="INVALID_TEXT");
        }
    );
    for (name, text, length) in [
        ("Japanese 140 characters", "あ".repeat(140), 280),
        ("Compound family emoji", "👨‍👩‍👧‍👦".into(), 2),
        (
            "Long URL",
            format!("https://example.com/{}", "a".repeat(300)),
            23,
        ),
    ] {
        case!(h, name, {
            let v = h
                .ok("post_tweet", json!({"account":"posting","text":text}))
                .await?;
            let id = v["tweetId"].as_str().unwrap();
            ensure!(
                v["weightedLength"] == length
                    && id.bytes().all(|b| b.is_ascii_digit())
                    && v["url"].as_str().unwrap().contains(id)
            );
        });
    }
    case!(h, "Explicit duplicate posts send twice", {
        let before = h.count();
        for _ in 0..2 {
            h.ok(
                "post_tweet",
                json!({"account":"posting","text":"explicit duplicate"}),
            )
            .await?;
        }
        ensure!(
            h.trace()[before..]
                .iter()
                .filter(|r| r["path"].as_str().unwrap().ends_with("create_tweet_v2"))
                .count()
                == 2
        );
    });
    case!(h, "Image uses multipart upload then one post", {
        let r = invoke(h, "normal", 1, 1, None, json!([h.png])).await?;
        ensure!(
            r[0]["contentType"]
                .as_str()
                .unwrap()
                .starts_with("multipart/form-data; boundary=")
                && r[0]["multipartPayloadValid"] == true
                && r[1]["mediaIds"] == json!(["700"])
        );
    });
    h.state.scenario("normal");
    case!(
        h,
        "Slow image upload uses its own budget then creates one post",
        {
            let mut c = h
                .spawn(&h.secrets, &h.root, 250, Some("--slow-image-upload"), &[])
                .await?;
            h.state.scenario("upload-slow");
            let before = h.count();
            h.call_on(
                &c,
                "post_tweet",
                json!({"account":"posting","text":"slow image","images":[h.png]}),
                false,
                false,
            )
            .await?;
            ensure!(
                h.trace()[before..]
                    .iter()
                    .map(|v| v["path"].as_str().unwrap())
                    .collect::<Vec<_>>()
                    == ["/twitter/upload_media_v2", "/twitter/create_tweet_v2"]
            );
            checked_close(&mut c).await?;
        }
    );
    h.state.scenario("normal");
    case!(h, "Posting upload recovery and stage safety", {
        h.restart(1500).await?;
        let image = json!([h.png]);
        invoke(
            h,
            "upload-jsonerror",
            1,
            0,
            Some("API_REJECTED"),
            image.clone(),
        )
        .await?;
        for status in [502, 503, 504] {
            invoke(
                h,
                &format!("upload-recover-{status}"),
                2,
                1,
                None,
                image.clone(),
            )
            .await?;
            invoke(
                h,
                &format!("upload-always-{status}"),
                2,
                0,
                Some("API_HTTP_ERROR"),
                image.clone(),
            )
            .await?;
        }
        invoke(h, "upload-stream502", 2, 1, None, image.clone()).await?;
        let stats = h.state.stats();
        crate::support::wait_until(Duration::from_secs(1), || {
            let s = stats.clone();
            async move { Ok(s.dropped.load(Ordering::SeqCst)) }
        })
        .await?;
        ensure!(
            !stats.complete.load(Ordering::SeqCst),
            "Error stream was not cancelled"
        );
        for status in [401, 429, 500] {
            invoke(
                h,
                &format!("upload-always-{status}"),
                1,
                0,
                Some("API_HTTP_ERROR"),
                image.clone(),
            )
            .await?;
        }
        for (s, code) in [
            ("upload-html", "INVALID_RESPONSE"),
            ("upload-missing-id", "UPLOAD_FAILED"),
            ("upload-timeout", "API_UNAVAILABLE"),
            ("upload-network", "API_UNAVAILABLE"),
        ] {
            invoke(h, s, 1, 0, Some(code), image.clone()).await?;
        }
        let r = invoke(h, "upload-retry-order", 5, 1, None, json!(h.ordered)).await?;
        let mut hashes = vec![];
        for p in &h.ordered {
            hashes.push(sha(fs::read(p).await?));
        }
        ensure!(
            r[..5]
                .iter()
                .map(|v| v["uploadedImageHash"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
                == vec![
                    hashes[0].clone(),
                    hashes[1].clone(),
                    hashes[1].clone(),
                    hashes[2].clone(),
                    hashes[3].clone()
                ]
        );
        ensure!(r[5]["mediaIds"] == json!(["701", "703", "704", "705"]));
        invoke(h, "create502", 1, 1, Some("POST_UNCERTAIN"), image.clone()).await?;
        invoke(h, "jsonerror", 0, 1, Some("API_REJECTED"), json!([])).await?;
        invoke(h, "missing-id", 0, 1, Some("POST_UNCERTAIN"), json!([])).await?;
        h.state.scenario("normal");
        let v = h
            .fail(
                "post_tweet",
                json!({"account":"posting","text":"invalid image","images":[h.invalid]}),
                true,
            )
            .await?;
        ensure!(
            v["error"]["stage"] == "input_validation"
                && v["error"]["postAttempted"] == false
                && v["error"]["uploadAttempts"] == 0
        );
        h.restart(250).await?;
        invoke(
            h,
            "upload-always-502",
            1,
            0,
            Some("API_HTTP_ERROR"),
            image.clone(),
        )
        .await?;
        h.restart(1000).await?;
        let start = tokio::time::Instant::now();
        invoke(
            h,
            "upload-retry-time-budget",
            2,
            0,
            Some("API_UNAVAILABLE"),
            image,
        )
        .await?;
        ensure!(
            start.elapsed() < Duration::from_millis(1200),
            "Retry received a fresh timeout budget"
        );
        h.restart(250).await?;
    });
    h.state.scenario("normal");
    h.restart(250).await?;
    for (phase, count) in [("backoff", 1), ("second", 2), ("upload", 1)] {
        let name = if phase == "upload" {
            "MCP cancellation during upload prevents tweet creation".into()
        } else {
            format!("MCP cancellation during upload retry {phase} prevents further requests")
        };
        case!(h, name, {
            h.restart(1500).await?;
            h.state.scenario(if phase == "upload" {
                "cancel-upload"
            } else if phase == "backoff" {
                "upload-cancel-backoff"
            } else {
                "upload-cancel-second"
            });
            let before = h.count();
            let p=h.client.start_request("tools/call",json!({"name":"post_tweet","arguments":{"account":"posting","text":"cancel during upload","images":[h.png]}})).await?;
            h.state.accepted().await?;
            if phase == "backoff" {
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
            h.client.cancel(p.id).await?;
            ensure!(p.wait_timeout(Duration::from_millis(300)).await.is_err());
            tokio::time::sleep(Duration::from_millis(if phase == "upload" {
                220
            } else {
                350
            }))
            .await;
            let r = h.trace();
            ensure!(
                r[before..]
                    .iter()
                    .filter(|v| v["path"].as_str().unwrap().ends_with("upload_media_v2"))
                    .count()
                    == count
                    && r[before..]
                        .iter()
                        .all(|v| !v["path"].as_str().unwrap().ends_with("create_tweet_v2"))
            );
        });
        h.state.scenario("normal");
        h.restart(250).await?;
    }
    for s in ["image-order", "image-later-failure"] {
        case!(
            h,
            format!("Four image snapshots preserve order and stop on {s}"),
            {
                let fail = s == "image-later-failure";
                let r = invoke(
                    h,
                    s,
                    if fail { 3 } else { 4 },
                    if fail { 0 } else { 1 },
                    if fail { Some("UPLOAD_FAILED") } else { None },
                    json!(h.ordered),
                )
                .await?;
                for (i, p) in h.ordered.iter().take(if fail { 3 } else { 4 }).enumerate() {
                    ensure!(
                        r[i]["uploadedImageHash"] == sha(fs::read(p).await?)
                            && r[i]["multipartPayloadValid"] == true
                    );
                }
                if !fail {
                    ensure!(r.last().unwrap()["mediaIds"] == json!(["700", "701", "702", "703"]));
                }
            }
        );
    }
    h.state.scenario("normal");
    for problem in [
        "http401",
        "http429",
        "http500",
        "html",
        "jsonerror",
        "missing-id",
        "timeout",
    ] {
        case!(h, format!("{problem} is safe failure with no retry"), {
            h.state.scenario(problem);
            let before = h.count();
            let v = h
                .fail(
                    "post_tweet",
                    json!({"account":"posting","text":"failure probe"}),
                    false,
                )
                .await?;
            ensure!(h.count() - before == 1);
            let e = &v["error"];
            if ["http500", "html", "timeout"].contains(&problem) {
                ensure!(e["code"] == "POST_UNCERTAIN");
                let m = e["message"].as_str().unwrap();
                ensure!(
                    m.contains("成否は未確定")
                        && m.contains("投稿済みの可能性")
                        && m.contains("自動再送はしていません")
                );
            } else if ["http401", "http429"].contains(&problem) {
                ensure!(e["code"] == "API_HTTP_ERROR");
            } else if problem == "jsonerror" {
                ensure!(e["code"] == "API_REJECTED");
            }
        });
    }
    h.state.scenario("normal");
    Ok(())
}
