use super::*;
use anyhow::Context;
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
const CANARIES: [&str; 7] = [
    KEY,
    COOKIE,
    PASSWORD,
    SEED,
    PROXY,
    "127.0.0.1",
    "ProxyError-private-code",
];
fn no_canary(text: &str) -> Result<()> {
    for canary in CANARIES {
        ensure!(!text.contains(canary), "Canary leaked: {canary}");
    }
    Ok(())
}
fn without_recorded(error: &Value) -> Value {
    let mut shown = error.clone();
    if let Some(o) = shown.as_object_mut() {
        o.remove("rejectionTextRecorded");
    }
    shown
}
/// 失敗応答に `rejectionTextRecorded: true` が付くのは記録できたときだけで、文面は応答に出ない。
fn check_response(v: &Value, recorded: bool) -> Result<()> {
    let e = &v["error"];
    if recorded {
        ensure!(e["rejectionTextRecorded"] == true, "Flag missing: {v}");
    } else {
        ensure!(
            e.get("rejectionTextRecorded").is_none(),
            "Unexpected flag: {v}"
        );
    }
    let raw = v.to_string();
    no_canary(&raw)?;
    ensure!(
        !raw.contains("private-rejection-canary") && !raw.contains("failed to connect"),
        "Refusal text appeared in the MCP response"
    );
    Ok(())
}
/// 拒否ケースの周で記録された9件（画像5件→投稿4件）の中身を確かめる。
async fn verify_refusal_records(h: &Harness) -> Result<()> {
    let (raw, records) = h.rejection_log_read().await?;
    no_canary(&raw)?;
    // 応答の他の欄（request_id・debug の中身）は読まない。キー名だけは bodyKeys に残る。
    ensure!(
        !raw.contains("credentials"),
        "Unread provider fields appeared in the record"
    );
    ensure!(records.iter().all(|r| r.get("requestId").is_none()));
    let expected = [
        ("image_upload", "Invalid login cookies"),
        ("image_upload", "ProxyError: failed to connect to proxy"),
        ("image_upload", "Credits is not enough.Please recharge"),
        ("image_upload", "ProxyError: read timed out"),
        ("image_upload", "Unrecognized private refusal"),
        ("post_creation", "ProxyError: failed to connect to proxy"),
        ("post_creation", "Could not extract tweet_id"),
        (
            "post_creation",
            "Oops, your request looks like it might be automated.",
        ),
        ("post_creation", "Unrecognized private refusal"),
    ];
    ensure!(records.len() == expected.len(), "Record count: {raw}");
    for (r, (stage, reason)) in records.iter().zip(expected) {
        let post = stage == "post_creation";
        let length = if post {
            "diagnostic post refusal".len()
        } else {
            "diagnostic refusal".len()
        };
        ensure!(
            r["schema"] == 1
                && r["tool"] == "post_tweet"
                && r["account"] == "posting"
                && r["proxyMode"].is_string()
                && r["stage"] == stage
                && r["postAttempted"] == post
                && r["code"] == "API_REJECTED"
                && r["httpStatus"] == 200
                && r["weightedLength"] == length
                && r["imageCount"] == usize::from(!post)
                && r.get("uploadAttempt").is_none() == post
                && (post || r["uploadAttempt"] == 1),
            "Unexpected record: {r}"
        );
        let text = r["texts"][0]["text"].as_str().unwrap_or_default();
        ensure!(
            text.contains(reason) && text.contains("<secret>"),
            "Reason or redaction missing ({reason}): {r}"
        );
    }
    Ok(())
}
pub async fn run(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "API refusal diagnostics keep fixed reasons and never disclose provider fields",
        {
            // 1周目は既定（設定なし）、2周目は期限内の有効化設定を置いた状態で同じ拒否を起こす。
            for recording in [false, true] {
                h.rejection_log_reset().await?;
                if recording {
                    h.rejection_log_enable(1).await?;
                }
                for (scenario, status, reasons, source) in [
                    (
                        "rejection-auth",
                        200,
                        json!(["authentication"]),
                        "provider_message",
                    ),
                    ("rejection-proxy", 200, json!(["proxy"]), "provider_message"),
                    (
                        "rejection-credits",
                        200,
                        json!(["credits"]),
                        "provider_message",
                    ),
                    (
                        "rejection-mixed",
                        200,
                        json!(["proxy", "upstream_timeout"]),
                        "provider_message",
                    ),
                    (
                        "rejection-unknown",
                        200,
                        json!(["unknown"]),
                        "provider_message",
                    ),
                    ("rejection-http402", 402, json!(["credits"]), "http_status"),
                ] {
                    h.state.scenario(scenario);
                    // 通常の短い画像時間枠では、1秒待つ再試行の余地がなく1要求で止まる。
                    let uploads = 1;
                    let before = h.count();
                    let logged = h.rejection_log_count().await?;
                    let v = h
                    .fail(
                        "post_tweet",
                        json!({"account":"posting","text":"diagnostic refusal","images":[h.png]}),
                        false,
                    )
                    .await?;
                    let shown = without_recorded(&v["error"]);
                    ensure!(
                        shown
                            == json!({
                                "code":if status==200{"API_REJECTED"}else{"API_HTTP_ERROR"},
                                "message":if status==200{"The API reported that the request failed."}else{"The API did not accept the request."},
                                "httpStatus":status,"rejectionReasons":reasons,"rejectionReasonSource":source,
                                "stage":"image_upload","postAttempted":false,"uploadAttempts":uploads
                            }),
                        "Unexpected safe refusal metadata"
                    );
                    // 2xx 本文エラーだけが記録され、2xx 以外（API_HTTP_ERROR）は記録されない。
                    let expect = recording && status == 200;
                    check_response(&v, expect)?;
                    ensure!(
                        h.rejection_log_count().await? == logged + usize::from(expect),
                        "Image refusal record count: {scenario}"
                    );
                    let requests = h.trace();
                    ensure!(
                        h.count() == before + uploads
                            && requests[before]["path"] == "/twitter/upload_media_v2"
                    );
                }
                for (scenario, code, source, reasons, next_step) in [
                    (
                        "rejection-proxy",
                        "API_REJECTED",
                        "provider_message",
                        json!(["proxy"]),
                        "Call get_my_posts for this account to check whether the post exists before calling post_tweet again. Do not resend repeatedly.",
                    ),
                    (
                        "rejection-noid",
                        "API_REJECTED",
                        "provider_message",
                        json!(["post_id_missing"]),
                        "The post may have been created. Do not call post_tweet again yet. Wait several minutes, then call get_my_posts for this account and look for this text. Resend only if it is still absent and the user still wants it; immediate resends have been rejected again.",
                    ),
                    (
                        "rejection-http502",
                        "POST_UNCERTAIN",
                        "http_status",
                        json!(["upstream_unavailable"]),
                        "The post may have been created. Do not call post_tweet again yet. Wait several minutes, then call get_my_posts for this account and look for this text. Resend only if it is still absent and the user still wants it; immediate resends have been rejected again.",
                    ),
                    (
                        "rejection-automated",
                        "API_REJECTED",
                        "provider_message",
                        json!(["automation_suspected"]),
                        "X appears to be limiting posts from this account. Do not resend right away. Tell the user, wait several minutes or longer, and call get_my_posts to check whether the post exists before any resend.",
                    ),
                    (
                        "rejection-unknown",
                        "API_REJECTED",
                        "provider_message",
                        json!(["unknown"]),
                        "Call get_my_posts for this account to check whether the post exists before calling post_tweet again. Do not resend repeatedly.",
                    ),
                ] {
                    h.state.scenario(scenario);
                    let before = h.count();
                    let logged = h.rejection_log_count().await?;
                    let v = h
                        .fail(
                            "post_tweet",
                            json!({"account":"posting","text":"diagnostic post refusal"}),
                            false,
                        )
                        .await?;
                    let e = &v["error"];
                    ensure!(
                        e["code"] == code
                            && e["stage"] == "post_creation"
                            && e["postAttempted"] == true
                            && e["uploadAttempts"] == 0
                    );
                    ensure!(
                        e["rejectionReasonSource"] == source && e["rejectionReasons"] == reasons
                    );
                    ensure!(e["nextStep"] == next_step);
                    // POST_UNCERTAIN（502）は2xx 以外なので記録されない。
                    let expect = recording && code == "API_REJECTED";
                    check_response(&v, expect)?;
                    ensure!(
                        h.rejection_log_count().await? == logged + usize::from(expect),
                        "Post refusal record count: {scenario}"
                    );
                    ensure!(
                        h.count() == before + 1
                            && h.trace()[before]["path"] == "/twitter/create_tweet_v2"
                    );
                    for secret in [
                        KEY,
                        COOKIE,
                        PASSWORD,
                        SEED,
                        PROXY,
                        "private-rejection-canary",
                        "ProxyError-private-code",
                    ] {
                        ensure!(!v.to_string().contains(secret), "Provider detail leaked");
                    }
                }
                h.state.scenario("rejection-auth");
                for (name, args) in [
                    (
                        "get_tweets",
                        json!({"account":"reading","tweetIds":["502"]}),
                    ),
                    ("refresh_session", json!({"account":"auth"})),
                ] {
                    let before = h.count();
                    let logged = h.rejection_log_count().await?;
                    let v = h.fail(name, args, false).await?;
                    // 読取り系とログインは、有効でも記録しない。
                    check_response(&v, false)?;
                    ensure!(h.rejection_log_count().await? == logged);
                    ensure!(
                        v["error"]
                            == json!({"code":"API_REJECTED","message":"The API reported that the request failed.","httpStatus":200,"rejectionReasons":["authentication"],"rejectionReasonSource":"provider_message"})
                    );
                    ensure!(h.count() == before + 1);
                }
                h.state.scenario("normal");
                if recording {
                    verify_refusal_records(h).await?;
                } else {
                    // 既定では記録も diagnostics フォルダーも作らない。
                    ensure!(
                        !h.diagnostics_dir().exists(),
                        "diagnostics created by default"
                    );
                }
                h.rejection_log_reset().await?;
            }
        }
    );
    case!(
        h,
        "Rejection log redacts post body, Cookie, user name and proxy before saving",
        {
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            h.state.scenario("rejection-leak");
            let text = "diagnostic leak body sentence";
            let v = h
                .fail(
                    "post_tweet",
                    json!({"account":"posting","text":text}),
                    false,
                )
                .await?;
            h.state.scenario("normal");
            check_response(&v, true)?;
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(records.len() == 1, "Expected one leak record: {raw}");
            ensure!(!raw.contains(text) && !raw.contains("leak body sentence"));
            ensure!(!raw.contains("Poster"), "User name remained in the record");
            no_canary(&raw)?;
            let r = &records[0];
            let saved = r["texts"][0]["text"].as_str();
            ensure!(
                saved.is_some_and(|t| t.contains("<post_text>") && t.contains("<secret>"))
                    || r["textDropped"].is_string(),
                "Body and Cookie were not replaced: {raw}"
            );
            ensure!(r["texts"][0]["field"] == "message");
            ensure!(r["weightedLength"] == text.len() && r["imageCount"] == 0);
        }
    );
    case!(
        h,
        "Rejection log keeps nothing when disabled, expired or over the 30 day limit",
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            for (at, until) in [
                (now - 200, now - 100),
                (now, now + 31 * 86_400),
                (now + 100, now + 50),
            ] {
                h.rejection_log_reset().await?;
                h.rejection_log_setting(at, until).await?;
                h.state.scenario("rejection-proxy");
                let v = h
                    .fail(
                        "post_tweet",
                        json!({"account":"posting","text":"disabled log"}),
                        false,
                    )
                    .await?;
                check_response(&v, false)?;
                ensure!(
                    !h.diagnostics_dir()
                        .join("provider-rejections.jsonl")
                        .exists(),
                    "Disabled setting still recorded: {at} {until}"
                );
            }
            h.state.scenario("normal");
            h.rejection_log_reset().await?;
        }
    );
    case!(
        h,
        "Rejection log records a post without an ID as POST_UNCERTAIN and a retried image once per request",
        {
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            h.state.scenario("missing-id");
            let v = h
                .fail(
                    "post_tweet",
                    json!({"account":"posting","text":"no id here"}),
                    false,
                )
                .await?;
            h.state.scenario("normal");
            ensure!(v["error"]["code"] == "POST_UNCERTAIN");
            check_response(&v, true)?;
            let (raw, records) = h.rejection_log_read().await?;
            ensure!(records.len() == 1, "Expected one record: {raw}");
            let r = &records[0];
            ensure!(
                r["code"] == "POST_UNCERTAIN"
                    && r["httpStatus"] == 200
                    && r["stage"] == "post_creation"
                    && r["postAttempted"] == true
                    && r.get("uploadAttempt").is_none(),
                "Unexpected missing-ID record: {raw}"
            );
            no_canary(&raw)?;
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            // 画像の unknown 拒否は1秒後に1回再試行され、要求ごとに1件ずつ残る。
            let mut c = h
                .spawn(&h.secrets, &h.root, 250, Some("--slow-image-upload"), &[])
                .await?;
            h.state.scenario("rejection-unknown");
            let before = h.count();
            let v = h
                .call_on(
                    &c,
                    "post_tweet",
                    json!({"account":"posting","text":"retry record","images":[h.png]}),
                    true,
                    false,
                )
                .await;
            h.state.scenario("normal");
            checked_close(&mut c).await?;
            let v = v?;
            ensure!(h.count() == before + 2, "Image was not retried once");
            ensure!(v["error"]["uploadAttempts"] == 2 && v["error"]["postAttempted"] == false);
            check_response(&v, true)?;
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(records.len() == 2, "Expected two records: {raw}");
            for (i, r) in records.iter().enumerate() {
                ensure!(
                    r["stage"] == "image_upload"
                        && r["postAttempted"] == false
                        && r["uploadAttempt"] == i + 1
                        && r["imageCount"] == 1
                        && r["code"] == "API_REJECTED",
                    "Unexpected retry record {i}: {raw}"
                );
            }
            no_canary(&raw)?;
        }
    );
    case!(
        h,
        "Recorded image refusal does not flag a later unrecorded post failure",
        {
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            // 画像の1回目は拒否されて記録され、再試行は成功し、その後の投稿作成が502で失敗する。
            let mut c = h
                .spawn(&h.secrets, &h.root, 250, Some("--slow-image-upload"), &[])
                .await?;
            h.state.scenario("upload-proxyreject-create502");
            let before = h.count();
            let v = h
                .call_on(
                    &c,
                    "post_tweet",
                    json!({"account":"posting","text":"later failure","images":[h.png]}),
                    true,
                    false,
                )
                .await;
            h.state.scenario("normal");
            checked_close(&mut c).await?;
            let v = v?;
            ensure!(h.count() == before + 3, "Unexpected request count");
            ensure!(
                v["error"]["code"] == "POST_UNCERTAIN"
                    && v["error"]["stage"] == "post_creation"
                    && v["error"]["uploadAttempts"] == 2,
                "Unexpected final error: {v}"
            );
            check_response(&v, false)?;
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(
                records.len() == 1
                    && records[0]["stage"] == "image_upload"
                    && records[0]["uploadAttempt"] == 1,
                "Expected one image record: {raw}"
            );
            no_canary(&raw)?;
        }
    );
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
                            == "The settings changed while preparing the relay. Run again."
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
                let v = h
                    .fail(
                        "post_tweet",
                        json!({"account":"posting","text":emoji.repeat(141)}),
                        true,
                    )
                    .await?;
                let e = &v["error"];
                ensure!(e["code"] == "INVALID_TEXT");
                ensure!(
                    e["weightedLength"] == 282
                        && e["maxWeightedLength"] == 280
                        && e["excessWeightedLength"] == 2
                );
                ensure!(e["stage"] == "input_validation" && e["postAttempted"] == false);
                ensure!(e.get("nextStep").is_none());
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
            let id = v["tweetId"].as_str().context("Missing string tweet ID")?;
            let url = v["url"].as_str().context("Missing string tweet URL")?;
            ensure!(
                v["weightedLength"] == length
                    && !id.is_empty()
                    && id.bytes().all(|b| b.is_ascii_digit())
                    && url.contains(id)
            );
        });
    }
    case!(h, "Explicit duplicate posts send twice", {
        let before = h.count();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let v = h
                .ok(
                    "post_tweet",
                    json!({"account":"posting","text":"explicit duplicate"}),
                )
                .await?;
            let id = v["tweetId"].as_str().context("Missing string tweet ID")?;
            ensure!(!id.is_empty());
            ids.push(id.to_owned());
        }
        ensure!(ids[0] != ids[1] && h.count() == before + 2);
        ensure!(
            h.trace()[before..]
                .iter()
                .filter(|r| r["method"] == "POST" && r["path"] == "/twitter/create_tweet_v2")
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
            2,
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
        // プロキシ拒否は同じ画像だけ1秒待って1回再試行し、成功後に投稿を1回だけ送る。
        invoke(h, "upload-proxyreject-recover", 2, 1, None, image.clone()).await?;
        invoke(
            h,
            "upload-proxyreject-always",
            2,
            0,
            Some("API_REJECTED"),
            image.clone(),
        )
        .await?;
        // 認証拒否は再試行しない。
        invoke(
            h,
            "upload-authreject-always",
            1,
            0,
            Some("API_REJECTED"),
            image.clone(),
        )
        .await?;
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
            // 通信切断だけは1回再試行する（待機1秒）。他は1要求で停止する。
            let n = if s == "upload-network" { 2 } else { 1 };
            invoke(h, s, n, 0, Some(code), image.clone()).await?;
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
                    m.contains("succeeded is unknown")
                        && m.contains("may have been posted")
                        && m.contains("not resent automatically")
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
