//! twitterapi.io client. Provider errors and credentials never escape this boundary.
// client_emoji.txt is generated from emoji-regex 11.0.0, mapping UTF-16 units
// to plane-1 scalars. Its license follows:
// Copyright Mathias Bynens <https://mathiasbynens.be/>
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
// of the Software, and to permit persons to whom the Software is furnished to do
// so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
use crate::api_rejection::RejectionCapture;
use crate::error::{AppError, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat};
use hmac::{Hmac, KeyInit, Mac};
use regex::Regex;
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::OnceLock,
    time::Duration,
};
use tokio::{io::AsyncReadExt, time::Instant};
use tokio_util::sync::CancellationToken;
use unicode_normalization::UnicodeNormalization;

const MAX_IMAGE: usize = 5 * 1024 * 1024;
const MAX_RESPONSE: usize = 10 * 1024 * 1024;
const FEED_PREFIX: &str = "plumivox-feed.";
const SEARCH_PREFIX: &str = "plumivox-search.";
const MAX_FEED: usize = 100_000;
const MAX_PROVIDER: usize = 10_000;
const MAX_SEARCH: usize = 200_000;

// 固定の合成CAとloopback HTTPSを、独立した検証プロセスだけで有効にする。
#[cfg(feature = "test-fixture")]
static TLS_FIXTURE: OnceLock<bool> = OnceLock::new();

#[cfg(feature = "test-fixture")]
pub fn configure_tls_fixture(trust_ca: bool) -> Result<()> {
    TLS_FIXTURE.set(trust_ca).map_err(|_| {
        err(
            "INVALID_FIXTURE",
            "The TLS verification mode is already set.",
        )
    })
}

