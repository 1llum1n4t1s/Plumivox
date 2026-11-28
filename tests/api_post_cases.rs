use super::*;
use crate::support::McpClient;
use anyhow::Context;
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};
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
/// 拒否ケースの周で記録された11件（画像6件→投稿5件）の中身を確かめる。
async fn verify_refusal_records(h: &Harness) -> Result<()> {
    let (raw, records) = h.rejection_log_read().await?;
    no_canary(&raw)?;
    // 応答の他の欄（request_id・debug の中身）は読まない。キー名だけは bodyKeys に残る。
    ensure!(
        !raw.contains("credentials"),
        "Unread provider fields appeared in the record"
    );
    ensure!(records.iter().all(|r| r.get("requestId").is_none()));
    let rejected = ("API_REJECTED", 200);
    let expected = [
        ("image_upload", "Invalid login cookies", rejected),
        (
            "image_upload",
            "ProxyError: failed to connect to proxy",
            rejected,
        ),
        (
            "image_upload",
            "Credits is not enough.Please recharge",
            rejected,
        ),
        ("image_upload", "ProxyError: read timed out", rejected),
        ("image_upload", "Unrecognized private refusal", rejected),
        (
            "image_upload",
            "Credits is not enough.Please recharge",
            ("API_HTTP_ERROR", 402),
        ),
        (
            "post_creation",
            "ProxyError: failed to connect to proxy",
            rejected,
        ),
        ("post_creation", "Could not extract tweet_id", rejected),
        ("post_creation", "Bad gateway", ("POST_UNCERTAIN", 502)),
        (
            "post_creation",
            "Oops, your request looks like it might be automated.",
            rejected,
        ),
        ("post_creation", "Unrecognized private refusal", rejected),
    ];
    ensure!(records.len() == expected.len(), "Record count: {raw}");
    for (r, (stage, reason, (code, status))) in records.iter().zip(expected) {
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
                && r["code"] == code
                && r["httpStatus"] == status
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
                    // 記録が有効なら、2xx 本文エラーも2xx 以外（API_HTTP_ERROR）も1件ずつ記録される。
                    let expect = recording;
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
                        "The post may have been created. Do not call post_tweet again yet. Wait several minutes, then call get_my_posts for this account and look for this text. Resend only if it is still absent and the user still wants it, and if the response has retryAfter, only after that time (you may call get_my_posts before it); immediate resends have been rejected again.",
                    ),
                    (
                        "rejection-http502",
                        "POST_UNCERTAIN",
                        "http_status",
                        json!(["upstream_unavailable"]),
                        "The post may have been created. Do not call post_tweet again yet. Wait several minutes, then call get_my_posts for this account and look for this text. Resend only if it is still absent and the user still wants it, and if the response has retryAfter, only after that time (you may call get_my_posts before it); immediate resends have been rejected again.",
                    ),
                    (
                        "rejection-automated",
                        "API_REJECTED",
                        "provider_message",
                        json!(["automation_suspected"]),
                        "X appears to be limiting posts from this account. Do not resend right away. If the response has retryAfter, do not call post_tweet again until that time and tell the user. Tell the user, wait several minutes or longer, and call get_my_posts to check whether the post exists before any resend.",
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
                    // POST_UNCERTAIN（502）も、記録が有効なら本文を読んで記録される。
                    let expect = recording;
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
        "Provider numeric codes classify X refusals and HTTP error integers are not X codes",
        {
            h.rejection_log_reset().await?;
            // (場面, 最終エラー, 分類, 分類の出所, xErrorCode, xHttpStatus, xRejected)
            let cases = [
                (
                    "rejection-code226",
                    "API_REJECTED",
                    json!(["automation_suspected"]),
                    "provider_code",
                    json!(226),
                    Value::Null,
                    true,
                ),
                (
                    "rejection-code344",
                    "API_REJECTED",
                    json!(["daily_limit"]),
                    "provider_code",
                    json!(344),
                    Value::Null,
                    true,
                ),
                (
                    "rejection-code344s",
                    "API_REJECTED",
                    json!(["daily_limit"]),
                    "provider_code",
                    json!(344),
                    Value::Null,
                    true,
                ),
                // 表にないコードは分類を増やさず、応答にも出さない（X の拒否とだけ分かる）。
                (
                    "rejection-codeunknown",
                    "API_REJECTED",
                    json!(["unknown"]),
                    "provider_message",
                    Value::Null,
                    Value::Null,
                    true,
                ),
                (
                    "rejection-xstatus",
                    "API_REJECTED",
                    json!(["rate_limit"]),
                    "provider_code",
                    Value::Null,
                    json!(429),
                    true,
                ),
                // HTTP 400 の `{error:int}` は提供元自身のコードかもしれないので X のコードにしない。
                (
                    "rejection-http400",
                    "API_HTTP_ERROR",
                    json!(["invalid_request"]),
                    "http_status",
                    Value::Null,
                    Value::Null,
                    false,
                ),
            ];
            for (scenario, code, reasons, source, x_code, x_status, x_rejected) in cases {
                h.state.scenario(scenario);
                let before = h.count();
                let v = h
                    .fail(
                        "post_tweet",
                        json!({"account":"posting","text":"provider code refusal"}),
                        false,
                    )
                    .await?;
                let e = &v["error"];
                ensure!(
                    e["code"] == code
                        && e["stage"] == "post_creation"
                        && e["rejectionReasons"] == reasons
                        && e["rejectionReasonSource"] == source
                        && e["xErrorCode"] == x_code
                        && e["xHttpStatus"] == x_status
                        && (e["xRejected"] == true) == x_rejected
                        && e.get("rejectionTextRecorded").is_none(),
                    "Unexpected code classification: {scenario}: {v}"
                );
                ensure!(h.count() == before + 1, "Post was retried: {scenario}");
                ensure!(
                    !v.to_string().contains("999001"),
                    "Unknown provider code appeared in the response: {scenario}"
                );
                if scenario == "rejection-code226" {
                    ensure!(
                        e["nextStep"]
                            .as_str()
                            .is_some_and(|s| s.contains("retryAfter")),
                        "nextStep misses retryAfter: {v}"
                    );
                }
                no_canary(&v.to_string())?;
            }
            // 記録が無効なら、本文を読まず diagnostics フォルダーも作らない。
            ensure!(
                !h.diagnostics_dir().exists(),
                "diagnostics created by default"
            );
            // 記録が有効なら、コードだけの応答は文面なしで、HTTPエラーは伏字済みの本文で1件ずつ記録される。
            for (scenario, code, status, codes, with_text) in [
                (
                    "rejection-code226",
                    "API_REJECTED",
                    200,
                    json!([226]),
                    false,
                ),
                ("rejection-http502", "POST_UNCERTAIN", 502, json!([]), true),
                ("rejection-http400", "API_HTTP_ERROR", 400, json!([]), true),
            ] {
                h.rejection_log_reset().await?;
                h.rejection_log_enable(1).await?;
                h.state.scenario(scenario);
                let v = h
                    .fail(
                        "post_tweet",
                        json!({"account":"posting","text":"provider code record"}),
                        false,
                    )
                    .await?;
                h.state.scenario("normal");
                check_response(&v, true)?;
                let (raw, records) = h.rejection_log_read().await?;
                ensure!(records.len() == 1, "Expected one record: {scenario}: {raw}");
                let r = &records[0];
                ensure!(
                    r["code"] == code
                        && r["httpStatus"] == status
                        && r["providerCodes"] == codes
                        && r["stage"] == "post_creation",
                    "Unexpected record: {scenario}: {raw}"
                );
                let text = r["texts"][0]["text"].as_str().unwrap_or_default();
                ensure!(
                    with_text == text.contains("<secret>")
                        && with_text != r["texts"].as_array().is_some_and(Vec::is_empty),
                    "Unexpected redacted text: {scenario}: {raw}"
                );
                no_canary(&raw)?;
            }
            h.rejection_log_reset().await?;
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
        "Recorded image refusal and a later post HTTP failure are each recorded once",
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
            // 投稿作成の502も本文を読んで記録され、最終エラーに対応する1件として旗が付く。
            check_response(&v, true)?;
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(
                records.len() == 2
                    && records[0]["stage"] == "image_upload"
                    && records[0]["uploadAttempt"] == 1
                    && records[1]["stage"] == "post_creation"
                    && records[1]["code"] == "POST_UNCERTAIN"
                    && records[1]["httpStatus"] == 502,
                "Expected an image record and a post record: {raw}"
            );
            no_canary(&raw)?;
        }
    );
    case!(
        h,
        "Rejection log redacts the detail field of an HTTP 400 body and keeps no text from a body over 64 KiB",
        {
            let text = "diagnostic leak body sentence";
            let post = json!({"account":"posting","text":text});
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            h.state.scenario("rejection-http400detail");
            let v = h.fail("post_tweet", post.clone(), false).await?;
            h.state.scenario("normal");
            check_response(&v, true)?;
            ensure!(v["error"]["code"] == "API_HTTP_ERROR" && v["error"]["httpStatus"] == 400);
            let (raw, records) = h.rejection_log_read().await?;
            ensure!(records.len() == 1, "Expected one detail record: {raw}");
            ensure!(
                !raw.contains(text) && !raw.contains("leak body sentence"),
                "Post text remained in the record: {raw}"
            );
            ensure!(!raw.contains("Poster"), "User name remained: {raw}");
            no_canary(&raw)?;
            let r = &records[0];
            ensure!(
                r["code"] == "API_HTTP_ERROR"
                    && r["httpStatus"] == 400
                    && r["stage"] == "post_creation"
                    && r["texts"][0]["field"] == "detail",
                "Unexpected detail record: {raw}"
            );
            let saved = r["texts"][0]["text"].as_str();
            ensure!(
                saved.is_some_and(|t| t.contains("<secret>")) || r["textDropped"].is_string(),
                "Detail was not redacted: {raw}"
            );
            // 64KiB を超える本文は読み切らず、JSON として読めないので、文面なしの1件になる。
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            h.state.scenario("rejection-http400big");
            let v = h.fail("post_tweet", post, false).await?;
            h.state.scenario("normal");
            check_response(&v, true)?;
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(records.len() == 1, "Expected one oversized record: {raw}");
            let r = &records[0];
            ensure!(
                r["code"] == "API_HTTP_ERROR"
                    && r["httpStatus"] == 400
                    && r["stage"] == "post_creation"
                    && r["texts"].as_array().is_some_and(Vec::is_empty)
                    && !raw.contains("aaaaaaaaaa"),
                "Oversized body was not recorded without text: {raw}"
            );
        }
    );
    case!(
        h,
        "Slow bodies of non-2xx responses stay inside the time limit and keep the image retry",
        {
            h.rejection_log_reset().await?;
            h.rejection_log_enable(1).await?;
            // 画像の1回目が502で、本文が終わらずに届き続ける。記録が有効でも、本文を読む時間に
            // 上限があるので画像ごとの期限が残り、再試行が1回行われて投稿まで進む。
            let mut c = h
                .spawn(&h.secrets, &h.root, 250, Some("--slow-image-upload"), &[])
                .await?;
            h.state.scenario("upload-stream502");
            let before = h.count();
            let started = std::time::Instant::now();
            let v = h
                .call_on(
                    &c,
                    "post_tweet",
                    json!({"account":"posting","text":"slow body retry","images":[h.png]}),
                    false,
                    false,
                )
                .await;
            let elapsed = started.elapsed();
            h.state.scenario("normal");
            checked_close(&mut c).await?;
            let v = v?;
            let sent = h.trace()[before..].to_vec();
            ensure!(
                path_count(&sent, "upload_media_v2") == 2
                    && path_count(&sent, "create_tweet_v2") == 1,
                "The image was not retried once with a slow error body: {v} {sent:?}"
            );
            ensure!(elapsed < Duration::from_secs(8), "Too slow: {elapsed:?}");
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(
                records.len() == 1
                    && records[0]["stage"] == "image_upload"
                    && records[0]["uploadAttempt"] == 1
                    && records[0]["httpStatus"] == 502
                    && records[0]["texts"].as_array().is_some_and(Vec::is_empty),
                "Expected one text-less record for the first attempt: {raw}"
            );
            no_canary(&raw)?;
            // 投稿作成が502で本文が終わらずに届き続けても、読む時間は1秒で切られ、
            // 全体の時間制限（5秒）より前に、結果不明として文面なしの1件を記録して返る。
            h.rejection_log_enable(1).await?;
            let mut c = h.spawn(&h.secrets, &h.root, 5000, None, &[]).await?;
            h.state.scenario("create502-stream");
            let started = std::time::Instant::now();
            let v = h
                .call_on(
                    &c,
                    "post_tweet",
                    json!({"account":"posting","text":"slow body post"}),
                    true,
                    false,
                )
                .await;
            let elapsed = started.elapsed();
            h.state.scenario("normal");
            checked_close(&mut c).await?;
            let v = v?;
            ensure!(
                v["error"]["code"] == "POST_UNCERTAIN" && v["error"]["uploadAttempts"] == 0,
                "Unexpected slow-body post result: {v}"
            );
            ensure!(
                elapsed < Duration::from_secs(4),
                "The slow error body was read past the cap: {elapsed:?}"
            );
            check_response(&v, true)?;
            let (raw, records) = h.rejection_log_read().await?;
            h.rejection_log_reset().await?;
            ensure!(
                records.len() == 1
                    && records[0]["code"] == "POST_UNCERTAIN"
                    && records[0]["httpStatus"] == 502
                    && records[0]["texts"].as_array().is_some_and(Vec::is_empty),
                "Expected one text-less post record: {raw}"
            );
            no_canary(&raw)?;
        }
    );
    case!(
        h,
        "Image upload is not retried when a numeric code classifies the refusal",
        {
            // 画像ごとの期限が1.5秒の試験用の動作で、再試行できる余地がある状態にする。
            let mut c = h
                .spawn(&h.secrets, &h.root, 250, Some("--slow-image-upload"), &[])
                .await?;
            // (場面, 要求回数, 分類)。数字のコード・x_status で分類が付く拒否は再送しない。
            // 対照として、コードのない文面だけの unknown は従来どおり1回再試行される。
            let cases = [
                ("rejection-code226", 1, json!(["automation_suspected"])),
                ("rejection-xstatus", 1, json!(["rate_limit"])),
                ("rejection-unknown", 2, json!(["unknown"])),
            ];
            let mut outcome = Ok(());
            for (scenario, uploads, reasons) in cases {
                h.state.scenario(scenario);
                let before = h.count();
                let v = h
                    .call_on(
                        &c,
                        "post_tweet",
                        json!({"account":"posting","text":"upload code refusal","images":[h.png]}),
                        true,
                        false,
                    )
                    .await;
                let v = match v {
                    Ok(v) => v,
                    Err(e) => {
                        outcome = Err(e);
                        break;
                    }
                };
                let e = &v["error"];
                let sent = h.trace()[before..].to_vec();
                if !(e["code"] == "API_REJECTED"
                    && e["stage"] == "image_upload"
                    && e["postAttempted"] == false
                    && e["uploadAttempts"] == uploads
                    && e["rejectionReasons"] == reasons
                    && sent.len() == uploads
                    && path_count(&sent, "upload_media_v2") == uploads)
                {
                    outcome = Err(anyhow::anyhow!(
                        "Unexpected upload attempts for {scenario}: {v} {sent:?}"
                    ));
                    break;
                }
            }
            h.state.scenario("normal");
            checked_close(&mut c).await?;
            outcome?;
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
    guard_cases(h).await?;
    h.state.scenario("normal");
    Ok(())
}

// ---- 冷却ガード（全プロセス共通の投稿の一時停止） ----

/// 1分の代わりに1000msとする（5分→5秒、15分→15秒、3時間→180秒）。
const GUARD_MINUTE_MS: &str = "1000";
const GUARD_FORBIDDEN: [&str; 9] = [
    KEY,
    COOKIE,
    PASSWORD,
    SEED,
    PROXY,
    "Poster",
    "127.0.0.1",
    "private-rejection-canary",
    "leak body sentence",
];

fn guard_root(h: &Harness, name: &str) -> PathBuf {
    h.output.join(format!("post-guard-{name}"))
}

/// 冷却を有効にした MCP サーバーを、指定の状態ルートで起動する（1分=1000ms）。
async fn guard_client(h: &Harness, root: &Path, timeout: u64) -> Result<McpClient> {
    guard_client_with(h, &h.secrets, root, timeout, GUARD_MINUTE_MS).await
}

/// 設定ファイルと、1分の代わりにする ms を指定する。冷却が明けるのを待たず、明ける前に複数の
/// 操作（呼び出し・ファイル検査・CLI）を終える前提のケースは、遅い環境でも間に合うよう大きくする。
async fn guard_client_with(
    h: &Harness,
    secrets: &Path,
    root: &Path,
    timeout: u64,
    minute_ms: &str,
) -> Result<McpClient> {
    fs::create_dir_all(root).await?;
    h.spawn(
        secrets,
        root,
        timeout,
        None,
        &[("PLUMIVOX_POST_GUARD_TEST".into(), minute_ms.into())],
    )
    .await
}

/// 冷却が明けるのを待たないケースが使う、1分の代わりにする ms（5分→50秒、15分→150秒）。
const GUARD_LONG_MINUTE_MS: &str = "10000";

fn guard_args(h: &Harness, text: &str) -> Value {
    json!({"account":"posting","text":text,"images":[h.png]})
}

fn path_count(requests: &[Value], suffix: &str) -> usize {
    requests
        .iter()
        .filter(|r| r["path"].as_str().unwrap().ends_with(suffix))
        .count()
}

/// 冷却の応答を確かめ、`retryAfter` を返す。
fn check_cooldown(v: &Value, reason: &str, max_seconds: u64) -> Result<String> {
    let e = &v["error"];
    ensure!(
        e["code"] == "POST_COOLDOWN"
            && e["stage"] == "post_guard"
            && e["postAttempted"] == false
            && e["uploadAttempts"] == 0
            && e["cooldownReason"] == reason
            && e["nextStep"].is_string(),
        "Unexpected cooldown response ({reason}): {v}"
    );
    check_retry(e, max_seconds)?;
    let raw = v.to_string();
    no_canary(&raw)?;
    ensure!(!raw.contains("Poster"), "User name in cooldown response");
    Ok(e["retryAfter"].as_str().unwrap().to_owned())
}

/// `retryAfter`（RFC3339 の UTC）と `retryAfterSeconds`（1以上）を確かめる。
fn check_retry(e: &Value, max_seconds: u64) -> Result<()> {
    let seconds = e["retryAfterSeconds"].as_u64().unwrap_or(0);
    let at = e["retryAfter"].as_str().unwrap_or_default();
    ensure!(
        (1..=max_seconds).contains(&seconds)
            && at.ends_with('Z')
            && chrono::DateTime::parse_from_rfc3339(at).is_ok(),
        "Invalid retry information: {e}"
    );
    Ok(())
}

/// `retryAfter` を過ぎるまで待つ（期限は応答の値。固定の待ちを重ねない）。
async fn wait_until(retry_after: &str) -> Result<()> {
    let at = chrono::DateTime::parse_from_rfc3339(retry_after)?.with_timezone(&chrono::Utc);
    let wait = (at - chrono::Utc::now()).to_std().unwrap_or_default();
    ensure!(
        wait < Duration::from_secs(30),
        "Cooldown is too long: {wait:?}"
    );
    tokio::time::sleep(wait + Duration::from_millis(150)).await;
    Ok(())
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// 状態ルートに作られたファイルが `post-guard/` の中だけで、秘密・本文・提供元の文面を含まないことを確かめる。
/// 状態ファイル（`.json`）の中身を返す。
fn check_guard_files(root: &Path, texts: &[&str]) -> Result<Vec<String>> {
    let mut files = vec![];
    collect_files(root, &mut files)?;
    let guard = root.join("post-guard");
    let mut states = vec![];
    for file in &files {
        ensure!(
            file.starts_with(&guard),
            "File outside post-guard: {}",
            file.display()
        );
        let text = String::from_utf8_lossy(&std::fs::read(file)?).into_owned();
        for forbidden in GUARD_FORBIDDEN.iter().chain(texts) {
            ensure!(
                !text.contains(forbidden),
                "Guard file contains forbidden text: {}",
                file.display()
            );
        }
        if file.extension().is_some_and(|e| e == "json") {
            states.push(text);
        }
    }
    ensure!(!states.is_empty(), "No guard state file");
    Ok(states)
}

async fn guard_cases(h: &mut Harness) -> Result<()> {
    case!(
        h,
        "Post cooldown is shared by MCP processes and ends on schedule",
        {
            let root = guard_root(h, "shared");
            let mut a = guard_client(h, &root, 5000).await?;
            let mut b = guard_client(h, &root, 5000).await?;
            h.state.scenario("normal");
            let start = h.count();
            h.call_on(
                &a,
                "post_tweet",
                guard_args(h, "guard shared one"),
                false,
                false,
            )
            .await?;
            let sent = h.trace()[start..].to_vec();
            ensure!(path_count(&sent, "upload_media_v2") == 1);
            ensure!(path_count(&sent, "create_tweet_v2") == 1);
            // 別プロセスの投稿は、画像も投稿も送らずに冷却で止まる。
            let v = h
                .call_on(
                    &b,
                    "post_tweet",
                    guard_args(h, "guard shared two"),
                    true,
                    true,
                )
                .await?;
            let retry_after = check_cooldown(&v, "min_interval", 5)?;
            ensure!(h.count() == start + sent.len(), "Cooldown sent a request");
            check_guard_files(&root, &["guard shared"])?;
            // 冷却が明けたら、止められた側からも送れる。
            wait_until(&retry_after).await?;
            h.call_on(
                &b,
                "post_tweet",
                guard_args(h, "guard shared three"),
                false,
                false,
            )
            .await?;
            let sent = h.trace()[start..].to_vec();
            ensure!(path_count(&sent, "upload_media_v2") == 2);
            ensure!(path_count(&sent, "create_tweet_v2") == 2);
            checked_close(&mut a).await?;
            checked_close(&mut b).await?;
        }
    );
    case!(
        h,
        "Post from another process during a send is stopped as in progress",
        {
            let root = guard_root(h, "busy");
            let mut a = guard_client(h, &root, 15000).await?;
            let mut b = guard_client(h, &root, 5000).await?;
            h.state.scenario("upload-hold");
            h.state.hold_uploads(true);
            let start = h.count();
            let held = h.state.held_count();
            let pending = a
                .start_request(
                    "tools/call",
                    json!({"name":"post_tweet","arguments":guard_args(h, "guard busy one")}),
                )
                .await?;
            h.state.wait_held(held).await?;
            let sending = h.count();
            let v = h
                .call_on(
                    &b,
                    "post_tweet",
                    guard_args(h, "guard busy two"),
                    true,
                    true,
                )
                .await?;
            check_cooldown(&v, "post_in_progress", 1)?;
            ensure!(h.count() == sending, "Busy post sent a request");
            h.state.hold_uploads(false);
            let done = pending.wait_timeout(Duration::from_secs(30)).await?;
            ensure!(done["isError"] != true, "Held post failed: {done}");
            let sent = h.trace()[start..].to_vec();
            ensure!(path_count(&sent, "upload_media_v2") == 1);
            ensure!(path_count(&sent, "create_tweet_v2") == 1);
            // 送信が終わると排他は放され、送信後の間隔の冷却に変わる。
            let v = h
                .call_on(
                    &b,
                    "post_tweet",
                    guard_args(h, "guard busy three"),
                    true,
                    true,
                )
                .await?;
            check_cooldown(&v, "min_interval", 5)?;
            check_guard_files(&root, &["guard busy"])?;
            h.state.scenario("normal");
            checked_close(&mut a).await?;
            checked_close(&mut b).await?;
        }
    );
    case!(
        h,
        "Process killed during a send leaves an interrupted cooldown",
        {
            let root = guard_root(h, "killed");
            let mut a = guard_client(h, &root, 15000).await?;
            h.state.scenario("upload-hold");
            h.state.hold_uploads(true);
            let held = h.state.held_count();
            let pending = a
                .start_request(
                    "tools/call",
                    json!({"name":"post_tweet","arguments":guard_args(h, "guard killed one")}),
                )
                .await?;
            h.state.wait_held(held).await?;
            a.kill().await?;
            drop(pending);
            let _ = a.close().await;
            let states = check_guard_files(&root, &["guard killed"])?;
            ensure!(
                states.iter().any(|s| s.contains("\"inFlight\":{")),
                "inFlight was not left behind: {states:?}"
            );
            h.state.scenario("normal");
            let mut b = guard_client(h, &root, 5000).await?;
            let v = h
                .call_on(
                    &b,
                    "post_tweet",
                    guard_args(h, "guard killed two"),
                    true,
                    true,
                )
                .await?;
            check_cooldown(&v, "interrupted", 15)?;
            check_guard_files(&root, &["guard killed"])?;
            checked_close(&mut b).await?;
        }
    );
    case!(
        h,
        "Rejected post starts a cooldown and the next post is not sent",
        {
            let root = guard_root(h, "rejected");
            let mut a = guard_client(h, &root, 5000).await?;
            h.state.scenario("rejection-code226");
            let start = h.count();
            // 画像なしの投稿で、投稿作成の段階の拒否にする。
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    json!({"account":"posting","text":"guard rejected one"}),
                    true,
                    false,
                )
                .await?;
            let e = &v["error"];
            ensure!(
                e["code"] == "API_REJECTED"
                    && e["stage"] == "post_creation"
                    && e["cooldownReason"] == "automation_suspected",
                "Unexpected rejection: {v}"
            );
            check_retry(e, 180)?;
            ensure!(e["retryAfterSeconds"].as_u64().unwrap() >= 170);
            let sent = h.trace()[start..].to_vec();
            ensure!(path_count(&sent, "upload_media_v2") == 0);
            ensure!(path_count(&sent, "create_tweet_v2") == 1);
            h.state.scenario("normal");
            let sent = h.count();
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    guard_args(h, "guard rejected two"),
                    true,
                    true,
                )
                .await?;
            let retry_after = check_cooldown(&v, "automation_suspected", 180)?;
            ensure!(retry_after == e["retryAfter"], "Retry time changed");
            ensure!(h.count() == sent, "Cooldown sent a request");
            check_guard_files(&root, &["guard rejected"])?;
            checked_close(&mut a).await?;
        }
    );
    case!(
        h,
        "Guard state and CLI status never show secrets or provider text",
        {
            let root = guard_root(h, "files");
            // 2回目の呼び出し・ファイル検査・終了・CLI の `status` を、冷却が明ける前に終える必要があるので、
            // 上書き値を大きくする（15分が150秒）。
            let mut a = guard_client_with(h, &h.secrets, &root, 5000, GUARD_LONG_MINUTE_MS).await?;
            // 提供元の文面（と混ぜた秘密）を持つ拒否。投稿IDが取れない拒否は、結果不明として15分相当の冷却になる。
            h.state.scenario("rejection-noid");
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    guard_args(h, "guard files one"),
                    true,
                    false,
                )
                .await?;
            let reason = "previous_uncertain";
            ensure!(
                v["error"]["cooldownReason"] == reason,
                "Unexpected cooldown after rejection: {v}"
            );
            check_retry(&v["error"], 150)?;
            no_canary(&v.to_string())?;
            h.state.scenario("normal");
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    guard_args(h, "guard files two"),
                    true,
                    true,
                )
                .await?;
            check_cooldown(&v, reason, 150)?;
            let texts = ["guard files", "Could not extract tweet_id"];
            check_guard_files(&root, &texts)?;
            checked_close(&mut a).await?;
            // CLI は隔離した状態ルートの冷却を表示する。解除は端末が要るので、端末でなければ2で終わる。
            let (code, out, err) = guard_cli(&root, &["status"]).await?;
            ensure!(code == 0, "status failed: {code}: {err}");
            ensure!(
                out.contains("cooling down") && out.contains(reason),
                "status does not show the cooldown: {out}"
            );
            let (code, out_clear, err) = guard_cli(&root, &["clear"]).await?;
            ensure!(code == 2, "clear without a terminal: {code}");
            for text in [&out, &out_clear, &err] {
                no_canary(text)?;
                ensure!(!text.contains("Poster") && !text.contains("Could not extract"));
            }
            check_guard_files(&root, &texts)?;
        }
    );
    case!(
        h,
        "HTTP 429 without an X code gives a fixed rate limit cooldown and no strike",
        {
            // 15分が150秒。X の拒否と分かる code:88 は strike が1つ付く（対照）。
            for (name, scenario, code, strikes) in [
                ("limit429", "rejection-http429", "API_HTTP_ERROR", 0),
                ("limit88", "rejection-code88", "API_REJECTED", 1),
            ] {
                let root = guard_root(h, name);
                let mut a =
                    guard_client_with(h, &h.secrets, &root, 5000, GUARD_LONG_MINUTE_MS).await?;
                h.state.scenario(scenario);
                let v = h
                    .call_on(
                        &a,
                        "post_tweet",
                        json!({"account":"posting","text":"guard limit one"}),
                        true,
                        false,
                    )
                    .await?;
                h.state.scenario("normal");
                let e = &v["error"];
                ensure!(
                    e["code"] == code
                        && e["stage"] == "post_creation"
                        && e["postAttempted"] == true
                        && e["cooldownReason"] == "rate_limit",
                    "Unexpected rate limit response ({scenario}): {v}"
                );
                check_retry(e, 150)?;
                ensure!(
                    e["retryAfterSeconds"].as_u64().unwrap_or(0) >= 130,
                    "Rate limit cooldown is shorter than 15 minutes: {v}"
                );
                no_canary(&v.to_string())?;
                checked_close(&mut a).await?;
                let states = check_guard_files(&root, &["guard limit"])?;
                let state: Value = serde_json::from_str(&states[0])?;
                ensure!(
                    state["strikes"] == strikes && state["cooldown"]["reason"] == "rate_limit",
                    "Unexpected strikes after {scenario}: {state}"
                );
            }
        }
    );
    case!(
        h,
        "Proxy-only rejection at post creation starts the minimum interval",
        {
            let root = guard_root(h, "proxy");
            let mut a = guard_client_with(h, &h.secrets, &root, 5000, GUARD_LONG_MINUTE_MS).await?;
            // 画像の段階の proxy 拒否は、投稿の作成に至らないので冷却をかけない（対照）。
            h.state.scenario("rejection-proxy");
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    guard_args(h, "guard proxy one"),
                    true,
                    false,
                )
                .await?;
            ensure!(
                v["error"]["stage"] == "image_upload" && v["error"]["cooldownReason"].is_null(),
                "Image-stage proxy rejection started a cooldown: {v}"
            );
            // 投稿作成の段階の proxy 拒否は、中継が X へ送った後に切れた可能性があるので5分相当の間隔を置く。
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    json!({"account":"posting","text":"guard proxy two"}),
                    true,
                    false,
                )
                .await?;
            h.state.scenario("normal");
            let e = &v["error"];
            ensure!(
                e["code"] == "API_REJECTED"
                    && e["stage"] == "post_creation"
                    && e["postAttempted"] == true
                    && e["rejectionReasons"] == json!(["proxy"])
                    && e["cooldownReason"] == "min_interval",
                "Unexpected proxy rejection response: {v}"
            );
            check_retry(e, 50)?;
            let sent = h.count();
            let v = h
                .call_on(
                    &a,
                    "post_tweet",
                    guard_args(h, "guard proxy three"),
                    true,
                    true,
                )
                .await?;
            check_cooldown(&v, "min_interval", 50)?;
            ensure!(h.count() == sent, "Cooldown sent a request");
            check_guard_files(&root, &["guard proxy"])?;
            checked_close(&mut a).await?;
        }
    );
    case!(
        h,
        "Guard state and status hide the user name when the account name only contains it",
        {
            // 設定名 `poster_main` は、X のユーザー名 `Poster` と一致はしないが含む。
            let secrets = h.output.join("guard-label-secrets.json");
            fs::write(
                &secrets,
                serde_json::to_vec(&json!({"twitterapi_io":{"poster_main":{
                    "user_name":"Poster","api_key":KEY,"proxy":PROXY,"login_cookies":COOKIE
                }}}))?,
            )
            .await?;
            let root = guard_root(h, "label");
            let mut a = guard_client_with(h, &secrets, &root, 5000, GUARD_LONG_MINUTE_MS).await?;
            h.state.scenario("normal");
            h.call_on(
                &a,
                "post_tweet",
                json!({"account":"poster_main","text":"guard label one"}),
                false,
                false,
            )
            .await?;
            checked_close(&mut a).await?;
            let states = check_guard_files(&root, &["guard label", "poster", "Poster"])?;
            let state: Value = serde_json::from_str(&states[0])?;
            ensure!(state["account"] == "<user>", "Unexpected label: {state}");
            let (code, out, err) = guard_cli(&root, &["status"]).await?;
            ensure!(code == 0, "status failed: {code}: {err}");
            // 状態ルートのパスは、利用者の環境の文字列なので、比べる前に除く。
            let shown = out.replace(&root.display().to_string(), "");
            ensure!(
                shown.contains("<user>") && !shown.to_lowercase().contains("poster"),
                "status shows the user name: {out}"
            );
            ensure!(!err.to_lowercase().contains("poster"));
            let _ = fs::remove_file(&secrets).await;
        }
    );
    case!(
        h,
        "Guard state written by a newer schema refuses the post and stays unchanged",
        {
            let root = guard_root(h, "newer");
            let mut a = guard_client_with(h, &h.secrets, &root, 5000, GUARD_LONG_MINUTE_MS).await?;
            h.state.scenario("normal");
            h.call_on(
                &a,
                "post_tweet",
                guard_args(h, "guard newer one"),
                false,
                false,
            )
            .await?;
            checked_close(&mut a).await?;
            // 状態ファイルを、いまの版より大きい schema に書き換える。
            let mut files = vec![];
            collect_files(&root.join("post-guard"), &mut files)?;
            let state_file = files
                .iter()
                .find(|file| file.extension().is_some_and(|e| e == "json"))
                .context("No guard state file")?
                .clone();
            let mut value: Value = serde_json::from_slice(&std::fs::read(&state_file)?)?;
            value["schema"] = json!(999);
            let newer = serde_json::to_vec(&value)?;
            std::fs::write(&state_file, &newer)?;
            let mut b = guard_client_with(h, &h.secrets, &root, 5000, GUARD_LONG_MINUTE_MS).await?;
            let sent = h.count();
            let v = h
                .call_on(
                    &b,
                    "post_tweet",
                    guard_args(h, "guard newer two"),
                    true,
                    true,
                )
                .await?;
            check_cooldown(&v, "guard_state_reset", 150)?;
            ensure!(h.count() == sent, "Refused post sent a request");
            checked_close(&mut b).await?;
            ensure!(
                std::fs::read(&state_file)? == newer,
                "The newer state file was changed"
            );
            let mut after = vec![];
            collect_files(&root.join("post-guard"), &mut after)?;
            ensure!(
                after
                    .iter()
                    .all(|file| file.extension().is_none_or(|e| e != "tmp")),
                "A temporary file was left behind: {after:?}"
            );
            check_guard_files(&root, &["guard newer"])?;
        }
    );
    Ok(())
}

/// `plumivox --post-guard <args>` を、隔離した状態ルートで端末なしに実行する。
async fn guard_cli(root: &Path, args: &[&str]) -> Result<(i32, String, String)> {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_plumivox"));
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = command
        .arg("--post-guard")
        .args(args)
        .env("PLUMIVOX_STATE_DIR", root)
        .env("PLUMIVOX_LANGUAGE", "en")
        .env_remove("PLUMIVOX_POST_GUARD_TEST")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("Cannot run plumivox --post-guard")?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}
