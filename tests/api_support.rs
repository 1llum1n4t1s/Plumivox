use crate::support::{self, HttpRequest, HttpResponse, HttpServer, McpClient, Result, Suite};
use anyhow::{Context, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use hmac::{Hmac, KeyInit, Mac};
use http_body_util::BodyExt;
use hyper::{
    Response,
    body::{Body, Bytes, Frame},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::{
    fs,
    sync::Notify,
    time::{Instant, Sleep},
};

pub const KEY: &str = "synthetic-api-secret-never-display";
pub const COOKIE: &str = "synthetic-cookie-secret-never-display";
pub const PASSWORD: &str = "synthetic-password-never-display";
pub const SEED: &str = "JBSWY3DPEHPK3PXP";
pub const PROXY: &str = "http://127.0.0.1:8888";
pub fn sha(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}
pub fn ids(v: &Value) -> Vec<String> {
    v["tweets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_owned())
        .collect()
}
pub fn merge(mut a: Value, b: Value) -> Value {
    for (k, v) in b.as_object().unwrap() {
        a[k] = v.clone();
    }
    a
}
pub fn png() -> Vec<u8> {
    STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=").unwrap()
}
pub fn session_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "session.keychain"
    } else {
        "session.dpapi"
    }
}
pub fn session_source() -> &'static str {
    if cfg!(target_os = "macos") {
        "plumivox_keychain"
    } else {
        "plumivox_dpapi"
    }
}
pub fn tweet(id: &str, time: Value) -> Value {
    json!({"id":id,"text":format!("Tweet {id}"),"createdAt":time,"author":{"userName":"PublicUser","name":"Public User"},"url":format!("https://x.com/PublicUser/status/{id}")})
}
pub fn normal_tweet(id: &str) -> Value {
    tweet(id, json!("2026-10-12T12:00:00Z"))
}
pub fn signed(prefix: &str, data: &Value) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(data).unwrap());
    let mut mac = Hmac::<Sha256>::new_from_slice(KEY.as_bytes()).unwrap();
    mac.update(payload.as_bytes());
    format!(
        "{prefix}{payload}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}
pub fn decode(cursor: &str) -> Value {
    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(cursor.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap()
}
pub fn search_args() -> Value {
    json!({"account":"reading","keyword":"needle phrase","startTime":"2026-10-12T20:00:00+09:00","endTime":"2026-10-13T00:00:00+09:00"})
}
pub fn feed_data() -> Value {
    json!({"version":2,"account":"reading","userName":"reader","batches":[{"users":["public0"],"cursor":"search-next"}],"coverage":{"accounts":1,"followingHasNextPage":false,"followingNextCursor":null,"protectedAccountsExcluded":0,"protectionStatusUnknownAccounts":0,"followingPage":"first","followingContinuationAvailable":false,"followingContinuationUnavailableReason":null}})
}

#[derive(Default)]
pub struct BodyStats {
    pub sent: AtomicUsize,
    pub dropped: AtomicBool,
    pub complete: AtomicBool,
}
struct TimedBody {
    chunks: Vec<Bytes>,
    index: usize,
    repeat: bool,
    delay: Duration,
    sleep: Pin<Box<Sleep>>,
    stats: Arc<BodyStats>,
}
impl Body for TimedBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<io::Result<Frame<Bytes>>>> {
        let this = self.get_mut();
        if !this.repeat && this.index >= this.chunks.len() {
            this.stats.complete.store(true, Ordering::SeqCst);
            return Poll::Ready(None);
        }
        if this.sleep.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        let chunk = this.chunks[this.index % this.chunks.len()].clone();
        this.index += 1;
        this.stats.sent.fetch_add(chunk.len(), Ordering::SeqCst);
        this.sleep.as_mut().reset(Instant::now() + this.delay);
        Poll::Ready(Some(Ok(Frame::data(chunk))))
    }
}
impl Drop for TimedBody {
    fn drop(&mut self) {
        self.stats.dropped.store(true, Ordering::SeqCst);
    }
}
fn stream(
    status: u16,
    chunks: Vec<Bytes>,
    delay: u64,
    repeat: bool,
    stats: Arc<BodyStats>,
) -> HttpResponse {
    Response::builder()
        .status(status)
        .body(
            TimedBody {
                chunks,
                index: 0,
                repeat,
                delay: Duration::from_millis(delay),
                sleep: Box::pin(tokio::time::sleep(Duration::ZERO)),
                stats,
            }
            .boxed(),
        )
        .unwrap()
}
#[derive(Default)]
pub struct Mock {
    pub scenario: String,
    pub trace: Vec<Value>,
    pub uploads: usize,
    pub next_post: u64,
    pub active_logins: usize,
    pub peak_logins: usize,
    pub body_stats: Arc<BodyStats>,
    pub login_held: bool,
}
pub struct State {
    pub mock: Mutex<Mock>,
    pub accepted: Notify,
    pub release: Notify,
}
struct ActiveLogin(Arc<State>);
impl Drop for ActiveLogin {
    fn drop(&mut self) {
        self.0.mock.lock().unwrap().active_logins -= 1;
    }
}
impl State {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            mock: Mutex::new(Mock {
                scenario: "normal".into(),
                next_post: 900,
                ..Default::default()
            }),
            accepted: Notify::new(),
            release: Notify::new(),
        })
    }
    pub fn scenario(&self, name: &str) {
        let mut m = self.mock.lock().unwrap();
        m.scenario = name.into();
        m.uploads = 0;
        m.login_held = false;
    }
    pub fn trace(&self) -> Vec<Value> {
        self.mock.lock().unwrap().trace.clone()
    }
    pub fn count(&self) -> usize {
        self.mock.lock().unwrap().trace.len()
    }
    pub fn stats(&self) -> Arc<BodyStats> {
        self.mock.lock().unwrap().body_stats.clone()
    }
    pub async fn accepted(&self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(5), self.accepted.notified())
            .await
            .context("Mock did not accept request")?;
        Ok(())
    }
}
fn contains(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|v| v == part)
}
fn file_bytes(request: &HttpRequest) -> Option<&[u8]> {
    let start = request
        .body
        .windows(11)
        .position(|w| w == b"name=\"file\"")?;
    let offset = request.body[start..]
        .windows(4)
        .position(|w| w == b"\r\n\r\n")?
        + start
        + 4;
    let boundary = request
        .headers
        .get("content-type")?
        .to_str()
        .ok()?
        .split("boundary=")
        .nth(1)?
        .trim_matches('"');
    let suffix = format!("\r\n--{boundary}");
    let end = request.body[offset..]
        .windows(suffix.len())
        .position(|w| w == suffix.as_bytes())?
        + offset;
    Some(&request.body[offset..end])
}
pub async fn handle(state: Arc<State>, req: HttpRequest) -> io::Result<HttpResponse> {
    let query: HashMap<String, String> = url::form_urlencoded::parse(req.query.as_bytes())
        .into_owned()
        .collect();
    let payload: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    let scenario = state.mock.lock().unwrap().scenario.clone();
    let mut trace = json!({"method":req.method,"path":req.path,"query":query,"authenticated":req.headers.get("x-api-key").is_some_and(|v|v==KEY),"cookieHeaderPresent":req.headers.contains_key("cookie"),"contentType":req.headers.get("content-type").and_then(|v|v.to_str().ok()),"bodyBytes":req.body.len()});
    if req.path.ends_with("/create_tweet_v2") {
        trace["postingPayloadValid"] = json!(
            payload["login_cookies"] == COOKIE
                && payload["proxy"] == PROXY
                && payload["tweet_text"].is_string()
        );
        trace["mediaIds"] = payload.get("media_ids").cloned().unwrap_or(json!([]));
    }
    if req.path.ends_with("/upload_media_v2") {
        trace["multipartPayloadValid"] = json!(
            contains(&req.body, COOKIE.as_bytes())
                && contains(&req.body, b"name=\"proxy\"")
                && contains(&req.body, b"name=\"file\"")
                && contains(&req.body, &png())
        );
        if ["image-order", "image-later-failure", "upload-retry-order"].contains(&scenario.as_str())
        {
            trace["uploadedImageHash"] = json!(sha(
                file_bytes(&req).ok_or_else(|| io::Error::other("Missing multipart file"))?
            ));
        }
    }
    if req.path.ends_with("/user_login_v2") {
        trace["loginPayloadValid"] = json!(
            payload["user_name"] == "AuthUser"
                && payload["email"] == "auth@example.invalid"
                && payload["password"] == PASSWORD
                && payload["proxy"] == PROXY
        );
        trace["totpSeedExact"] = json!(payload["totp_secret"] == SEED);
        let mut keys = payload
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        keys.sort();
        trace["loginPayloadKeys"] = json!(keys);
    }
    state.mock.lock().unwrap().trace.push(trace);
    let send = |v| Ok(support::json_response(200, v));
    if scenario == "create502" && req.path.ends_with("/create_tweet_v2") {
        return Ok(support::json_response(502, json!({"message":COOKIE})));
    }
    if scenario.starts_with("upload-") && req.path.ends_with("/upload_media_v2") {
        let attempt = {
            let mut m = state.mock.lock().unwrap();
            m.uploads += 1;
            m.uploads
        };
        match scenario.as_str() {
            "upload-stream502" => {
                if attempt > 1 {
                    return send(json!({"status":"success","media_id":"700"}));
                }
                let stats = Arc::new(BodyStats::default());
                state.mock.lock().unwrap().body_stats = stats.clone();
                return Ok(stream(
                    502,
                    vec![Bytes::from_static(COOKIE.as_bytes())],
                    20,
                    true,
                    stats,
                ));
            }
            "upload-retry-time-budget" => {
                tokio::time::sleep(Duration::from_millis(if attempt == 1 { 100 } else { 900 }))
                    .await;
                return Ok(support::json_response(
                    if attempt == 1 { 502 } else { 200 },
                    if attempt == 1 {
                        json!({"message":COOKIE})
                    } else {
                        json!({"status":"success","media_id":"700"})
                    },
                ));
            }
            "upload-retry-order" => {
                return Ok(support::json_response(
                    if attempt == 2 { 502 } else { 200 },
                    if attempt == 2 {
                        json!({"message":COOKIE})
                    } else {
                        json!({"status":"success","media_id":(700+attempt).to_string()})
                    },
                ));
            }
            "upload-jsonerror" => return send(json!({"status":"error","message":COOKIE})),
            "upload-html" => return Ok(support::raw_response(200, COOKIE.as_bytes().to_vec())),
            "upload-missing-id" => return send(json!({"status":"success"})),
            "upload-timeout" | "upload-slow" => {
                tokio::time::sleep(Duration::from_millis(if scenario == "upload-slow" {
                    900
                } else {
                    1800
                }))
                .await
            }
            "upload-network" => return Err(io::Error::other("Synthetic TCP interruption")),
            "upload-cancel-backoff" => {
                state.accepted.notify_one();
                return Ok(support::json_response(502, json!({"message":COOKIE})));
            }
            "upload-cancel-second" => {
                if attempt == 1 {
                    return Ok(support::json_response(502, json!({"message":COOKIE})));
                }
                state.accepted.notify_one();
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            _ => {
                let parts = scenario.split('-').collect::<Vec<_>>();
                if parts.len() == 3 && ["recover", "always"].contains(&parts[1]) {
                    let code = if parts[1] == "recover" && attempt > 1 {
                        200
                    } else {
                        parts[2].parse().unwrap()
                    };
                    return Ok(support::json_response(
                        code,
                        if code == 200 {
                            json!({"status":"success","media_id":"700"})
                        } else {
                            json!({"message":COOKIE})
                        },
                    ));
                }
            }
        }
    }
    if scenario == "oversized-response" {
        let stats = Arc::new(BodyStats::default());
        state.mock.lock().unwrap().body_stats = stats.clone();
        return Ok(stream(
            200,
            vec![Bytes::from(vec![b' '; 1024 * 1024]); 32],
            4,
            false,
            stats,
        ));
    }
    if scenario == "split-utf8" {
        let bytes = serde_json::to_vec(
            &json!({"tweets":[merge(normal_tweet("506"),json!({"text":"🫱🏻‍🫲🏽 日本語"}))]}),
        )
        .unwrap();
        let i = bytes.windows(4).position(|w| w == "🫱".as_bytes()).unwrap() + 2;
        return Ok(stream(
            200,
            vec![
                Bytes::copy_from_slice(&bytes[..i]),
                Bytes::copy_from_slice(&bytes[i..]),
            ],
            10,
            false,
            Arc::new(BodyStats::default()),
        ));
    }
    if let Some(code) = scenario
        .strip_prefix("http")
        .and_then(|v| v.parse::<u16>().ok())
    {
        return Ok(support::json_response(code, json!({"message":KEY})));
    }
    if scenario == "html" {
        return Ok(support::raw_response(
            200,
            format!("<html>{COOKIE}</html>").into_bytes(),
        ));
    }
    if scenario == "jsonerror" {
        return send(json!({"status":"error","msg":COOKIE}));
    }
    if scenario == "timeout" {
        tokio::time::sleep(Duration::from_millis(700)).await;
        return send(json!({"tweet_id":"999"}));
    }
    if req.path.ends_with("/user_login_v2") {
        match scenario.as_str() {
            "login-cookie-newline" => {
                return send(json!({"status":"success","login_cookie":format!("{COOKIE}\n")}));
            }
            "login-cookie-nul" => {
                return send(json!({"status":"success","login_cookie":format!("{COOKIE}\0")}));
            }
            "login-cookie-utf8-over" => {
                return send(json!({"status":"success","login_cookie":"あ".repeat(400000)}));
            }
            "interprocess-login" => {
                let hold = {
                    let mut m = state.mock.lock().unwrap();
                    m.active_logins += 1;
                    m.peak_logins = m.peak_logins.max(m.active_logins);
                    let hold = !m.login_held;
                    m.login_held = true;
                    hold
                };
                let _active = ActiveLogin(state.clone());
                if hold {
                    state.accepted.notify_one();
                    state.release.notified().await;
                }
            }
            "login-delay" => {
                state.accepted.notify_one();
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            _ => {}
        }
        return send(match scenario.as_str() {
            "login-missing-cookie" => json!({"status":"success"}),
            "login-plural-cookie" => json!({"status":"success","login_cookies":COOKIE}),
            _ => json!({"status":"success","login_cookie":COOKIE}),
        });
    }
    if req.path.ends_with("/user/info") {
        return send(json!({"data":{"id":"101","userName":"Reader"}}));
    }
    if req.path.ends_with("/user/tweet_timeline") {
        return send(match scenario.as_str() {
            "cursor-timeline-over" => {
                json!({"tweets":[normal_tweet("1200")],"has_next_page":true,"next_cursor":"c".repeat(100001)})
            }
            "timeline-nested" => {
                let next = query
                    .get("cursor")
                    .is_some_and(|s| s == "nested-timeline-next");
                json!({"status":"success","code":0,"msg":"success","data":{"tweets":[normal_tweet(if next{"505"}else{"504"})]},"has_next_page":!next,"next_cursor":if next{""}else{"nested-timeline-next"}})
            }
            "timeline-nested-invalid" => json!({"data":{"tweets":{}},"has_next_page":false}),
            "timeline-nested-missing" => json!({"data":{},"has_next_page":false}),
            "timeline-nested-array" => json!({"data":[normal_tweet("504")],"has_next_page":false}),
            "timeline-invalid-top" => {
                json!({"tweets":{},"data":{"tweets":[normal_tweet("504")]},"has_next_page":false})
            }
            _ => {
                json!({"tweets":[normal_tweet("501")],"has_next_page":true,"next_cursor":"timeline-next"})
            }
        });
    }
    if req.path.ends_with("/tweets") {
        return send(json!({"tweets":[normal_tweet("502")]}));
    }
    if req.path.ends_with("/user/followings") {
        return send(followings(&scenario, &query));
    }
    if req.path.ends_with("/tweet/advanced_search") {
        if scenario == "search-cancel" {
            state.accepted.notify_one();
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        return send(search_response(&scenario, &query));
    }
    if req.path.ends_with("/upload_media_v2") {
        if ["image-order", "image-later-failure"].contains(&scenario.as_str()) {
            let index = {
                let mut m = state.mock.lock().unwrap();
                let i = m.uploads;
                m.uploads += 1;
                i
            };
            return send(if scenario == "image-later-failure" && index == 2 {
                json!({"status":"success"})
            } else {
                json!({"status":"success","media_id":(700+index).to_string()})
            });
        }
        if scenario == "cancel-upload" {
            state.accepted.notify_one();
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        return send(json!({"status":"success","media_id":"700","media_id_string":"700"}));
    }
    if req.path.ends_with("/create_tweet_v2") {
        let id = {
            let mut m = state.mock.lock().unwrap();
            let id = m.next_post;
            m.next_post += 1;
            id
        };
        return send(if scenario == "missing-id" {
            json!({"status":"success"})
        } else {
            json!({"status":"success","tweet_id":id.to_string()})
        });
    }
    Ok(support::json_response(
        404,
        json!({"status":"error","message":"Unhandled local mock endpoint"}),
    ))
}

fn followings(s: &str, q: &HashMap<String, String>) -> Value {
    let public = |n: usize| {
        (0..n)
            .map(|i| json!({"userName":format!("Public{i}"),"isProtected":false}))
            .collect::<Vec<_>>()
    };
    if s == "following-selection" {
        if q.get("cursor").is_some_and(|v| v == "selection-next") {
            return json!({"followings":[{"userName":"NextUnknown"},{"userName":"NextPublic","protected":false},{"userName":"NextPrivate","protected":true},{"userName":"NEXTUNKNOWN","protected":true},{"userName":"NextOther"}],"has_next_page":false,"next_cursor":""});
        }
        let mut users = vec![
            json!({"userName":"FirstUnknown"}),
            json!({"userName":"FIRSTUNKNOWN"}),
            json!({"userName":"","isProtected":false}),
            json!({"userName":"invalid-name","protected":false}),
            json!({"userName":"Private","isProtected":true,"protected":false}),
            json!({"userName":"OTHERPRIVATE","protected":false}),
            json!({"userName":"OtherPrivate","isProtected":false,"protected":true}),
            json!({"userName":"KnownFirst","protected":false}),
            json!({"userName":"KNOWNFIRST"}),
        ];
        users.extend((0..18).map(|i| json!({"userName":format!("Chosen{i}")})));
        users.extend([
            json!({"userName":"PastLimit","isProtected":false}),
            json!({"userName":"FIRSTUNKNOWN","protected":false}),
            json!({"userName":"Chosen0","isProtected":true,"protected":false}),
            json!({"userName":"CHOSEN0","isProtected":false}),
            json!({"userName":"Chosen1","isProtected":false}),
            json!({"userName":"PRIVATE","isProtected":false}),
            json!({"userName":"private","protected":true}),
            json!({"userName":"otherprivate","isProtected":false}),
            json!({"userName":"invalid-name","protected":true}),
            json!({"userName":"","protected":true}),
        ]);
        return json!({"followings":users,"has_next_page":true,"next_cursor":"selection-next"});
    }
    if s.starts_with("following-duplicates") {
        let users = (0..21).map(|i| json!({"userName":if i % 2 == 0 { "OnlyPublic" } else { "ONLYPUBLIC" },"protected":false})).collect::<Vec<_>>();
        return json!({"followings":users,"has_next_page":false,"next_cursor":if s.ends_with("stale-long"){"f".repeat(10001)}else if s.ends_with("stale"){"stale-cursor".to_owned()}else{String::new()}});
    }
    if s.starts_with("following-account-limit") {
        return json!({"followings":public(21),"has_next_page":s.ends_with("with-cursor"),"next_cursor":if s.ends_with("with-cursor"){"must-not-skip"}else{""}});
    }
    if s == "following-missing-cursor" {
        return json!({"followings":public(1),"has_next_page":true,"next_cursor":""});
    }
    if s == "following-many-protected" {
        let mut users = public(20);
        users.extend((0..201).map(|i| json!({"userName":format!("Private{i}"),"protected":true})));
        users.extend([
            json!({"userName":"PRIVATE0","isProtected":true}),
            json!({"userName":"Private0","isProtected":false}),
            json!({"userName":"invalid-name","protected":true}),
        ]);
        return json!({"followings":users,"has_next_page":true,"next_cursor":"many-protected-next"});
    }
    if s == "unknown-following" {
        return json!({"followings":[{"userName":"KnownPublic","isProtected":false},{"userName":"UnknownPublic"},{"userName":"OtherUnknown"},{"userName":"Private","isProtected":true}],"has_next_page":false,"next_cursor":""});
    }
    if s.starts_with("cursor-aggregate") {
        return json!({"followings":public(160),"has_next_page":false,"next_cursor":""});
    }
    if s == "cursor-wrapper-following" {
        return json!({"followings":public(20),"has_next_page":false,"next_cursor":""});
    }
    if s == "cursor-following-list-over" || s == "cursor-following-wrapper-over" {
        return json!({"followings":public(1),"has_next_page":true,"next_cursor":if s=="cursor-following-list-over"{"f".repeat(10001)}else{"following-next".into()}});
    }
    if q.get("cursor").is_some_and(|v| v == "following-next") {
        return json!({"followings":[{"userName":"LaterPublic","isProtected":false}],"has_next_page":false,"next_cursor":""});
    }
    let has = ["following-page", "search-following-page"].contains(&s);
    let mut users = public(25);
    users.push(json!({"userName":"Private","isProtected":true}));
    json!({"followings":if ["empty","search-empty"].contains(&s){vec![]}else{users},"has_next_page":has,"next_cursor":if has{"following-next"}else{""}})
}
fn search_response(s: &str, q: &HashMap<String, String>) -> Value {
    let query = q.get("query").map(String::as_str).unwrap_or("");
    let has = !q.get("cursor").is_some_and(|v| !v.is_empty());
    if s == "feed-date-order" {
        return json!({"tweets":[tweet("10",json!("1969-12-31T23:59:59Z")),tweet("20",Value::Null),tweet("30",json!("invalid-date")),tweet("40",json!("1970-01-01T00:00:00Z")),normal_tweet("50"),normal_tweet("60"),normal_tweet("070"),normal_tweet("70"),tweet("80",json!("1969-12-31T23:59:58Z"))],"has_next_page":false,"next_cursor":""});
    }
    let size = match s {
        "cursor-provider-over" => 10001,
        "cursor-provider-below" | "cursor-aggregate-over" | "cursor-wrapper-over" => 9500,
        "cursor-aggregate-below" => 8000,
        "cursor-wrapper-following" => 1000,
        _ => 0,
    };
    if size > 0 {
        return json!({"tweets":[normal_tweet("1200")],"has_next_page":has,"next_cursor":if has{"c".repeat(size)}else{String::new()}});
    }
    if s == "search-malformed" {
        return json!({"tweets":{}});
    }
    if s.starts_with("search-") {
        let phrase = regex::Regex::new(r#""([^"]+)""#)
            .unwrap()
            .captures(query)
            .map(|c| c[1].to_owned());
        let operators = regex::Regex::new(r#""[^"]*""#)
            .unwrap()
            .replace_all(query, "");
        let authors = regex::Regex::new(r"from:([A-Za-z0-9_]+)")
            .unwrap()
            .captures_iter(&operators)
            .map(|c| c[1].to_ascii_lowercase())
            .collect::<Vec<_>>();
        let epoch = |key: &str, default: i64| {
            regex::Regex::new(&format!(r"{key}:(-?\d+)"))
                .unwrap()
                .captures(&operators)
                .and_then(|c| c[1].parse::<i64>().ok())
                .unwrap_or(default)
        };
        let since = epoch("since_time", i64::MIN);
        let until = epoch("until_time", i64::MAX);
        let candidates = [
            ("803", "Public0", "2026-10-12T11:00:00Z", "needle phrase"),
            ("801", "Public0", "2026-10-12T13:00:00Z", "needle phrase"),
            ("802", "Public20", "2026-10-12T14:00:00Z", "needle phrase"),
            ("804", "Outside", "2026-10-12T12:00:00Z", "needle phrase"),
            ("805", "Public0", "2026-10-12T10:59:59Z", "needle phrase"),
            ("806", "Public20", "2026-10-12T15:00:00Z", "needle phrase"),
            (
                "807",
                "Public0",
                "2026-10-12T12:00:00Z",
                "different keyword",
            ),
            (
                "808",
                "LaterPublic",
                "2026-10-12T12:30:00Z",
                "needle phrase",
            ),
            (
                "809",
                "Public0",
                "2026-10-12T12:00:00Z",
                "needle OR from:Outside",
            ),
            (
                "810",
                "Outside",
                "2026-10-12T12:00:00Z",
                "needle OR from:Outside",
            ),
        ];
        let mut selected = candidates
            .iter()
            .filter(|(_, author, time, text)| {
                let t = chrono::DateTime::parse_from_rfc3339(time)
                    .unwrap()
                    .timestamp();
                phrase.as_ref().is_some_and(|p| text.contains(p))
                    && (authors.is_empty() || authors.contains(&author.to_ascii_lowercase()))
                    && t >= since
                    && t < until
            })
            .map(|(id, author, time, text)| {
                merge(
                    tweet(id, json!(time)),
                    json!({"text":text,"author":{"userName":author}}),
                )
            })
            .collect::<Vec<_>>();
        if authors.iter().any(|v| v == "public20") {
            if let Some(p) = phrase {
                if "needle phrase".contains(&p) && since <= 1791810000 && 1791810000 < until {
                    let (_, a, t, x) = candidates[1];
                    selected.push(merge(
                        tweet("801", json!(t)),
                        json!({"text":x,"author":{"userName":a}}),
                    ));
                }
            }
        }
        return json!({"tweets":selected,"has_next_page":has,"next_cursor":if has{"search-filter-next"}else{""}});
    }
    if query.to_ascii_lowercase().contains("from:laterpublic") {
        return json!({"tweets":[normal_tweet("604")],"has_next_page":false,"next_cursor":""});
    }
    let tweets = if query.to_ascii_lowercase().contains("from:public20") {
        // 同一IDの後続バッチで本文とネスト値が更新されても、後勝ちを保つ。
        let mut repeated = tweet("601", json!("2026-10-12T13:00:00Z"));
        repeated["text"] = json!("Later batch tweet");
        repeated["author"]["name"] = json!("Later batch author");
        vec![repeated, tweet("602", json!("2026-10-12T14:00:00Z"))]
    } else {
        vec![
            tweet("603", json!("2026-10-12T11:00:00Z")),
            tweet("601", json!("2026-10-12T13:00:00Z")),
        ]
    };
    json!({"tweets":tweets,"has_next_page":has,"next_cursor":if has{"search-next"}else{""}})
}

pub struct Harness {
    pub suite: Suite,
    pub state: Arc<State>,
    pub server: HttpServer,
    pub client: McpClient,
    pub output: PathBuf,
    pub secrets: PathBuf,
    pub root: PathBuf,
    pub config: Value,
    pub initial: Vec<u8>,
    pub png: PathBuf,
    pub invalid: PathBuf,
    pub oversized: PathBuf,
    pub ordered: Vec<PathBuf>,
    pub diagnostics: Vec<String>,
    pub cursor: Option<String>,
    pub search_cursor: Option<String>,
    pub following_cursor: Option<String>,
    pub list_cursor: Option<String>,
    pub case_filter: Option<String>,
    pub selected_count: usize,
}
impl Harness {
    pub async fn new() -> Result<Self> {
        let mut suite = Suite::new("PLUMIVOX_E2E_OUTPUT_DIR", "api-e2e")?;
        for s in [KEY, COOKIE, PASSWORD, SEED, PROXY] {
            suite.add_secret(s.into());
        }
        let output = suite.output().to_path_buf();
        let secrets = output.join("synthetic-secrets.json");
        let root = output.join("auth-state");
        let mut accounts = json!({"posting":{"user_name":"Poster","api_key":KEY,"proxy":PROXY,"login_cookies":COOKIE},"reading":{"user_name":"Reader","api_key":KEY},"broken":{"user_name":"Broken","api_key":"","email":"auth@example.invalid","password":PASSWORD,"proxy":PROXY},"auth":{"user_name":"AuthUser","api_key":KEY,"proxy":PROXY,"email":"auth@example.invalid","password":PASSWORD,"totp_secret":SEED},"noCredentials":{"user_name":"NoCredentials","api_key":KEY,"proxy":PROXY},"badSeed":{"user_name":"BadSeed","api_key":KEY,"proxy":PROXY,"email":"auth@example.invalid","password":PASSWORD,"totp_secret":"123456"}});
        for name in [
            format!("profile-{}", "p".repeat(65)),
            format!("cursor-profile-{}", "p".repeat(150000)),
            format!("feed-cursor-profile-{}", "p".repeat(72000)),
        ] {
            accounts[&name] = json!({"user_name":"Reader","api_key":KEY});
        }
        let config = json!({"twitterapi_io":accounts});
        let initial = serde_json::to_vec(&config)?;
        fs::write(&secrets, &initial).await?;
        let png_path = output.join("one-pixel.png");
        fs::write(&png_path, png()).await?;
        let invalid = output.join("invalid.png");
        fs::write(&invalid, b"not an image").await?;
        let oversized = output.join("oversized.png");
        let mut big = png();
        big.resize(big.len() + 5 * 1024 * 1024, 0);
        fs::write(&oversized, big).await?;
        let mut ordered = vec![];
        for i in 0..4 {
            let p = output.join(format!("ordered-{i}.png"));
            let mut bytes = png();
            bytes.extend(format!("image-order-{i}").as_bytes());
            fs::write(&p, bytes).await?;
            ordered.push(p);
        }
        let corrupt = output
            .join("corrupted-auth-state")
            .join(sha("posting\nposter"));
        fs::create_dir_all(&corrupt).await?;
        fs::write(
            corrupt.join(session_name()),
            b"deliberately invalid synthetic encrypted session",
        )
        .await?;
        let state = State::new();
        let shared = state.clone();
        let server = HttpServer::start(move |r| handle(shared.clone(), r)).await?;
        let args = vec![
            server.base_url.clone(),
            secrets.to_string_lossy().into(),
            root.to_string_lossy().into(),
            "250".into(),
        ];
        let client = McpClient::spawn(&args).await?;
        Ok(Self {
            suite,
            state,
            server,
            client,
            output,
            secrets,
            root,
            config,
            initial,
            png: png_path,
            invalid,
            oversized,
            ordered,
            diagnostics: vec![],
            cursor: None,
            search_cursor: None,
            following_cursor: None,
            list_cursor: None,
            case_filter: std::env::var("PLUMIVOX_E2E_CASE")
                .ok()
                .filter(|value| !value.is_empty()),
            selected_count: 0,
        })
    }
    pub fn selected(&mut self, name: &str) -> bool {
        let yes = self.case_filter.as_ref().is_none_or(|f| f == name);
        if yes {
            self.selected_count += 1;
        }
        yes
    }
    pub async fn spawn(
        &self,
        config: &Path,
        root: &Path,
        timeout: u64,
        mode: Option<&str>,
        env: &[(String, String)],
    ) -> Result<McpClient> {
        let mut args = vec![
            self.server.base_url.clone(),
            config.to_string_lossy().into(),
            root.to_string_lossy().into(),
            timeout.to_string(),
        ];
        if let Some(mode) = mode {
            args.push(mode.into());
        }
        McpClient::spawn_with_env(&args, env).await
    }
    pub async fn restart(&mut self, timeout: u64) -> Result<()> {
        self.client.close().await?;
        self.diagnostics.push(self.client.stderr());
        self.client = self
            .spawn(&self.secrets, &self.root, timeout, None, &[])
            .await?;
        Ok(())
    }
    pub async fn write_config(&self, v: &Value) -> Result<()> {
        fs::write(&self.secrets, serde_json::to_vec(v)?).await?;
        Ok(())
    }
    pub async fn restore(&mut self) -> Result<()> {
        fs::write(&self.secrets, &self.initial).await?;
        self.state.scenario("normal");
        Ok(())
    }
    pub fn trace(&self) -> Vec<Value> {
        self.state.trace()
    }
    pub fn count(&self) -> usize {
        self.state.count()
    }
    pub async fn call_on(
        &self,
        c: &McpClient,
        name: &str,
        args: Value,
        failure: bool,
        no_http: bool,
    ) -> Result<Value> {
        let before = self.count();
        let data = match c.call_tool(name, args).await {
            Ok(v) => {
                let raw = v.to_string();
                for s in [KEY, COOKIE, PASSWORD, SEED, PROXY] {
                    ensure!(!raw.contains(s), "Secret leaked in MCP result");
                }
                ensure!(
                    (v["isError"] == true) == failure,
                    "Unexpected MCP success/error: {}",
                    self.suite.redact(&raw)
                );
                if let Some(v) = v.get("structuredContent") {
                    v.clone()
                } else {
                    serde_json::from_str(
                        v["content"]
                            .as_array()
                            .context("Missing content")?
                            .iter()
                            .filter_map(|i| i["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                            .as_str(),
                    )?
                }
            }
            Err(e) if failure && e.to_string().contains("-32602") => {
                json!({"error":{"code":"MCP_INVALID_PARAMS","message":"MCP input validation error"}})
            }
            Err(e) => return Err(e),
        };
        if no_http {
            ensure!(self.count() == before, "Rejected input caused HTTP traffic");
        }
        if failure {
            ensure!(
                data["error"]["code"].is_string() && data["error"]["message"].is_string(),
                "Missing error code/message"
            );
        }
        Ok(data)
    }
    pub async fn ok(&self, name: &str, args: Value) -> Result<Value> {
        self.call_on(&self.client, name, args, false, false).await
    }
    pub async fn fail(&self, name: &str, args: Value, no_http: bool) -> Result<Value> {
        self.call_on(&self.client, name, args, true, no_http).await
    }
    pub fn own_session(&self) -> PathBuf {
        self.root.join(sha("auth\nauthuser")).join(session_name())
    }
    pub async fn gate(&self, phase: &str) -> Result<Gate> {
        let prefix = self
            .output
            .join(format!("auth-gate-{}", rand::random::<u128>()));
        let descriptor = prefix.with_extension("json");
        let phase_path = prefix.with_extension("phase.json");
        let resume = prefix.with_extension("resume");
        let result = prefix.with_extension("result.json");
        let lose = prefix.with_extension("lose-lock");
        fs::write(&descriptor,json!({"gate":phase,"phase_path":phase_path,"resume_path":resume,"result_path":result,"lose_lock_path":lose}).to_string()).await?;
        let client = self
            .spawn(
                &self.secrets,
                &self.root,
                15000,
                None,
                &[(
                    "PLUMIVOX_TEST_HOOKS".into(),
                    descriptor.to_string_lossy().into(),
                )],
            )
            .await?;
        Ok(Gate {
            client,
            phase: phase_path,
            resume,
            result,
            lose,
        })
    }
    pub async fn finish(mut self) -> Result<()> {
        self.client.close().await?;
        self.diagnostics.push(self.client.stderr());
        self.server.close().await?;
        self.suite
            .write_json("http-trace.json", &json!(self.trace()))?;
        self.suite
            .write_json("diagnostics.json", &json!(self.diagnostics))?;
        ensure!(
            self.selected_count > 0,
            "PLUMIVOX_E2E_CASE did not match any case"
        );
        self.suite.finish()
    }
}
pub struct Gate {
    pub client: McpClient,
    pub phase: PathBuf,
    pub resume: PathBuf,
    pub result: PathBuf,
    pub lose: PathBuf,
}
pub async fn wait_json(path: &Path) -> Result<Value> {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(bytes) = fs::read(path).await {
            if let Ok(v) = serde_json::from_slice(&bytes) {
                return Ok(v);
            }
        }
        ensure!(
            Instant::now() < end,
            "Fixture file did not arrive: {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
impl Gate {
    pub async fn wait(&self) -> Result<Value> {
        wait_json(&self.phase).await
    }
    pub async fn wait_result(&self) -> Result<Value> {
        wait_json(&self.result).await
    }
    pub async fn resume(&self) -> Result<()> {
        fs::write(&self.resume, b"resume").await?;
        Ok(())
    }
    pub async fn lose(&self) -> Result<()> {
        fs::write(&self.lose, b"lose").await?;
        Ok(())
    }
}
pub fn login_metadata(v: &Value, reused: bool) -> Result<()> {
    let mut keys = v
        .as_object()
        .context("Login metadata is not an object")?
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort();
    ensure!(
        keys == [
            "account",
            "acquiredAt",
            "postConfigured",
            "reused",
            "sessionSource",
            "userName"
        ] && v["postConfigured"] == true
            && v["reused"] == reused
            && v["sessionSource"].is_string(),
        "Invalid login metadata {v}"
    );
    if !reused {
        ensure!(
            v["acquiredAt"]
                .as_str()
                .is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok()),
            "Missing acquiredAt"
        );
    }
    Ok(())
}
pub async fn checked_close(client: &mut McpClient) -> Result<()> {
    client.close().await?;
    let stderr = client.stderr();
    for secret in [KEY, COOKIE, PASSWORD, SEED, PROXY] {
        ensure!(
            !stderr.contains(secret),
            "Secret leaked in child diagnostics"
        );
    }
    Ok(())
}
pub fn unavailable(v: &Value) -> Result<()> {
    ensure!(
        ids(v) == ["1200"]
            && v["hasNextPage"] == true
            && v["nextCursor"].is_null()
            && v["coverage"]["tweetContinuationAvailable"] == false
            && v["coverage"]["tweetContinuationUnavailableReason"] == "cursor_too_long",
        "Continuation information lost"
    );
    Ok(())
}