fn err(code: &str, message: &str) -> AppError {
    AppError::new(code, message)
}
/// API通信が完了しなかったことを示す `API_UNAVAILABLE` を作る。
/// `failureKind` は画像アップロード（`upload_media_v2`）で種別が分かる場合だけ付け、
/// それ以外は既定の空 details のまま返す。投稿作成の POST_UNCERTAIN 変換は
/// この details をそのまま引き継ぐため、形を変えてはいけない。
fn unavailable(endpoint: &str, kind: Option<&str>) -> AppError {
    let e = err(
        "API_UNAVAILABLE",
        "The API request did not complete. It was not retried automatically.",
    );
    match kind {
        Some(kind) if endpoint == "upload_media_v2" => e.with_details(json!({"failureKind": kind})),
        _ => e,
    }
}
/// 記録対象の1件（`post_tweet` の画像アップロード・投稿作成で捕捉した、伏字済みの拒否文面）。
/// 伏字は捕捉時に済んでおり、保存は `server` が行う。
pub(crate) struct PostRejection {
    pub(crate) capture: RejectionCapture,
    pub(crate) stage: &'static str,
    pub(crate) endpoint: &'static str,
    /// 画像アップロードだけ。投稿全体での要求の通し番号（応答の `uploadAttempts` と同じ数え方）。
    pub(crate) upload_attempt: Option<usize>,
    pub(crate) code: String,
    /// `api_rejection::details` の公開用の欄（httpStatus・rejectionReasons・rejectionReasonSource）。
    pub(crate) details: Value,
}
/// 記録が有効な `post_tweet` だけが作る。投稿テキスト、伏せる既知の値（選んだプロファイルの
/// 全文字列など。送信前に設定から取る）と、捕捉した全件（再試行ごとに1件）を持つ。
pub(crate) struct Recorder {
    post_text: String,
    pub(crate) values: Vec<String>,
    pub(crate) items: Vec<PostRejection>,
}
impl Recorder {
    pub(crate) fn new(post_text: &str, values: Vec<String>) -> Self {
        Self {
            post_text: post_text.to_owned(),
            values,
            items: Vec::new(),
        }
    }
    /// エラーが捕捉値を持っていれば取り出して控える（エラー自体には残さない）。
    fn keep(
        &mut self,
        e: &mut AppError,
        stage: &'static str,
        endpoint: &'static str,
        upload_attempt: Option<usize>,
    ) {
        if let Some(capture) = e.rejection.take() {
            self.items.push(PostRejection {
                capture: *capture,
                stage,
                endpoint,
                upload_attempt,
                code: e.code.clone(),
                details: e.details.clone(),
            });
        }
    }
}
fn collect_known<'a>(v: &'a Value, post_text: &str, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) if s != post_text => out.push(s),
        Value::Array(items) => items.iter().for_each(|v| collect_known(v, post_text, out)),
        Value::Object(map) => map.values().for_each(|v| collect_known(v, post_text, out)),
        _ => {}
    }
}
/// 既知の値による伏字（規則2）の入力: apiKey・loginCookies・proxy・userName、要求本文（フォームの
/// 文字列は loginCookies・proxy と同じ）の全文字列、`Recorder` が持つ設定由来の値、投稿テキスト。
/// 全部をここで集め、パターンや切り詰めより先に処理する。
fn capture_known(
    response: &Value,
    a: &Value,
    body: Option<&Value>,
    recorder: &Recorder,
) -> RejectionCapture {
    let mut known: Vec<&str> = ["apiKey", "loginCookies", "proxy", "userName"]
        .iter()
        .map(|key| string(&a[*key]))
        .collect();
    if let Some(body) = body {
        collect_known(body, &recorder.post_text, &mut known);
    }
    known.extend(recorder.values.iter().map(String::as_str));
    crate::api_rejection::capture(response, &known, Some(&recorder.post_text))
}
fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn len(s: &str) -> usize {
    s.encode_utf16().count()
}
fn numeric(v: &Value) -> bool {
    let s = string(v);
    !s.is_empty() && s.len() <= 30 && s.bytes().all(|b| b.is_ascii_digit())
}
fn username(s: &str) -> bool {
    !s.is_empty() && s.len() <= 15 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn text_value(v: &Value) -> Value {
    if v.is_string() {
        v.clone()
    } else {
        Value::Null
    }
}
fn normalize(v: &Value) -> Result<Value> {
    if !v.is_object() || !numeric(&v["id"]) {
        return Err(err(
            "INVALID_RESPONSE",
            "The post data has an invalid format.",
        ));
    }
    Ok(
        json!({"id":v["id"],"url":format!("https://x.com/i/status/{}",string(&v["id"])),
        "text":v["text"].as_str().unwrap_or(""),"createdAt":text_value(&v["createdAt"]),
        "author":if v["author"].is_object(){json!({"id":text_value(&v["author"]["id"]),"userName":text_value(&v["author"]["userName"]),"name":text_value(&v["author"]["name"])})}else{Value::Null},
        "isReply":v["isReply"]==true,"isRetweet":v["isRetweet"]==true || v["retweeted_tweet"].is_object(),"isQuote":v["isQuote"]==true}),
    )
}
fn page(v: &Value, max: usize) -> Result<Value> {
    let tweets = v["tweets"]
        .as_array()
        .ok_or_else(|| err("INVALID_RESPONSE", "The post list could not be read."))?;
    let has = v["has_next_page"] == true;
    let cursor = string(&v["next_cursor"]);
    if has && cursor.is_empty() {
        return Err(err(
            "INVALID_RESPONSE",
            "The information needed to fetch more results is missing.",
        ));
    }
    let too_long = has && len(cursor) > max;
    let mut result = json!({"tweets":tweets.iter().map(normalize).collect::<Result<Vec<_>>>()?,"hasNextPage":has,
        "nextCursor":if has && !too_long {json!(cursor)}else{Value::Null}});
    if too_long {
        result["coverage"] = json!({"tweetContinuationAvailable":false,"tweetContinuationUnavailableReason":"cursor_too_long"});
    }
    Ok(result)
}
fn encode(data: &Value, account: &Value, prefix: &str, max: usize) -> Option<String> {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(data).ok()?);
    let mut mac = Hmac::<Sha256>::new_from_slice(string(&account["apiKey"]).as_bytes()).ok()?;
    mac.update(payload.as_bytes());
    let cursor = format!(
        "{prefix}{payload}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    );
    (cursor.len() <= max).then_some(cursor)
}
fn decode(cursor: &str, account: &Value, prefix: &str, max: usize) -> Option<Value> {
    if len(cursor) > max {
        return None;
    }
    let (payload, sig) = cursor.strip_prefix(prefix)?.split_once('.')?;
    if payload.is_empty()
        || !payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || sig.len() != 43
    {
        return None;
    }
    let signature = URL_SAFE_NO_PAD.decode(sig).ok()?;
    if URL_SAFE_NO_PAD.encode(&signature) != sig {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(string(&account["apiKey"]).as_bytes()).ok()?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature).ok()?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}
fn feed_encode(batches: &Value, coverage: &Value, account: &Value) -> Option<String> {
    encode(
        &json!({"version":2,"account":account["account"],"userName":string(&account["userName"]).to_ascii_lowercase(),"batches":batches,"coverage":coverage}),
        account,
        FEED_PREFIX,
        MAX_FEED,
    )
}
fn feed_decode(cursor: &str, account: &Value) -> Result<(Vec<Value>, Value)> {
    let invalid = || {
        err(
            "INVALID_CURSOR",
            "The feed cursor is invalid or belongs to another account.",
        )
    };
    let data = decode(cursor, account, FEED_PREFIX, MAX_FEED).ok_or_else(invalid)?;
    let c = &data["coverage"];
    let batches = data["batches"].as_array().ok_or_else(invalid)?;
    if data["version"] != 2
        || data["account"] != account["account"]
        || data["userName"] != string(&account["userName"]).to_ascii_lowercase()
        || batches.is_empty()
        || batches.len() > 10
        || !c.is_object()
        || !c["followingHasNextPage"].is_boolean()
        || !c["accounts"].as_u64().is_some_and(|n| n <= 200)
        || c["protectedAccountsExcluded"].as_u64().is_none()
        || !c["protectionStatusUnknownAccounts"]
            .as_u64()
            .is_some_and(|n| n <= c["accounts"].as_u64().unwrap_or(0))
    {
        return Err(invalid());
    }
    for b in batches {
        let users = b["users"].as_array().ok_or_else(invalid)?;
        if users.is_empty()
            || users.len() > 20
            || !users.iter().all(|v| username(string(v)))
            || string(&b["cursor"]).is_empty()
            || len(string(&b["cursor"])) > MAX_PROVIDER
        {
            return Err(invalid());
        }
    }
    let coverage = json!({"accounts":c["accounts"],"followingHasNextPage":c["followingHasNextPage"],"followingNextCursor":text_value(&c["followingNextCursor"]),
        "protectedAccountsExcluded":c["protectedAccountsExcluded"],"protectionStatusUnknownAccounts":c["protectionStatusUnknownAccounts"],
        "followingPage":if c["followingPage"]=="continuation"{"continuation"}else{"first"},
        "followingContinuationAvailable":c["followingContinuationAvailable"]==true,
        "followingContinuationUnavailableReason":if matches!(string(&c["followingContinuationUnavailableReason"]), "cursor_too_long" | "account_limit"){c["followingContinuationUnavailableReason"].clone()}else{Value::Null}});
    Ok((batches.clone(), coverage))
}
fn search_limit(c: &Value, kind: &str) -> usize {
    if kind == "tweets" && c["scope"] == "following" {
        MAX_FEED
    } else {
        MAX_PROVIDER
    }
}
fn search_encode(inner: &str, a: &Value, c: &Value, kind: &str) -> Option<String> {
    if inner.is_empty() || len(inner) > search_limit(c, kind) {
        return None;
    }
    encode(
        &json!({"version":1,"kind":kind,"conditions":c,"inner":inner}),
        a,
        SEARCH_PREFIX,
        MAX_SEARCH,
    )
}
fn search_decode(v: &Value, a: &Value, c: &Value, kind: &str) -> Result<String> {
    let invalid = || {
        err(
            "INVALID_CURSOR",
            "The search cursor is invalid, or the search conditions or account do not match.",
        )
    };
    let d = decode(string(v), a, SEARCH_PREFIX, MAX_SEARCH).ok_or_else(invalid)?;
    let inner = string(&d["inner"]);
    if d["version"] != 1
        || d["kind"] != kind
        || d["conditions"] != *c
        || inner.is_empty()
        || len(inner) > search_limit(c, kind)
    {
        return Err(invalid());
    }
    Ok(inner.into())
}
fn search_time(v: Option<&Value>) -> Result<Option<(i64, String)>> {
    let Some(v) = v else { return Ok(None) };
    static FORMAT: OnceLock<Regex> = OnceLock::new();
    let s = string(v);
    if !FORMAT
        .get_or_init(|| {
            Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(Z|[+-]\d{2}:\d{2})$").unwrap()
        })
        .is_match(s)
    {
        return Err(err(
            "INVALID_SEARCH",
            "Specify date-times in ISO 8601 with seconds precision and a time zone.",
        ));
    }
    let t = DateTime::parse_from_rfc3339(s).map_err(|_| {
        err(
            "INVALID_SEARCH",
            "The search date-time is invalid. Specify a date and time that exist.",
        )
    })?;
    if s.get(17..19) == Some("60") {
        return Err(err(
            "INVALID_SEARCH",
            "The search date-time is invalid. Specify a date and time that exist.",
        ));
    }
    Ok(Some((
        t.timestamp(),
        t.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true),
    )))
}

