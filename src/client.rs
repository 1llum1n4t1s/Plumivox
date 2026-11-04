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
    TLS_FIXTURE
        .set(trust_ca)
        .map_err(|_| err("INVALID_FIXTURE", "TLS検証モードは設定済みです。"))
}

fn err(code: &str, message: &str) -> AppError {
    AppError::new(code, message)
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
        return Err(err("INVALID_RESPONSE", "投稿データの形式が不正です。"));
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
        .ok_or_else(|| err("INVALID_RESPONSE", "投稿一覧を確認できません。"))?;
    let has = v["has_next_page"] == true;
    let cursor = string(&v["next_cursor"]);
    if has && cursor.is_empty() {
        return Err(err("INVALID_RESPONSE", "続きの取得情報が不足しています。"));
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
            "フィードのcursorが不正、または別アカウントのものです。",
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
            "検索のcursorが不正、または検索条件・アカウントが一致しません。",
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
            "日時はタイムゾーン付きISO 8601の秒精度で指定してください。",
        ));
    }
    let t = DateTime::parse_from_rfc3339(s).map_err(|_| {
        err(
            "INVALID_SEARCH",
            "検索日時が不正です。存在する日付と時刻を指定してください。",
        )
    })?;
    if s.get(17..19) == Some("60") {
        return Err(err(
            "INVALID_SEARCH",
            "検索日時が不正です。存在する日付と時刻を指定してください。",
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
        let operation = async {
            let base = url::Url::parse(&self.base).map_err(|_| {
                err(
                    "API_UNAVAILABLE",
                    "API通信が完了しませんでした。自動再試行はしていません。",
                )
            })?;
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
                return Err(err("API_UNAVAILABLE", "API通信先が不正です。"));
            }
            let mut url = base
                .join(&format!("/twitter/{endpoint}"))
                .map_err(|_| err("API_UNAVAILABLE", "API通信先が不正です。"))?;
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
            let client = self.http.as_ref().map_err(|_| {
                err(
                    "API_UNAVAILABLE",
                    "API通信が完了しませんでした。自動再試行はしていません。",
                )
            })?;
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
            let network = || {
                err(
                    "API_UNAVAILABLE",
                    "API通信が完了しませんでした。自動再試行はしていません。",
                )
                .with_details(if endpoint == "upload_media_v2" {
                    json!({"failureKind":"network"})
                } else {
                    json!({})
                })
            };
            let mut response = req.send().await.map_err(|_| network())?;
            if response.status().is_redirection() {
                return Err(network());
            }
            if !response.status().is_success() {
                return Err(err("API_HTTP_ERROR", "APIが要求を受け付けませんでした。")
                    .with_details(json!({"httpStatus":response.status().as_u16()})));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                    return Err(err(
                        "INVALID_RESPONSE",
                        "API応答のサイズが上限を超えました。",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            let v: Value = serde_json::from_str(&String::from_utf8_lossy(&bytes))
                .map_err(|_| err("INVALID_RESPONSE", "APIからJSON応答を確認できません。"))?;
            if !v.is_object() {
                return Err(err("INVALID_RESPONSE", "API応答の形式が不正です。"));
            }
            if v["status"] == "error" || v["status"] == "failed" || v["success"] == false {
                return Err(err("API_REJECTED", "APIが処理の失敗を返しました。"));
            }
            Ok(v)
        };
        let result = tokio::select! { biased;
            _=cancel.cancelled()=>Err(err("API_UNAVAILABLE","API通信が完了しませんでした。自動再試行はしていません。").with_details(if endpoint=="upload_media_v2"{json!({"failureKind":"cancelled"})}else{json!({})})),
            r=tokio::time::timeout(timeout,operation)=>r.unwrap_or_else(|_|Err(err("API_UNAVAILABLE","API通信が完了しませんでした。自動再試行はしていません。").with_details(if endpoint=="upload_media_v2"{json!({"failureKind":"timeout"})}else{json!({})})))
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
                    "投稿の成否は未確定です。投稿済みの可能性があります。自動再送はしていません。",
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
            return Err(err("INVALID_RESPONSE", "アカウントIDを確認できません。"));
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
            .ok_or_else(|| err("INVALID_INPUT", "投稿IDを指定してください。"))?
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
            .ok_or_else(|| err("INVALID_RESPONSE", "投稿一覧を確認できません。"))?
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
                "ログインCookieを確認できません。保存セッションは変更していません。",
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
                "maxAccountsは20から200までの整数にしてください。",
            ));
        }
        let cursor = string(&input["cursor"]);
        let following = string(&input["followingCursor"]);
        if !cursor.is_empty() && !following.is_empty() {
            return Err(err(
                "INVALID_CURSOR",
                "cursorとfollowingCursorは同時に指定できません。",
            ));
        }
        if following.starts_with(SEARCH_PREFIX) {
            return Err(err(
                "INVALID_CURSOR",
                "検索用のfollowingCursorはsearch_tweetsへ渡してください。",
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
                .ok_or_else(|| err("INVALID_RESPONSE", "フォロー先一覧を確認できません。"))?;
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
                .ok_or_else(|| err("INVALID_CURSOR", "フィードのcursorが不正です。"))?;
            let from = users
                .iter()
                .map(|v| format!("from:{}", string(v)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let query = format!(
                "({from}) include:nativeretweets{}",
                search_query.map(|s| format!(" {s}")).unwrap_or_default()
            );
            let p = page(
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
            for t in p["tweets"].as_array().unwrap() {
                let id = string(&t["id"]).to_owned();
                if let Some(index) = positions.get(&id) {
                    tweets[*index] = t.clone();
                } else {
                    positions.insert(id, tweets.len());
                    tweets.push(t.clone());
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
                "キーワードは空でない512文字以内の語句にしてください。引用符・バックスラッシュ・制御文字は使用できません。",
            ));
        }
        let scope = input.get("scope").map(string).unwrap_or("all");
        if !matches!(scope, "all" | "following") {
            return Err(err(
                "INVALID_SEARCH",
                "検索対象はallまたはfollowingを指定してください。",
            ));
        }
        if input.get("cursor").is_some() && input.get("followingCursor").is_some() {
            return Err(err(
                "INVALID_CURSOR",
                "cursorとfollowingCursorは同時に指定できません。",
            ));
        }
        if scope == "all"
            && (input.get("maxAccounts").is_some() || input.get("followingCursor").is_some())
        {
            return Err(err(
                "INVALID_SEARCH",
                "全体検索ではmaxAccountsとfollowingCursorを指定できません。",
            ));
        }
        if input.get("maxAccounts").is_some()
            && !input["maxAccounts"]
                .as_u64()
                .is_some_and(|n| (20..=200).contains(&n))
        {
            return Err(err(
                "INVALID_SEARCH",
                "maxAccountsは20から200までの整数にしてください。",
            ));
        }
        let start = search_time(input.get("startTime"))?;
        let end = search_time(input.get("endTime"))?;
        if let (Some(s), Some(e)) = (&start, &end) {
            if s.0 >= e.0 {
                return Err(err(
                    "INVALID_SEARCH",
                    "開始日時は終了日時より前にしてください。",
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
                    "フォロー一覧の続き情報が不足しています。",
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
                    .ok_or_else(|| err("INVALID_IMAGE", "画像には絶対パスを指定してください。"))?;
                if images.len() > 4 {
                    return Err(err("INVALID_IMAGE", "画像は4枚以内にしてください。"));
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
    pub async fn send_post(
        &self,
        a: &Value,
        prepared: PreparedPost,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let mut stage = "image_upload";
        let mut attempted = false;
        let mut attempts = 0;
        let result=async{
            let mut ids=Vec::new();
            for image in prepared.files{
                let deadline=Instant::now()+self.image_timeout;
                let mut uploaded=None;
                for attempt in 0..2{
                    if cancel.is_cancelled(){return Err(cancelled());}
                    let remaining=deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero(){return Err(err("API_UNAVAILABLE","API通信が完了しませんでした。"));}
                    let part=reqwest::multipart::Part::bytes(image.bytes.clone()).file_name(image.name).mime_str(image.mime).map_err(|_|err("INVALID_IMAGE","画像ファイルを読み込めません。"))?;
                    let form=reqwest::multipart::Form::new().text("login_cookies",string(&a["loginCookies"]).to_owned()).text("proxy",string(&a["proxy"]).to_owned()).part("file",part);
                    attempts+=1;
                    match self.request(a,"upload_media_v2",None,None,Some(form),&cancel,remaining).await{
                        Ok(v)=>{uploaded=Some(v);break;}
                        Err(e)=>{
                            if cancel.is_cancelled(){return Err(cancelled());}
                            if attempt!=0 || e.code!="API_HTTP_ERROR" || !matches!(e.details["httpStatus"].as_u64(),Some(502|503|504)) || deadline.saturating_duration_since(Instant::now())<=Duration::from_millis(250){return Err(e);}
                            tokio::select!{_=cancel.cancelled()=>return Err(cancelled()),_=tokio::time::sleep(Duration::from_millis(250))=>{}}
                        }
                    }
                }
                let uploaded=uploaded.ok_or_else(||err("UPLOAD_FAILED","画像アップロードの成功を確認できません。投稿は送信していません。"))?;
                if uploaded["status"]!="success" || !numeric(&uploaded["media_id"]){return Err(err("UPLOAD_FAILED","画像アップロードの成功を確認できません。投稿は送信していません。"));}
                ids.push(uploaded["media_id"].clone());
            }
            if cancel.is_cancelled(){return Err(cancelled());}
            stage="post_creation";attempted=true;
            let mut body=json!({"login_cookies":a["loginCookies"],"proxy":a["proxy"],"tweet_text":prepared.text});
            if !ids.is_empty(){body["media_ids"]=json!(ids);}
            let v=self.request(a,"create_tweet_v2",None,Some(&body),None,&cancel,self.timeout).await?;
            if v["status"]!="success" || !numeric(&v["tweet_id"]){return Err(err("POST_UNCERTAIN","投稿IDを確認できないため成否は未確定です。自動再送はしていません。"));}
            Ok(json!({"account":a["account"],"tweetId":v["tweet_id"],"url":format!("https://x.com/i/status/{}",string(&v["tweet_id"])),"weightedLength":prepared.weighted}))
        }.await;
        result.map_err(|e| post_error(e, stage, attempted, attempts))
    }
}
fn cancelled() -> AppError {
    err(
        "CANCELLED_BEFORE_POST",
        "処理は中止されました。投稿は送信していません。",
    )
}
fn post_error(mut e: AppError, stage: &str, attempted: bool, attempts: usize) -> AppError {
    if stage == "image_upload" && e.code == "API_UNAVAILABLE" {
        e.message = "画像アップロードの通信が完了しませんでした。投稿は送信していません。".into();
    }
    if !e.details.is_object() {
        e.details = json!({});
    }
    e.details["stage"] = json!(stage);
    e.details["postAttempted"] = json!(attempted);
    e.details["uploadAttempts"] = json!(attempts);
    e
}
async fn read_image(path: &str) -> Result<Image> {
    if !Path::new(path).is_absolute() {
        return Err(err("INVALID_IMAGE", "画像には絶対パスを指定してください。"));
    }
    let invalid = || err("INVALID_IMAGE", "画像ファイルを読み込めません。");
    let file = tokio::fs::File::open(path).await.map_err(|_| invalid())?;
    let info = file.metadata().await.map_err(|_| invalid())?;
    if !info.is_file() || info.len() < 4 || info.len() > MAX_IMAGE as u64 {
        return Err(err(
            "INVALID_IMAGE",
            "画像は5 MiB以下のPNGまたはJPEGファイルにしてください。",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_IMAGE + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| invalid())?;
    if bytes.len() > MAX_IMAGE {
        return Err(err("INVALID_IMAGE", "画像は5 MiB以下にしてください。"));
    }
    let png = bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let jpeg = bytes.starts_with(&[255, 216, 255]);
    if !png && !jpeg {
        return Err(err(
            "INVALID_IMAGE",
            "PNGまたはJPEG画像を指定してください。",
        ));
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
            "投稿文は空でなく、Xの加重文字数280以内にしてください。",
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
    if !(1..=280).contains(&weight) {
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