pub struct TwitterApiClient {
    base: String,
    timeout: Duration,
    image_timeout: Duration,
    http: std::result::Result<reqwest::Client, reqwest::Error>,
}
pub struct PreparedPost {
    text: String,
    weighted: usize,
    files: Vec<Image>,
}
impl PreparedPost {
    /// 拒否記録の組み立て用（本文・加重文字数・画像の枚数）。
    pub(crate) fn summary(&self) -> (String, usize, usize) {
        (self.text.clone(), self.weighted, self.files.len())
    }
}
struct Image {
    bytes: Vec<u8>,
    mime: &'static str,
    name: &'static str,
}
impl TwitterApiClient {
    pub fn new(api_base_url: String, timeout: Duration, image_timeout: Duration) -> Self {
        // reqwest 0.13の既定はOSの証明書ストアとAWS-LCへ変わった。
        // 従来と同じwebpkiのルート・ring・TLS 1.2/1.3をクライアントごとに固定する。
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        #[cfg(feature = "test-fixture")]
        let roots = {
            let mut roots = roots;
            if TLS_FIXTURE.get() == Some(&true) {
                roots
                    .add(rustls::pki_types::CertificateDer::from(
                        include_bytes!("../tests/fixtures/tls/ca-cert.der").to_vec(),
                    ))
                    .expect("fixed synthetic TLS fixture CA");
            }
            roots
        };
        let mut tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .expect("ring supports the configured TLS 1.2 and TLS 1.3 versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        Self {
            base: api_base_url,
            timeout,
            image_timeout,
            http: reqwest::Client::builder()
                .tls_backend_preconfigured(tls)
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build(),
        }
    }
    async fn request(
        &self,
        a: &Value,
        endpoint: &str,
        query: Option<&Value>,
        body: Option<&Value>,
        form: Option<reqwest::multipart::Form>,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> Result<Value> {
        self.request_with(a, endpoint, query, body, form, cancel, timeout, None)
            .await
    }
    #[allow(clippy::too_many_arguments)] // request_status の引数をそのまま渡す。
    async fn request_with(
        &self,
        a: &Value,
        endpoint: &str,
        query: Option<&Value>,
        body: Option<&Value>,
        form: Option<reqwest::multipart::Form>,
        cancel: &CancellationToken,
        timeout: Duration,
        recorder: Option<&Recorder>,
    ) -> Result<Value> {
        self.request_status(a, endpoint, query, body, form, cancel, timeout, recorder)
            .await
            .map(|(v, _)| v)
    }
    /// 成功時は本文と実際の HTTP ステータス（2xx）を返す。
    /// `recorder` が Some のときだけ、2xx 本文エラー（`API_REJECTED`）の文面を伏字処理して
    /// エラーに付ける（ローカルの拒否記録が有効な投稿だけが使う）。
    #[allow(clippy::too_many_arguments)] // request の引数に記録の有無を足しただけ。
    async fn request_status(
        &self,
        a: &Value,
        endpoint: &str,
        query: Option<&Value>,
        body: Option<&Value>,
        form: Option<reqwest::multipart::Form>,
        cancel: &CancellationToken,
        timeout: Duration,
        recorder: Option<&Recorder>,
    ) -> Result<(Value, u16)> {
        let operation = async {
            // 通信先の解析失敗は upload でも failureKind を付けない（従来どおり）。
            let base = url::Url::parse(&self.base).map_err(|_| unavailable(endpoint, None))?;
            #[cfg(not(feature = "test-fixture"))]
            let tls_fixture_endpoint = false;
            #[cfg(feature = "test-fixture")]
            let tls_fixture_endpoint = TLS_FIXTURE.get().is_some()
                && base.scheme() == "https"
                && base.host_str() == Some("127.0.0.1")
                && base.port().is_some()
                && base.path() == "/"
                && base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none();
            if !(base.scheme() == "https"
                && base.host_str() == Some("api.twitterapi.io")
                && base.port_or_known_default() == Some(443))
                && !(base.scheme() == "http"
                    && matches!(base.host_str(), Some("127.0.0.1" | "[::1]" | "::1")))
                && !tls_fixture_endpoint
            {
                return Err(err("API_UNAVAILABLE", "The API endpoint is invalid."));
            }
            let mut url = base
                .join(&format!("/twitter/{endpoint}"))
                .map_err(|_| err("API_UNAVAILABLE", "The API endpoint is invalid."))?;
            if let Some(q) = query.and_then(Value::as_object) {
                for (k, v) in q {
                    if !v.is_null() {
                        url.query_pairs_mut().append_pair(
                            k,
                            &v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string()),
                        );
                    }
                }
            }
            // HTTP クライアント構築失敗も upload で failureKind を付けない（従来どおり）。
            let client = self
                .http
                .as_ref()
                .map_err(|_| unavailable(endpoint, None))?;
            let mut req = if body.is_some() || form.is_some() {
                client.post(url)
            } else {
                client.get(url)
            };
            req = req.header("X-API-Key", string(&a["apiKey"]));
            if let Some(b) = body {
                req = req.json(b);
            }
            if let Some(f) = form {
                req = req.multipart(f);
            }
            let network = || unavailable(endpoint, Some("network"));
            let mut response = req.send().await.map_err(|_| network())?;
            if response.status().is_redirection() {
                return Err(network());
            }
            if !response.status().is_success() {
                return Err(err("API_HTTP_ERROR", "The API did not accept the request.")
                    .with_details(crate::api_rejection::details(
                        response.status().as_u16(),
                        None,
                    )));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                    return Err(err(
                        "INVALID_RESPONSE",
                        "The API response exceeded the size limit.",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            let v: Value =
                serde_json::from_str(&String::from_utf8_lossy(&bytes)).map_err(|_| {
                    err(
                        "INVALID_RESPONSE",
                        "No JSON response could be read from the API.",
                    )
                })?;
            if !v.is_object() {
                return Err(err(
                    "INVALID_RESPONSE",
                    "The API response has an invalid format.",
                ));
            }
            if v["status"] == "error" || v["status"] == "failed" || v["success"] == false {
                let mut rejected =
                    err("API_REJECTED", "The API reported that the request failed.").with_details(
                        crate::api_rejection::details(response.status().as_u16(), Some(&v)),
                    );
                rejected.rejection =
                    recorder.map(|recorder| Box::new(capture_known(&v, a, body, recorder)));
                return Err(rejected);
            }
            Ok((v, response.status().as_u16()))
        };
        let result = tokio::select! { biased;
            _=cancel.cancelled()=>Err(unavailable(endpoint,Some("cancelled"))),
            r=tokio::time::timeout(timeout,operation)=>r.unwrap_or_else(|_|Err(unavailable(endpoint,Some("timeout"))))
        };
        result.map_err(|e| {
            if endpoint == "create_tweet_v2"
                && (e.code == "INVALID_RESPONSE"
                    || e.code == "API_UNAVAILABLE"
                    || (e.code == "API_HTTP_ERROR"
                        && e.details["httpStatus"].as_u64().unwrap_or(0) >= 500))
            {
                err(
                    "POST_UNCERTAIN",
                    "Whether the post succeeded is unknown. It may have been posted. It was not resent automatically.",
                )
                .with_details(e.details)
            } else {
                e
            }
        })
    }
    pub async fn get_my_posts(
        &self,
        a: &Value,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let info = self
            .request(
                a,
                "user/info",
                Some(&json!({"userName":a["userName"]})),
                None,
                None,
                &cancel,
                self.timeout,
            )
            .await?;
        if !numeric(&info["data"]["id"]) {
            return Err(err(
                "INVALID_RESPONSE",
                "The account ID could not be confirmed.",
            ));
        }
        let mut result=self.request(a,"user/tweet_timeline",Some(&json!({"userId":info["data"]["id"],"includeReplies":input["includeReplies"].as_bool().unwrap_or(false),"includeParentTweet":false,"cursor":input["cursor"]})),None,None,&cancel,self.timeout).await?;
        if result.get("tweets").is_none() && result["data"].is_object() {
            result["tweets"] = result["data"]["tweets"].clone();
        }
        let mut p = page(&result, MAX_FEED)?;
        p["account"] = a["account"].clone();
        Ok(p)
    }
    pub async fn get_tweets(
        &self,
        a: &Value,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let mut seen = HashSet::new();
        let ids = input["tweetIds"]
            .as_array()
            .ok_or_else(|| err("INVALID_INPUT", "Specify post IDs."))?
            .iter()
            .map(string)
            .filter(|s| seen.insert(*s))
            .collect::<Vec<_>>()
            .join(",");
        let v = self
            .request(
                a,
                "tweets",
                Some(&json!({"tweet_ids":ids})),
                None,
                None,
                &cancel,
                self.timeout,
            )
            .await?;
        let tweets = v["tweets"]
            .as_array()
            .ok_or_else(|| err("INVALID_RESPONSE", "The post list could not be read."))?
            .iter()
            .map(normalize)
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"account":a["account"],"tweets":tweets}))
    }
    pub async fn login_account(&self, a: &Value, cancel: CancellationToken) -> Result<String> {
        let mut body = json!({"user_name":a["userName"],"email":a["email"],"password":a["password"],"proxy":a["proxy"]});
        if !string(&a["totpSecret"]).is_empty() {
            body["totp_secret"] = a["totpSecret"].clone();
        }
        let v = self
            .request(
                a,
                "user_login_v2",
                None,
                Some(&body),
                None,
                &cancel,
                self.timeout,
            )
            .await?;
        let cookie = v
            .get("login_cookie")
            .filter(|v| !v.is_null())
            .unwrap_or(&v["login_cookies"]);
        if v["status"] != "success"
            || !cookie.is_string()
            || string(cookie).trim().is_empty()
            || len(string(cookie)) > 1024 * 1024
        {
            return Err(err(
                "LOGIN_FAILED",
                "The login cookie could not be confirmed. The saved session was not changed.",
            ));
        }
        Ok(string(cookie).into())
    }
    pub async fn get_following_feed(
        &self,
        a: &Value,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<Value> {
        self.following(a, input, &cancel, None).await
    }
    async fn following(
        &self,
        a: &Value,
        input: &Value,
        cancel: &CancellationToken,
        search_query: Option<&str>,
    ) -> Result<Value> {
        let max = input["maxAccounts"].as_u64().unwrap_or(100) as usize;
        if !(20..=200).contains(&max) {
            return Err(err(
                "INVALID_SEARCH",
                "maxAccounts must be an integer from 20 to 200.",
            ));
        }
        let cursor = string(&input["cursor"]);
        let following = string(&input["followingCursor"]);
        if !cursor.is_empty() && !following.is_empty() {
            return Err(err(
                "INVALID_CURSOR",
                "cursor and followingCursor cannot be specified together.",
            ));
        }
        if following.starts_with(SEARCH_PREFIX) {
            return Err(err(
                "INVALID_CURSOR",
                "Pass a search followingCursor to search_tweets.",
            ));
        }
        let mut requests = 0;
        let (batches, coverage) = if !cursor.is_empty() {
            feed_decode(cursor, a)?
        } else {
            let v=self.request(a,"user/followings",Some(&json!({"userName":a["userName"],"pageSize":max,"cursor":input["followingCursor"]})),None,None,cancel,self.timeout).await?;
            requests += 1;
            let users = v["followings"]
                .as_array()
                .ok_or_else(|| err("INVALID_RESPONSE", "The following list could not be read."))?;
            let mut protection = HashMap::<String, u8>::new();
            let mut order = Vec::new();
            for user in users {
                let name = string(&user["userName"]);
                if !username(name) {
                    continue;
                }
                let name = name.to_ascii_lowercase();
                // 同名の全行を集約し、保護 > 公開 > 不明の順で判定する。
                let status = if user["isProtected"] == true || user["protected"] == true {
                    2
                } else if user["isProtected"] == false || user["protected"] == false {
                    1
                } else {
                    0
                };
                let current = protection.entry(name.clone()).or_insert_with(|| {
                    order.push(name);
                    0
                });
                *current = (*current).max(status);
            }
            let protected = protection.values().filter(|&&status| status == 2).count();
            let account_limit = order.len() - protected > max;
            let names = order
                .into_iter()
                .filter(|name| protection[name] != 2)
                .take(max)
                .collect::<Vec<_>>();
            let unknown = names.iter().filter(|name| protection[*name] == 0).count();
            let has = v["has_next_page"] == true || account_limit;
            let raw = string(&v["next_cursor"]);
            let too_long = len(raw) > MAX_PROVIDER;
            let c = json!({"accounts":names.len(),"followingHasNextPage":has,"followingNextCursor":if has && !account_limit && !raw.is_empty() && !too_long{json!(raw)}else{Value::Null},
                "protectedAccountsExcluded":protected,"protectionStatusUnknownAccounts":unknown,"followingPage":if following.is_empty(){"first"}else{"continuation"},
                "followingContinuationAvailable":has && !account_limit && !raw.is_empty() && !too_long,"followingContinuationUnavailableReason":if account_limit{json!("account_limit")}else if has && too_long{json!("cursor_too_long")}else{Value::Null}});
            (
                names
                    .chunks(20)
                    .map(|users| json!({"users":users,"cursor":null}))
                    .collect(),
                c,
            )
        };
        let mut tweets = Vec::<Value>::new();
        let mut positions = HashMap::<String, usize>::new();
        let mut pending = Vec::new();
        let mut has = false;
        let mut too_long = false;
        for batch in batches {
            let users = batch["users"]
                .as_array()
                .ok_or_else(|| err("INVALID_CURSOR", "The feed cursor is invalid."))?;
            let from = users
                .iter()
                .map(|v| format!("from:{}", string(v)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let query = format!(
                "({from}) include:nativeretweets{}",
                search_query.map(|s| format!(" {s}")).unwrap_or_default()
            );
            let mut p = page(
                &self
                    .request(
                        a,
                        "tweet/advanced_search",
                        Some(&json!({"query":query,"queryType":"Latest","cursor":batch["cursor"]})),
                        None,
                        None,
                        cancel,
                        self.timeout,
                    )
                    .await?,
                MAX_PROVIDER,
            )?;
            requests += 1;
            // pageで正規化済みの投稿を移動し、本文・authorの複製を避ける。
            // 残りの継続情報はこの後も同じページから読み取る。
            for t in p["tweets"].as_array_mut().unwrap().drain(..) {
                let id = string(&t["id"]).to_owned();
                if let Some(index) = positions.get(&id) {
                    tweets[*index] = t;
                } else {
                    positions.insert(id, tweets.len());
                    tweets.push(t);
                }
            }
            has |= p["hasNextPage"] == true;
            too_long |= p["coverage"]["tweetContinuationUnavailableReason"] == "cursor_too_long";
            if p["hasNextPage"] == true && p["nextCursor"].is_string() {
                pending.push(json!({"users":users,"cursor":p["nextCursor"]}));
            }
        }
        // The JS comparator puts the later item first when numeric IDs are
        // equal (e.g. "070" and "70"). Preserve that deterministic tie rule.
        tweets.sort_by_cached_key(|tweet| {
            let time = DateTime::parse_from_rfc3339(string(&tweet["createdAt"]))
                .or_else(|_| DateTime::parse_from_rfc2822(string(&tweet["createdAt"])))
                .or_else(|_| {
                    DateTime::parse_from_str(string(&tweet["createdAt"]), "%a %b %d %H:%M:%S %z %Y")
                })
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            let id = string(&tweet["id"]);
            std::cmp::Reverse((time, id.parse::<u128>().unwrap_or(0), positions[id]))
        });
        let next = if !pending.is_empty() && !too_long {
            feed_encode(&json!(pending), &coverage, a)
        } else {
            None
        };
        if !pending.is_empty() && next.is_none() {
            too_long = true;
        }
        let mut c = coverage;
        c["protectedTweetsAvailable"] = json!(false);
        c["completeFollowingList"] =
            json!(c["followingHasNextPage"] != true && c["followingPage"] != "continuation");
        c["globalTimelineOrderGuaranteed"] = json!(false);
        c["tweetContinuationAvailable"] = json!(next.is_some());
        c["tweetContinuationUnavailableReason"] = if too_long {
            json!("cursor_too_long")
        } else {
            Value::Null
        };
        Ok(
            json!({"account":a["account"],"mode":"reconstructed_following_feed","officialHomeEquivalent":false,"tweets":tweets,"hasNextPage":has,"nextCursor":next,"coverage":c,"apiRequests":requests}),
        )
    }
    pub async fn search_tweets(
        &self,
        a: &Value,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let keyword = string(&input["keyword"]);
        if keyword.trim().is_empty()
            || len(keyword.trim()) > 512
            || keyword.chars().any(|c| {
                c == '"' || c == '\\' || c.is_control() || c == '\u{2028}' || c == '\u{2029}'
            })
        {
            return Err(err(
                "INVALID_SEARCH",
                "The keyword must be a non-empty phrase of at most 512 characters. Quotes, backslashes and control characters are not allowed.",
            ));
        }
        let scope = input.get("scope").map(string).unwrap_or("all");
        if !matches!(scope, "all" | "following") {
            return Err(err("INVALID_SEARCH", "Set scope to all or following."));
        }
        if input.get("cursor").is_some() && input.get("followingCursor").is_some() {
            return Err(err(
                "INVALID_CURSOR",
                "cursor and followingCursor cannot be specified together.",
            ));
        }
        if scope == "all"
            && (input.get("maxAccounts").is_some() || input.get("followingCursor").is_some())
        {
            return Err(err(
                "INVALID_SEARCH",
                "maxAccounts and followingCursor cannot be specified when scope is all.",
            ));
        }
        if input.get("maxAccounts").is_some()
            && !input["maxAccounts"]
                .as_u64()
                .is_some_and(|n| (20..=200).contains(&n))
        {
            return Err(err(
                "INVALID_SEARCH",
                "maxAccounts must be an integer from 20 to 200.",
            ));
        }
        let start = search_time(input.get("startTime"))?;
        let end = search_time(input.get("endTime"))?;
        if let (Some(s), Some(e)) = (&start, &end) {
            if s.0 >= e.0 {
                return Err(err(
                    "INVALID_SEARCH",
                    "The start time must be earlier than the end time.",
                ));
            }
        }
        let conditions = json!({"account":a["account"],"userName":string(&a["userName"]).to_ascii_lowercase(),"scope":scope,"keyword":keyword.trim(),
            "startEpoch":start.as_ref().map(|s|s.0),"endEpoch":end.as_ref().map(|s|s.0),"maxAccounts":if scope=="following"{json!(input["maxAccounts"].as_u64().unwrap_or(100))}else{Value::Null}});
        let mut query = format!("\"{}\"", keyword.trim());
        if let Some(s) = &start {
            query.push_str(&format!(" since_time:{}", s.0));
        }
        if let Some(e) = &end {
            query.push_str(&format!(" until_time:{}", e.0));
        }
        let cursor = input
            .get("cursor")
            .map(|v| search_decode(v, a, &conditions, "tweets"))
            .transpose()?;
        let following = input
            .get("followingCursor")
            .map(|v| search_decode(v, a, &conditions, "following"))
            .transpose()?;
        let mut result = if scope == "all" {
            let mut p = page(
                &self
                    .request(
                        a,
                        "tweet/advanced_search",
                        Some(&json!({"query":query,"queryType":"Latest","cursor":cursor})),
                        None,
                        None,
                        &cancel,
                        self.timeout,
                    )
                    .await?,
                MAX_PROVIDER,
            )?;
            p["apiRequests"] = json!(1);
            p["coverage"] = json!({"publicSearch":true,"protectedTweetsAvailable":false,"completeSearch":false,"globalTimelineOrderGuaranteed":false,
                "tweetContinuationAvailable":p["nextCursor"].is_string(),"tweetContinuationUnavailableReason":p["coverage"]["tweetContinuationUnavailableReason"]});
            p
        } else {
            let mut p=self.following(a,&json!({"maxAccounts":conditions["maxAccounts"],"cursor":cursor,"followingCursor":following}),&cancel,Some(&query)).await?;
            let next = string(&p["coverage"]["followingNextCursor"]);
            if p["coverage"]["followingHasNextPage"] == true
                && next.is_empty()
                && p["coverage"]["followingContinuationUnavailableReason"].is_null()
            {
                return Err(err(
                    "INVALID_RESPONSE",
                    "The continuation information for the following list is missing.",
                ));
            }
            let encoded = search_encode(next, a, &conditions, "following");
            if !next.is_empty()
                && encoded.is_none()
                && p["coverage"]["followingContinuationUnavailableReason"].is_null()
            {
                p["coverage"]["followingContinuationUnavailableReason"] = json!("cursor_too_long");
            }
            p["coverage"]["followingContinuationAvailable"] =
                json!(p["coverage"]["followingHasNextPage"] == true && encoded.is_some());
            p["coverage"]["followingNextCursor"] = json!(encoded);
            p
        };
        let inner = string(&result["nextCursor"]);
        let next = search_encode(inner, a, &conditions, "tweets");
        if !inner.is_empty()
            && next.is_none()
            && result["coverage"]["tweetContinuationUnavailableReason"].is_null()
        {
            result["coverage"]["tweetContinuationUnavailableReason"] = json!("cursor_too_long");
        }
        result["coverage"]["tweetContinuationAvailable"] = json!(next.is_some());
        Ok(
            json!({"account":a["account"],"scope":scope,"keyword":keyword.trim(),"startTime":start.map(|s|s.1),"endTime":end.map(|s|s.1),
            "tweets":result["tweets"],"hasNextPage":result["hasNextPage"],"nextCursor":next,"apiRequests":result["apiRequests"],"coverage":result["coverage"]}),
        )
    }
    pub async fn prepare_post(
        &self,
        _account: &Value,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<PreparedPost> {
        let result = async {
            let text = string(&input["text"]);
            let weighted = weighted_text(text)?;
            let mut files = Vec::new();
            if let Some(images) = input.get("images") {
                let images = images
                    .as_array()
                    .ok_or_else(|| err("INVALID_IMAGE", "Specify absolute paths for images."))?;
                if images.len() > 4 {
                    return Err(err("INVALID_IMAGE", "Attach at most 4 images."));
                }
                for image in images {
                    files.push(read_image(string(image)).await?);
                }
            }
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            Ok(PreparedPost {
                text: text.into(),
                weighted,
                files,
            })
        }
        .await;
        result.map_err(|e| post_error(e, "input_validation", false, 0))
    }
    /// `recorder` が Some のときだけ（ローカルの拒否記録が有効なとき）、画像アップロードと
    /// 投稿作成の拒否文面を伏字処理して `recorder` に控える。無いときは捕捉の処理を一切しない。
    pub(crate) async fn send_post(
        &self,
        a: &Value,
        prepared: PreparedPost,
        cancel: CancellationToken,
        mut recorder: Option<&mut Recorder>,
    ) -> Result<Value> {
        let mut stage = "image_upload";
        let mut attempted = false;
        let mut attempts = 0;
        let result=async{
            let mut ids=Vec::new();
            for image in &prepared.files{
                // 要求回数は失敗時にも post_error が読むため、戻り値ではなく参照で数える。
                ids.push(self.upload_image(a,image,&cancel,&mut attempts,recorder.as_deref_mut()).await?);
            }
            if cancel.is_cancelled(){return Err(cancelled());}
            stage="post_creation";attempted=true;
            let mut body=json!({"login_cookies":a["loginCookies"],"proxy":a["proxy"],"tweet_text":prepared.text});
            if !ids.is_empty(){body["media_ids"]=json!(ids);}
            let (v,http_status)=match self.request_status(a,"create_tweet_v2",None,Some(&body),None,&cancel,self.timeout,recorder.as_deref()).await{
                Ok(r)=>r,
                Err(mut e)=>{
                    if let Some(r)=recorder.as_deref_mut(){r.keep(&mut e,"post_creation","create_tweet_v2",None);}
                    return Err(e);
                }
            };
            if v["status"]!="success" || !numeric(&v["tweet_id"]){
                let e=err("POST_UNCERTAIN","Whether the post succeeded is unknown because no post ID was returned. It was not resent automatically.");
                if let Some(r)=recorder.as_deref_mut(){
                    let capture=capture_known(&v,a,Some(&body),r);
                    r.items.push(PostRejection{capture,stage:"post_creation",endpoint:"create_tweet_v2",upload_attempt:None,code:e.code.clone(),details:crate::api_rejection::details(http_status,Some(&v))});
                }
                return Err(e);
            }
            Ok(json!({"account":a["account"],"tweetId":v["tweet_id"],"url":format!("https://x.com/i/status/{}",string(&v["tweet_id"])),"weightedLength":prepared.weighted}))
        }.await;
        result.map_err(|e| post_error(e, stage, attempted, attempts))
    }
    /// 画像1枚をアップロードし、確認できた media_id を返す。
    /// - 期限 `deadline` は画像ごとに1回だけ作り、再試行で延長しない。
    /// - 再試行は初回が HTTP 502/503/504 で、残り時間が250msを超える場合の1回だけ。
    ///   投稿作成はここでは扱わず、自動再試行もしない。
    /// - `attempts` は要求を送る直前に加算する（失敗時の診断で送信回数を示すため）。
    async fn upload_image(
        &self,
        a: &Value,
        image: &Image,
        cancel: &CancellationToken,
        attempts: &mut usize,
        mut recorder: Option<&mut Recorder>,
    ) -> Result<Value> {
        let deadline = Instant::now() + self.image_timeout;
        let mut uploaded = None;
        for attempt in 0..2 {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(err("API_UNAVAILABLE", "The API request did not complete."));
            }
            let part = reqwest::multipart::Part::bytes(image.bytes.clone())
                .file_name(image.name)
                .mime_str(image.mime)
                .map_err(|_| err("INVALID_IMAGE", "The image file could not be read."))?;
            let form = reqwest::multipart::Form::new()
                .text("login_cookies", string(&a["loginCookies"]).to_owned())
                .text("proxy", string(&a["proxy"]).to_owned())
                .part("file", part);
            *attempts += 1;
            // 画像アップロードは再試行も1回ごとに1件として控える。
            match self
                .request_with(
                    a,
                    "upload_media_v2",
                    None,
                    None,
                    Some(form),
                    cancel,
                    remaining,
                    recorder.as_deref(),
                )
                .await
            {
                Ok(v) => {
                    uploaded = Some(v);
                    break;
                }
                Err(mut e) => {
                    if let Some(r) = recorder.as_deref_mut() {
                        r.keep(&mut e, "image_upload", "upload_media_v2", Some(*attempts));
                    }
                    if cancel.is_cancelled() {
                        return Err(cancelled());
                    }
                    // 再試行可否と待機時間は upload_retry_delay が決める。1回だけ、
                    // かつ画像ごとの期限に待機分が残る場合に限る。
                    let delay = match upload_retry_delay(&e) {
                        Some(d) if attempt == 0 => d,
                        _ => return Err(e),
                    };
                    if deadline.saturating_duration_since(Instant::now()) <= delay {
                        return Err(e);
                    }
                    // 待機中の取消は投稿前の中止として扱う。
                    tokio::select! {_=cancel.cancelled()=>return Err(cancelled()),_=tokio::time::sleep(delay)=>{}}
                }
            }
        }
        let uploaded = uploaded.ok_or_else(|| {
            err(
                "UPLOAD_FAILED",
                "The image upload could not be confirmed. The post was not sent.",
            )
        })?;
        if uploaded["status"] != "success" || !numeric(&uploaded["media_id"]) {
            return Err(err(
                "UPLOAD_FAILED",
                "The image upload could not be confirmed. The post was not sent.",
            ));
        }
        Ok(uploaded["media_id"].clone())
    }
}
/// 画像アップロード失敗のうち、同じ画像をもう一度送って良い一時的な失敗の待機時間を返す。
/// アップロードは投稿を作らず副作用が孤立メディアに限られるため再試行できる。
/// - HTTP 502/503/504: 250ms（従来どおり）。
/// - 住宅プロキシ・上流の一時障害や、原因不明のJSON拒否（status=error）、
///   通信切断（network）: 1秒。プロキシ経路の切替わりを待つため長めにする。
/// 認証・残高・メディア不正・重複・レート制限など、再送しても直らない拒否は対象外。
/// タイムアウトは期限を使い切っているため対象外。
fn upload_retry_delay(e: &AppError) -> Option<Duration> {
    match e.code.as_str() {
        "API_HTTP_ERROR" => matches!(e.details["httpStatus"].as_u64(), Some(502 | 503 | 504))
            .then_some(Duration::from_millis(250)),
        "API_REJECTED" => {
            let reasons = e.details["rejectionReasons"].as_array()?;
            reasons
                .iter()
                .all(|r| {
                    matches!(
                        r.as_str(),
                        Some("proxy" | "upstream_timeout" | "upstream_unavailable" | "unknown")
                    )
                })
                .then_some(Duration::from_secs(1))
        }
        "API_UNAVAILABLE" => {
            (e.details["failureKind"] == "network").then_some(Duration::from_secs(1))
        }
        _ => None,
    }
}
fn cancelled() -> AppError {
    err(
        "CANCELLED_BEFORE_POST",
        "The operation was cancelled. The post was not sent.",
    )
}
fn post_error(mut e: AppError, stage: &str, attempted: bool, attempts: usize) -> AppError {
    if stage == "image_upload" && e.code == "API_UNAVAILABLE" {
        e.message = "The image upload request did not complete. The post was not sent.".into();
    }
    if !e.details.is_object() {
        e.details = json!({});
    }
    e.details["stage"] = json!(stage);
    e.details["postAttempted"] = json!(attempted);
    e.details["uploadAttempts"] = json!(attempts);
    if attempted
        && let Some(step) = crate::api_rejection::post_creation_next_step(&e.code, &e.details)
    {
        e.details["nextStep"] = json!(step);
    }
    e
}
async fn read_image(path: &str) -> Result<Image> {
    if !Path::new(path).is_absolute() {
        return Err(err("INVALID_IMAGE", "Specify absolute paths for images."));
    }
    let invalid = || err("INVALID_IMAGE", "The image file could not be read.");
    let file = tokio::fs::File::open(path).await.map_err(|_| invalid())?;
    let info = file.metadata().await.map_err(|_| invalid())?;
    if !info.is_file() || info.len() < 4 || info.len() > MAX_IMAGE as u64 {
        return Err(err(
            "INVALID_IMAGE",
            "Images must be PNG or JPEG files of 5 MiB or less.",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_IMAGE + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| invalid())?;
    if bytes.len() > MAX_IMAGE {
        return Err(err("INVALID_IMAGE", "Images must be 5 MiB or less."));
    }
    let png = bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let jpeg = bytes.starts_with(&[255, 216, 255]);
    if !png && !jpeg {
        return Err(err("INVALID_IMAGE", "Specify a PNG or JPEG image."));
    }
    Ok(Image {
        bytes,
        mime: if png { "image/png" } else { "image/jpeg" },
        name: if png { "image.png" } else { "image.jpg" },
    })
}
fn weighted_text(text: &str) -> Result<usize> {
    use twitter_text::extractor::{Extract, Extractor};
    let invalid = || {
        err(
            "INVALID_TEXT",
            "The post text must not be empty and must be within X's weighted length of 280.",
        )
    };
    if text.trim().is_empty() {
        return Err(invalid());
    }
    let normalized: String = text.nfc().collect();
    // Apply v3 weights directly. The Rust parser's emoji segmentation differs
    // for adjacent newer sequences; the bundled Unicode 17 data is authoritative.
    fn character_weight(c: char) -> usize {
        match c as u32 {
            0..=0x10ff | 0x2000..=0x200d | 0x2010..=0x201f | 0x2032..=0x2037 => 1,
            _ => 2,
        }
    }
    if normalized
        .chars()
        .any(|c| matches!(c, '\u{fffe}' | '\u{feff}' | '\u{ffff}'))
    {
        return Err(invalid());
    }
    let mut weight: usize = normalized.chars().map(character_weight).sum();
    let urls = Extractor::new().extract_urls_with_indices(&normalized);
    // Preserve the original UTF-16 regex exactly by mapping every code unit to
    // a scalar in plane 1. Every mapped scalar occupies four UTF-8 bytes.
    static EMOJI: OnceLock<Regex> = OnceLock::new();
    let matcher = EMOJI.get_or_init(|| {
        Regex::new(include_str!("client_emoji.txt")).expect("bundled emoji expression")
    });
    let units = normalized.encode_utf16().collect::<Vec<_>>();
    for url in &urls {
        let url_weight: usize = url.value.chars().map(character_weight).sum();
        weight = weight - url_weight + 23;
    }
    let mapped: String = units
        .iter()
        .map(|u| char::from_u32(0x10000 + *u as u32).unwrap())
        .collect();
    for m in matcher.find_iter(&mapped) {
        let start = m.start() / 4;
        let end = m.end() / 4;
        let (scalar_count, emoji_weight) = char::decode_utf16(units[start..end].iter().copied())
            .try_fold((0usize, 0usize), |(count, total), scalar| {
                let scalar = scalar.map_err(|_| invalid())?;
                Ok::<_, AppError>((count + 1, total + character_weight(scalar)))
            })?;
        if scalar_count == 1
            || urls
                .iter()
                .any(|u| start < (u.end as usize) && end > (u.start as usize))
        {
            continue;
        }
        weight = weight - emoji_weight + 2;
    }
    if weight > 280 {
        return Err(err(
            "INVALID_TEXT",
            "The post text exceeds X's weighted length of 280. The post was not sent. Shorten it by at least excessWeightedLength weighted characters (ask the user first if they wrote the exact text) and call post_tweet again. Each URL counts as 23, and Japanese, Chinese, and Korean characters and each emoji count as 2.",
        )
        .with_details(json!({
            "weightedLength": weight,
            "maxWeightedLength": 280,
            "excessWeightedLength": weight - 280
        })));
    }
    if weight == 0 {
        return Err(invalid());
    }
    Ok(weight)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weighted_unicode_and_urls() {
        for emoji in ["🫱🏻‍🫲🏽", "🫶🏻", "🙂‍↔️", "🐦‍🔥", "🚶‍➡️", "🇨🇶", "👨‍👩‍👧‍👦"]
        {
            assert_eq!(weighted_text(&emoji.repeat(140)).unwrap(), 280, "{emoji}");
            assert!(weighted_text(&emoji.repeat(141)).is_err(), "{emoji}");
        }
        assert_eq!(
            weighted_text(&format!("あ🫱🏻‍🫲🏽 https://example.com/{}", "a".repeat(400))).unwrap(),
            28
        );
        assert!(weighted_text("🫱🏻‍🫲🏽\u{fffe}").is_err());
        assert!(weighted_text(&format!("https://#️⃣.com/{}", "a".repeat(300))).is_err());
        assert_eq!(weighted_text(&"あ".repeat(140)).unwrap(), 280);
        assert_eq!(weighted_text("e\u{301}").unwrap(), 1);
    }
    #[test]
    fn over_limit_reports_weight_numbers_only_for_excess() {
        for text in ["あ".repeat(141), "😀".repeat(141)] {
            let e = weighted_text(&text).unwrap_err();
            assert_eq!(e.code, "INVALID_TEXT");
            assert_eq!(e.details["weightedLength"], 282);
            assert_eq!(e.details["maxWeightedLength"], 280);
            assert_eq!(e.details["excessWeightedLength"], 2);
        }
        let blank = weighted_text("   ").unwrap_err();
        assert_eq!(blank.code, "INVALID_TEXT");
        assert!(blank.details.get("weightedLength").is_none());
    }
    #[test]
    fn signed_cursor_boundaries() {
        let a = json!({"account":"a","userName":"Reader","apiKey":"secret"});
        let c = json!({"scope":"all","keyword":"some keyword","account":"a"});
        let cursor = search_encode("provider", &a, &c, "tweets").unwrap();
        // Nodeの標準cryptoで生成した既存wire形式の固定値。HMAC/base64更新でも保持する。
        assert_eq!(
            cursor,
            "plumivox-search.eyJ2ZXJzaW9uIjoxLCJraW5kIjoidHdlZXRzIiwiY29uZGl0aW9ucyI6eyJzY29wZSI6ImFsbCIsImtleXdvcmQiOiJzb21lIGtleXdvcmQiLCJhY2NvdW50IjoiYSJ9LCJpbm5lciI6InByb3ZpZGVyIn0.HzPcKxtrchqqcemcwGJAfD_MwCG-XxxmUlI9OvClv9I"
        );
        assert_eq!(
            search_decode(&json!(cursor), &a, &c, "tweets").unwrap(),
            "provider"
        );
        assert!(
            search_decode(
                &json!(cursor),
                &a,
                &json!({"scope":"all","keyword":"changed","account":"a"}),
                "tweets"
            )
            .is_err()
        );
        assert!(search_decode(&json!(cursor), &json!({"apiKey":"wrong"}), &c, "tweets").is_err());
        assert!(search_encode(&"a".repeat(MAX_PROVIDER + 1), &a, &c, "tweets").is_none());
        let p=page(&json!({"tweets":[{"id":"1"}],"has_next_page":true,"next_cursor":"a".repeat(MAX_PROVIDER+1)}),MAX_PROVIDER).unwrap();
        assert_eq!(p["tweets"][0]["id"], "1");
        assert_eq!(p["hasNextPage"], true);
        assert!(p["nextCursor"].is_null());
    }
    #[test]
    fn search_date_validation() {
        assert!(search_time(Some(&json!("2026-02-30T00:00:00Z"))).is_err());
        assert!(search_time(Some(&json!("2026-01-01T00:00:60Z"))).is_err());
        assert_eq!(
            search_time(Some(&json!("2026-01-01T09:00:00+09:00")))
                .unwrap()
                .unwrap()
                .1,
            "2026-01-01T00:00:00Z"
        );
    }
}
