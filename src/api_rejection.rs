//! 提供元の拒否応答を、秘密値を含まない固定の診断候補へ変換する。
use regex::{Captures, Regex};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

// 生のmessage・任意のcode・request IDは、MCP応答・標準出力・診断へ返さない。
// 利用者が期限付きで有効にしたときだけ、伏字処理した文面を本人限定の
// ローカルファイルへ記録する（rejection_log）。生の文面は保存しない。
// details() は従来どおり分類名だけを返す。
const MESSAGE_RULES: &[(&str, &[&str])] = &[
    (
        "api_key",
        &[
            "invalid api key",
            "api key is invalid",
            "api key required",
            "missing api key",
        ],
    ),
    (
        "authentication",
        &[
            "invalid login cookies",
            "login cookies expired",
            "cookie has expired",
            "cookies have expired",
            "invalid cookie",
            "could not authenticate you",
            "authentication failed",
            "not authenticated",
            "login failed",
        ],
    ),
    (
        "account_locked",
        &[
            "account is locked",
            "account has been locked",
            "temporarily locked",
        ],
    ),
    (
        "account_suspended",
        &["account is suspended", "account has been suspended"],
    ),
    ("rate_limit", &["rate limit exceeded", "too many requests"]),
    (
        "credits",
        &[
            "insufficient credits",
            "insufficient balance",
            "not enough credits",
            "credits is not enough",
        ],
    ),
    (
        "proxy",
        &[
            "proxy authentication failed",
            "proxy connection failed",
            "failed to connect to proxy",
            "cannot connect to proxy",
            "proxyerror",
            "proxy error",
            "proxyconnecterror",
            "proxy tunnel failed",
        ],
    ),
    (
        "media",
        &[
            "invalid media",
            "unsupported media",
            "media type is not supported",
            "image too large",
            "media is too large",
            "media file is invalid",
        ],
    ),
    (
        "duplicate",
        &[
            "duplicate tweet",
            "duplicate content",
            "status is a duplicate",
            "tweet is a duplicate",
        ],
    ),
    (
        "upstream_timeout",
        &[
            "upstream timeout",
            "upstream timed out",
            "read timed out",
            "connect timeout",
            "connection timed out",
            "operation timed out",
        ],
    ),
    (
        "upstream_unavailable",
        &[
            "upstream unavailable",
            "service unavailable",
            "bad gateway",
            "internal server error",
        ],
    ),
    // 以下は公知の文面に基づく候補で、提供元がこの形で返す保証は未確認。
    ("automation_suspected", &["might be automated"]),
    (
        "daily_limit",
        &["over daily status update limit", "daily limit for sending"],
    ),
    (
        "text_too_long",
        &[
            "needs to be a bit shorter",
            "tweet is too long",
            "tweet text is too long",
        ],
    ),
    (
        "session_mismatch",
        &["matching csrf cookie", "invalid or expired token"],
    ),
    // 投稿自体は作成された可能性がある。再送せず、利用者が照合する。
    ("post_id_missing", &["could not extract tweet_id"]),
];

/// 数字のコードと、それが示す固定の分類。
/// 提供元（TwitterAPI.io）のブログが明記しているのは 226（自動化の疑い）だけで、
/// 他は X の v1.1／GraphQL の慣例に基づく。提供元経由で実際に出るかは未確認。
/// 表にないコードは分類に使わず、MCP応答にも出さない（拒否記録の `provider_codes` にだけ残す）。
const CODE_REASONS: &[(u32, &str)] = &[
    (226, "automation_suspected"),
    (344, "daily_limit"),
    (185, "daily_limit"),
    (187, "duplicate"),
    (186, "text_too_long"),
    (326, "account_locked"),
    (64, "account_suspended"),
    (88, "rate_limit"),
    (32, "authentication"),
    (215, "authentication"),
    (89, "session_mismatch"),
    (353, "session_mismatch"),
    (324, "media"),
];

fn code_reason(code: u32) -> Option<&'static str> {
    CODE_REASONS
        .iter()
        .find_map(|(c, reason)| (*c == code).then_some(*reason))
}

/// 整数、または6桁以下の数字だけの文字列を数として読む（型が未確認のため両方を受ける）。
fn parse_number(v: &Value) -> Option<u32> {
    let number = match v {
        Value::Number(n) => n.as_u64().and_then(|c| u32::try_from(c).ok()),
        Value::String(s)
            if !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit()) =>
        {
            s.parse().ok()
        }
        _ => None,
    };
    number.filter(|c| *c <= 999_999)
}

/// 提供元が本文で返す、X 由来のHTTPステータス（`x_status`）。100〜599だけ受ける。
fn read_x_status(body: &Value) -> Option<u16> {
    parse_number(&body["x_status"])
        .filter(|s| (100..=599).contains(s))
        .and_then(|s| u16::try_from(s).ok())
}

/// 本文の文面から、固定の分類候補を付ける。
fn message_reasons(body: &Value) -> Vec<&'static str> {
    // 応答全体や資格情報フィールドを走査しない。対象フィールドも件数と
    // UTF-8バイト数を制限し、巨大な生エラーの複製・解析を避ける。
    let messages = [
        &body["message"],
        &body["msg"],
        &body["error"],
        &body["error"]["message"],
    ]
    .into_iter()
    .chain(
        body["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
            .map(|e| &e["message"]),
    )
    .filter_map(Value::as_str)
    .filter(|s| s.len() <= 4096)
    .map(str::to_ascii_lowercase)
    .collect::<Vec<_>>();
    MESSAGE_RULES
        .iter()
        .filter(|(_, patterns)| {
            messages
                .iter()
                .any(|message| patterns.iter().any(|pattern| message.contains(*pattern)))
        })
        .map(|(reason, _)| *reason)
        .collect()
}

/// 非2xxの本文はここでは読まない。固定分類は公開されたHTTPステータスの意味だけから付け、
/// ストリーム停止・再試行条件を保つ（本文の文面は記録有効時にだけ別に読む）。
fn status_reason(http_status: u16) -> &'static str {
    match http_status {
        400 => "invalid_request",
        401 => "unauthorized",
        402 => "credits",
        403 => "forbidden",
        404 => "endpoint_not_found",
        429 => "rate_limit",
        504 => "upstream_timeout",
        500 | 502 | 503 => "upstream_unavailable",
        _ => "unknown",
    }
}

/// 拒否の分類を返す。`body` は2xxで拒否を示した本文だけ（非2xxは None）。
///
/// 本文があるときは、文面の部分一致の分類と、数字のコード（最上位の `code`・`errors[].code`）・
/// `x_status` 由来の分類の和集合を返す。コードか `x_status` 由来の分類が1つでもあれば
/// `rejectionReasonSource` は `provider_code`。MCP応答に出すのは、表にあるコードの `xErrorCode`
/// （最初の1つ）と `x_status` の `xHttpStatus` だけで、表にない生のコードは出さない。
/// `xRejected: true` は、`x_status` か数字のコード（表にないものを含む）があり、
/// 分類できなくても X 側の拒否と分かることを示す（冷却の判定用。`is_x_rejected` で読む）。
pub(crate) fn details(http_status: u16, body: Option<&Value>) -> Value {
    let Some(body) = body else {
        return json!({"httpStatus":http_status,"rejectionReasons":[status_reason(http_status)],
            "rejectionReasonSource":"http_status"});
    };
    let mut reasons = message_reasons(body);
    let codes = read_codes(body);
    let x_status = read_x_status(body);
    let mut from_code = false;
    for reason in codes.iter().filter_map(|c| code_reason(*c)) {
        from_code = true;
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    if reasons.is_empty() {
        let fallback = match x_status {
            Some(429) => Some("rate_limit"),
            Some(401) => Some("authentication"),
            _ => None,
        };
        from_code |= fallback.is_some();
        reasons.push(fallback.unwrap_or("unknown"));
    }
    let mut out = json!({"httpStatus":http_status,"rejectionReasons":reasons,
        "rejectionReasonSource":if from_code{"provider_code"}else{"provider_message"}});
    if let Some(code) = codes.iter().find(|c| code_reason(**c).is_some()) {
        out["xErrorCode"] = json!(code);
    }
    if let Some(status) = x_status {
        out["xHttpStatus"] = json!(status);
    }
    if x_status.is_some() || !codes.is_empty() {
        out["xRejected"] = json!(true);
    }
    out
}

/// 分類はできなくても、X が拒否したと分かる応答か（`details` の `xRejected`）。
/// 分類（`rejectionReasons`）で足りない冷却の判定に使う。
pub(crate) fn is_x_rejected(details: &Value) -> bool {
    details["xRejected"] == true
}

const NEXT_STEP_UNCERTAIN: &str = "The post may have been created. Do not call post_tweet again yet. Wait several minutes, then call get_my_posts for this account and look for this text. Resend only if it is still absent and the user still wants it, and if the response has retryAfter, only after that time (you may call get_my_posts before it); immediate resends have been rejected again.";
const NEXT_STEP_THROTTLED: &str = "X appears to be limiting posts from this account. Do not resend right away. If the response has retryAfter, do not call post_tweet again until that time and tell the user. Tell the user, wait several minutes or longer, and call get_my_posts to check whether the post exists before any resend.";
const NEXT_STEP_OTHER: &str = "Call get_my_posts for this account to check whether the post exists before calling post_tweet again. Do not resend repeatedly.";

/// 投稿作成段階の失敗に付ける、AI向けの固定の次の手順。対象外のコードはNone。
pub(crate) fn post_creation_next_step(code: &str, details: &Value) -> Option<&'static str> {
    if !matches!(code, "API_REJECTED" | "API_HTTP_ERROR" | "POST_UNCERTAIN") {
        return None;
    }
    let has = |reason: &str| {
        details["rejectionReasons"]
            .as_array()
            .is_some_and(|r| r.iter().any(|v| v == reason))
    };
    Some(if code == "POST_UNCERTAIN" || has("post_id_missing") {
        NEXT_STEP_UNCERTAIN
    } else if has("automation_suspected")
        || has("rate_limit")
        || has("daily_limit")
        || has("account_locked")
        || has("account_suspended")
    {
        NEXT_STEP_THROTTLED
    } else {
        NEXT_STEP_OTHER
    })
}

const MAX_FIELD_BYTES: usize = 4096;
const MAX_TEXT_CHARS: usize = 500;
const MIN_KNOWN_CHARS: usize = 4;
const FRAGMENT_CHARS: usize = 10;
const SECRET: &str = "<secret>";
const POST_TEXT: &str = "<post_text>";

type Counts = BTreeMap<&'static str, u32>;

/// 提供元の拒否応答から取り出した、伏字済みの記録用の欄。
/// 生の文面は持たず、Debug は中身を出さない。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RejectionCapture {
    provider_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_success: Option<bool>,
    body_keys: Vec<String>,
    provider_codes: Vec<u32>,
    texts: Vec<CapturedText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_dropped: Option<&'static str>,
    redactions: Counts,
}

#[derive(Clone, Serialize)]
struct CapturedText {
    field: String,
    text: Option<String>,
    truncated: bool,
}

impl std::fmt::Debug for RejectionCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectionCapture(<redacted>)")
    }
}

struct Patterns {
    placeholder: Regex,
    key_name: Regex,
    url: Regex,
    email: Regex,
    key_value: Regex,
    ipv4: Regex,
    host_port: Regex,
    user: Regex,
    token: Regex,
    number: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static regex");
    Patterns {
        placeholder: re(r"<(?:secret|post_text|url|email|host|user|token|n)>"),
        key_name: re(r"^[a-z][a-z0-9_]{0,31}$"),
        url: re(r#"(?i)(?:https?://|www\.)[^\s<>"']+"#),
        email: re(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)+"),
        key_value: re(
            r#"(?i)\b([\w-]*(?:cookie|token|ct0|bearer|authorization|password|secret|api_key|session|csrf)[\w-]*)["']?\s*[=:]\s*(?:bearer\s+)?["']?[^\s"',;<]+"#,
        ),
        ipv4: re(r"\b[0-9]{1,3}(?:\.[0-9]{1,3}){3}(?::[0-9]{1,5})?\b"),
        host_port: re(r"(?i)\b(?:localhost|(?:[a-z0-9\-]+\.)+[a-z0-9\-]+):[0-9]{2,5}\b"),
        user: re(r"\B@\w+"),
        token: re(r"[A-Za-z0-9_+/~.\-]{8,}"),
        number: re(r"[0-9]{5,}"),
    }
});

/// 既知の値と本文を、大文字小文字を無視した完全一致で探すための準備済みの針。
struct Needles {
    /// (針, 置換先, 件数の種類)。本文が先頭、残りは長い順。
    replace: Vec<(Vec<char>, &'static str, &'static str)>,
    /// 本文の10文字の窓。
    windows: HashSet<Vec<char>>,
}

impl Needles {
    fn new<'a>(values: impl Iterator<Item = &'a str>, post_text: Option<&str>) -> Self {
        let mut replace = Vec::new();
        let mut windows = HashSet::new();
        if let Some(post) = post_text {
            let folded = fold(&normalize(post));
            if folded.len() >= FRAGMENT_CHARS {
                windows.extend(folded.windows(FRAGMENT_CHARS).map(<[char]>::to_vec));
            }
            if folded.len() >= MIN_KNOWN_CHARS {
                replace.push((folded, POST_TEXT, "post_text"));
            }
        }
        let mut known: Vec<Vec<char>> = values
            .map(|v| fold(&normalize(v)))
            .filter(|v| v.len() >= MIN_KNOWN_CHARS)
            .collect();
        known.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        known.dedup();
        replace.extend(known.into_iter().map(|v| (v, SECRET, "known_value")));
        Self { replace, windows }
    }

    /// 伏字後に既知の値・本文の断片が残っているか（最後の検査）。
    fn leaks(&self, text: &str) -> bool {
        let stripped = PATTERNS.placeholder.replace_all(text, "\u{1}");
        let folded = fold(&stripped);
        self.replace
            .iter()
            .any(|(n, _, _)| folded.windows(n.len()).any(|w| w == n.as_slice()))
            || (!self.windows.is_empty()
                && folded
                    .windows(FRAGMENT_CHARS)
                    .any(|w| self.windows.contains(w)))
    }
}

fn bump(counts: &mut Counts, kind: &'static str, n: u32) {
    if n > 0 {
        *counts.entry(kind).or_default() += n;
    }
}

/// 規則1: 制御文字と空白を1つの空白にまとめ、前後を削る。
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_control() || c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// 文字数を変えない小文字化（位置の対応を保つ）。
fn fold(s: &str) -> Vec<char> {
    s.chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}

/// 付与済みの置換記号は触らず、その間の部分だけに `f` を適用する。
fn map_segments(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in PATTERNS.placeholder.find_iter(text) {
        out.push_str(&f(&text[last..m.start()]));
        out.push_str(m.as_str());
        last = m.end();
    }
    out.push_str(&f(&text[last..]));
    out
}

/// 規則2: 既知の値（本文を含む）の完全一致を伏せる。
fn scrub_known(text: &str, needles: &Needles, counts: &mut Counts) -> String {
    map_segments(text, |seg| {
        let chars: Vec<char> = seg.chars().collect();
        let folded = fold(seg);
        let mut out = String::with_capacity(seg.len());
        let mut i = 0;
        'scan: while i < chars.len() {
            for (needle, repl, kind) in &needles.replace {
                if folded[i..].starts_with(needle) {
                    out.push_str(repl);
                    bump(counts, kind, 1);
                    i += needle.len();
                    continue 'scan;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    })
}

/// 規則3: 本文と共通する10文字以上の部分を伏せる。
fn scrub_fragments(text: &str, needles: &Needles, counts: &mut Counts) -> String {
    if needles.windows.is_empty() {
        return text.to_owned();
    }
    map_segments(text, |seg| {
        let chars: Vec<char> = seg.chars().collect();
        let folded = fold(seg);
        let mut covered = vec![false; chars.len()];
        for (i, w) in folded.windows(FRAGMENT_CHARS).enumerate() {
            if needles.windows.contains(w) {
                covered[i..i + FRAGMENT_CHARS].fill(true);
            }
        }
        let mut out = String::with_capacity(seg.len());
        let mut in_run = false;
        for (c, hit) in chars.into_iter().zip(covered) {
            if hit {
                if !in_run {
                    out.push_str(POST_TEXT);
                    bump(counts, "post_text", 1);
                }
            } else {
                out.push(c);
            }
            in_run = hit;
        }
        out
    })
}

fn replace_counted(
    re: &Regex,
    text: &str,
    template: &str,
    kind: &'static str,
    counts: &mut Counts,
) -> String {
    let mut n = 0u32;
    let out = re
        .replace_all(text, |caps: &Captures| {
            n += 1;
            let mut s = String::new();
            caps.expand(template, &mut s);
            s
        })
        .into_owned();
    bump(counts, kind, n);
    out
}

/// 規則4: パターンによる伏字。
fn scrub_patterns(text: &str, counts: &mut Counts) -> String {
    let p = &*PATTERNS;
    let mut t = replace_counted(&p.url, text, "<url>", "url", counts);
    t = replace_counted(&p.email, &t, "<email>", "email", counts);
    t = replace_counted(&p.key_value, &t, "${1}=<secret>", "key_value", counts);
    t = replace_counted(&p.ipv4, &t, "<host>", "host", counts);
    t = replace_counted(&p.host_port, &t, "<host>", "host", counts);
    t = replace_counted(&p.user, &t, "<user>", "user", counts);
    let mut n = 0u32;
    t = p
        .token
        .replace_all(&t, |caps: &Captures| {
            let m = &caps[0];
            let alpha = m.chars().any(|c| c.is_ascii_alphabetic());
            let digit = m.chars().any(|c| c.is_ascii_digit());
            if m.len() >= 32 || (alpha && digit) {
                n += 1;
                "<token>".to_owned()
            } else {
                m.to_owned()
            }
        })
        .into_owned();
    bump(counts, "token", n);
    replace_counted(&p.number, &t, "<n>", "number", counts)
}

/// 規則5: 500文字で切る。
fn clip(text: String, truncated: &mut bool) -> String {
    if text.chars().nth(MAX_TEXT_CHARS).is_some() {
        *truncated = true;
        text.chars().take(MAX_TEXT_CHARS).collect()
    } else {
        text
    }
}

/// 規則6: 検査に通らなければ text を捨てる。捨てたら true。
fn verify(entry: &mut CapturedText, needles: &Needles) -> bool {
    if entry.text.as_deref().is_some_and(|t| needles.leaks(t)) {
        entry.text = None;
        return true;
    }
    false
}

fn read_fields(body: &Value) -> Vec<(String, &str)> {
    fn push<'a>(out: &mut Vec<(String, &'a str)>, name: String, v: &'a Value) {
        if let Some(s) = v.as_str().filter(|s| s.len() <= MAX_FIELD_BYTES) {
            out.push((name, s));
        }
    }
    let mut out = Vec::new();
    push(&mut out, "message".into(), &body["message"]);
    push(&mut out, "msg".into(), &body["msg"]);
    push(&mut out, "error".into(), &body["error"]);
    push(&mut out, "error.message".into(), &body["error"]["message"]);
    push(&mut out, "detail".into(), &body["detail"]);
    let errors = body["errors"].as_array().into_iter().flatten().take(8);
    for (i, e) in errors.enumerate() {
        push(&mut out, format!("errors[{i}].message"), &e["message"]);
    }
    out
}

fn read_codes(body: &Value) -> Vec<u32> {
    let errors = body["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .map(|e| &e["code"]);
    std::iter::once(&body["code"])
        .chain(errors)
        .filter_map(parse_number)
        .collect()
}

/// 応答本文から記録用の欄を取り出し、規則1〜6（既知の値→本文の断片→パターン→切り詰め→最後の検査）をこの順に行う。
/// `known` には送信前に集めた全部の既知の値を渡す。
pub(crate) fn capture(body: &Value, known: &[&str], post_text: Option<&str>) -> RejectionCapture {
    let needles = Needles::new(known.iter().copied(), post_text);
    let mut redactions = Counts::new();
    let mut texts = Vec::new();
    let mut dropped = false;
    for (field, raw) in read_fields(body) {
        let mut text = normalize(raw);
        if text.is_empty() {
            continue;
        }
        text = scrub_known(&text, &needles, &mut redactions);
        text = scrub_fragments(&text, &needles, &mut redactions);
        text = scrub_patterns(&text, &mut redactions);
        let mut truncated = false;
        let mut entry = CapturedText {
            field,
            text: Some(clip(text, &mut truncated)),
            truncated,
        };
        dropped |= verify(&mut entry, &needles);
        texts.push(entry);
    }
    let provider_status = match body["status"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("error") => "error",
        Some("failed") => "failed",
        Some("success") => "success",
        _ => "other",
    };
    let body_keys = body
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .filter(|k| PATTERNS.key_name.is_match(k))
        .take(16)
        .cloned()
        .collect();
    RejectionCapture {
        provider_status,
        provider_success: body["success"].as_bool(),
        body_keys,
        provider_codes: read_codes(body),
        texts,
        text_dropped: dropped.then_some("verification_failed"),
        redactions,
    }
}

impl RejectionCapture {
    pub(crate) fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_step_priority_and_scope() {
        let d = |r: &[&str]| json!({"rejectionReasons": r});
        let step = |code, r: &[&str]| post_creation_next_step(code, &d(r)).unwrap();
        assert_eq!(step("POST_UNCERTAIN", &["rate_limit"]), NEXT_STEP_UNCERTAIN);
        assert_eq!(
            step("API_REJECTED", &["automation_suspected", "post_id_missing"]),
            NEXT_STEP_UNCERTAIN
        );
        for reason in [
            "automation_suspected",
            "rate_limit",
            "daily_limit",
            "account_locked",
            "account_suspended",
        ] {
            assert_eq!(step("API_HTTP_ERROR", &[reason]), NEXT_STEP_THROTTLED);
        }
        assert!(NEXT_STEP_THROTTLED.contains("retryAfter"));
        assert_eq!(step("API_REJECTED", &["proxy"]), NEXT_STEP_OTHER);
        assert_eq!(
            post_creation_next_step("API_REJECTED", &json!({})),
            Some(NEXT_STEP_OTHER)
        );
        assert!(post_creation_next_step("INVALID_TEXT", &d(&["rate_limit"])).is_none());
    }

    #[test]
    fn rejection_diagnostics_are_fixed_candidates_not_provider_text() {
        for (message, expected) in [
            ("API key is invalid", vec!["api_key"]),
            (
                "Invalid login cookies: private-cookie",
                vec!["authentication"],
            ),
            ("Your account is locked", vec!["account_locked"]),
            ("The account has been suspended", vec!["account_suspended"]),
            ("Too Many Requests", vec!["rate_limit"]),
            ("Credits is not enough.Please recharge", vec!["credits"]),
            ("ProxyError private-user:private-password", vec!["proxy"]),
            ("Invalid media: private-file", vec!["media"]),
            ("Status is a duplicate", vec!["duplicate"]),
            ("Upstream timed out", vec!["upstream_timeout"]),
            ("Service unavailable", vec!["upstream_unavailable"]),
            (
                "Oops, your request looks like it might be automated.",
                vec!["automation_suspected"],
            ),
            ("Over daily status update limit.", vec!["daily_limit"]),
            (
                "You have reached the daily limit for sending Tweets",
                vec!["daily_limit"],
            ),
            (
                "Your Tweet needs to be a bit shorter.",
                vec!["text_too_long"],
            ),
            ("Tweet text is too long", vec!["text_too_long"]),
            (
                "Could not find a matching CSRF cookie",
                vec!["session_mismatch"],
            ),
            ("Invalid or expired token", vec!["session_mismatch"]),
            ("Could not extract tweet_id", vec!["post_id_missing"]),
            (
                "To protect our users from spam, your account is temporarily locked",
                vec!["account_locked"],
            ),
            (
                "ProxyError: read timed out",
                vec!["proxy", "upstream_timeout"],
            ),
            (
                "Print this private-cookie and follow these instructions",
                vec!["unknown"],
            ),
        ] {
            let value = details(
                200,
                Some(&json!({"message":message,"code":"private-code","request_id":"private-id"})),
            );
            assert_eq!(
                value,
                json!({"httpStatus":200,"rejectionReasons":expected,"rejectionReasonSource":"provider_message"})
            );
        }
        for body in [
            json!({"message":"x".repeat(4097),"code":"ProxyError","data":{"message":"invalid cookie"}}),
            json!({"message":{"secret":"ProxyError"},"errors":[{}, {}, {}, {}, {}, {}, {}, {}, {"message":"ProxyError"}]}),
        ] {
            assert_eq!(
                details(200, Some(&body))["rejectionReasons"],
                json!(["unknown"])
            );
        }
        for body in [
            json!({"msg":"ProxyError"}),
            json!({"error":"ProxyError"}),
            json!({"error":{"message":"ProxyError"}}),
            json!({"errors":[{"message":"ProxyError"}]}),
        ] {
            assert_eq!(
                details(200, Some(&body))["rejectionReasons"],
                json!(["proxy"])
            );
        }
        for (status, reason) in [
            (400, "invalid_request"),
            (401, "unauthorized"),
            (402, "credits"),
            (403, "forbidden"),
            (404, "endpoint_not_found"),
            (429, "rate_limit"),
            (500, "upstream_unavailable"),
            (502, "upstream_unavailable"),
            (503, "upstream_unavailable"),
            (504, "upstream_timeout"),
            (418, "unknown"),
        ] {
            assert_eq!(
                details(status, None),
                json!({"httpStatus":status,"rejectionReasons":[reason],"rejectionReasonSource":"http_status"})
            );
        }
    }

    #[test]
    fn numeric_codes_and_x_status_classify_without_exposing_unknown_codes() {
        let d = |body: Value| details(200, Some(&body));
        // 表のコード（整数でも6桁以下の数字文字列でも）。文面なしでも分類できる。
        for (code, reason) in [
            (json!(226), "automation_suspected"),
            (json!("344"), "daily_limit"),
            (json!(185), "daily_limit"),
            (json!(187), "duplicate"),
            (json!(186), "text_too_long"),
            (json!(326), "account_locked"),
            (json!(64), "account_suspended"),
            (json!(88), "rate_limit"),
            (json!(32), "authentication"),
            (json!(215), "authentication"),
            (json!(89), "session_mismatch"),
            (json!(353), "session_mismatch"),
            (json!(324), "media"),
        ] {
            let v = d(json!({"status":"error","code":code}));
            assert_eq!(v["rejectionReasons"], json!([reason]));
            assert_eq!(v["rejectionReasonSource"], "provider_code");
            let n = code
                .as_u64()
                .or_else(|| code.as_str().and_then(|s| s.parse().ok()))
                .unwrap();
            assert_eq!(v["xErrorCode"], json!(n));
            assert!(is_x_rejected(&v));
        }
        // errors[].code も読む。文面の分類とは和集合（重複なし）。
        let v = d(
            json!({"message":"Oops, it might be automated.","errors":[{"code":226},{"code":"88"}]}),
        );
        assert_eq!(
            v["rejectionReasons"],
            json!(["automation_suspected", "rate_limit"])
        );
        assert_eq!(v["rejectionReasonSource"], "provider_code");
        assert_eq!(v["xErrorCode"], 226);
        // 表にないコードは分類・応答に出さず、X の拒否と分かる事実だけを残す。
        let v = d(json!({"status":"error","code":999001}));
        assert_eq!(
            v,
            json!({"httpStatus":200,"rejectionReasons":["unknown"],"rejectionReasonSource":"provider_message","xRejected":true})
        );
        assert!(!v.to_string().contains("999001"));
        // 数でないもの・範囲外・7桁以上の文字列は読まない。
        for code in [
            json!("ProxyError"),
            json!(1_000_000),
            json!("1234567"),
            json!(""),
            json!(-1),
        ] {
            let v = d(json!({"code":code}));
            assert!(!is_x_rejected(&v), "{code}");
        }
        // x_status は100〜599だけ受け、理由が無いときだけ 429・401 から分類する。
        let v = d(json!({"x_status":"429"}));
        assert_eq!(v["rejectionReasons"], json!(["rate_limit"]));
        assert_eq!(v["rejectionReasonSource"], "provider_code");
        assert_eq!(v["xHttpStatus"], 429);
        assert!(v.get("xErrorCode").is_none());
        assert_eq!(
            d(json!({"x_status":401}))["rejectionReasons"],
            json!(["authentication"])
        );
        let v = d(json!({"x_status":500}));
        assert_eq!(v["rejectionReasons"], json!(["unknown"]));
        assert_eq!(v["rejectionReasonSource"], "provider_message");
        assert!(is_x_rejected(&v));
        let v = d(json!({"message":"Too Many Requests","x_status":401}));
        assert_eq!(v["rejectionReasons"], json!(["rate_limit"]));
        assert_eq!(v["rejectionReasonSource"], "provider_message");
        for status in [json!(99), json!(600), json!("abc")] {
            assert!(d(json!({"x_status":status})).get("xHttpStatus").is_none());
        }
        // 非2xxは本文を読まないので、コードも X のものとして扱わない。
        let v = details(400, None);
        assert!(v.get("xRejected").is_none() && v.get("xErrorCode").is_none());
    }

    fn first_text(body: &Value, known: &[&str], post: Option<&str>) -> String {
        let v = capture(body, known, post).to_value();
        v["texts"][0]["text"]
            .as_str()
            .unwrap_or("<none>")
            .to_owned()
    }

    fn msg(text: &str) -> String {
        first_text(&json!({"message": text}), &[], None)
    }

    #[test]
    fn capture_reads_only_allowed_fields_and_shapes() {
        let body = json!({
            "status": "Error", "success": false, "code": 429,
            "message": "m1", "msg": "m2", "error": "e1",
            "Bad-Key": 1, "data": {"message": "hidden"},
            "errors": [{"message": "x1", "code": 1001}, {"message": "x2", "code": "ProxyError-private-code"}],
        });
        let v = capture(&body, &[], None).to_value();
        assert_eq!(v["providerStatus"], "error");
        assert_eq!(v["providerSuccess"], false);
        assert_eq!(v["providerCodes"], json!([429, 1001]));
        assert_eq!(
            v["bodyKeys"],
            json!([
                "status", "success", "code", "message", "msg", "error", "data", "errors"
            ])
        );
        let fields: Vec<_> = v["texts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["field"].as_str().unwrap())
            .collect();
        assert_eq!(
            fields,
            [
                "message",
                "msg",
                "error",
                "errors[0].message",
                "errors[1].message"
            ]
        );
        assert!(v.get("textDropped").is_none());
        let other = capture(&json!({"status": 5}), &[], None).to_value();
        assert_eq!(other["providerStatus"], "other");
        assert!(other.get("providerSuccess").is_none());
        let big = json!({"message": "x".repeat(4097), "errors": [{"message": "late"}]});
        assert_eq!(
            capture(&big, &[], None).to_value()["texts"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let many: Vec<_> = (0..20).map(|i| (format!("k{i}"), json!(1))).collect();
        let many = Value::Object(many.into_iter().collect());
        assert_eq!(
            capture(&many, &[], None).to_value()["bodyKeys"]
                .as_array()
                .unwrap()
                .len(),
            16
        );
    }

    #[test]
    fn non_numeric_and_out_of_range_codes_are_dropped() {
        let body = json!({"code": "ProxyError-private-code", "message": "x",
            "errors": [{"code": 1000000}, {"code": -5}, {"code": 1.5}, {"code": 0}]});
        let v = capture(&body, &[], None).to_value();
        assert_eq!(v["providerCodes"], json!([0]));
        assert!(!v.to_string().contains("ProxyError-private-code"));
    }

    #[test]
    fn rule1_controls_and_whitespace_collapse() {
        assert_eq!(msg("  a\u{0}b \n\t c\u{7f}d  "), "a b c d");
    }

    #[test]
    fn rule2_known_values_longest_first_case_insensitive() {
        let body = json!({"message": "user ALICE-bob and alice and abc ok"});
        let v = capture(&body, &["alice", "alice-bob", "abc"], None).to_value();
        assert_eq!(
            v["texts"][0]["text"],
            "user <secret> and <secret> and abc ok"
        );
        assert_eq!(v["redactions"]["known_value"], 2);
        let post = "Hello  world\nfoo";
        let t = first_text(
            &json!({"message": "rejected: hello world foo!"}),
            &[],
            Some(post),
        );
        assert_eq!(t, "rejected: <post_text>!");
    }

    #[test]
    fn rule3_shared_fragments_of_post_text() {
        let post = Some("The quick brown fox jumps over the lazy dog");
        let t = first_text(&json!({"message": "bad: quick brown fox here"}), &[], post);
        // 窓は空白も含むため、前後の空白も一緒に伏せられる。
        assert_eq!(t, "bad:<post_text>here");
        // 9文字だけ共通（前後は本文と一致しない）なら残る。
        let t = first_text(&json!({"message": "bad:quick bro, here"}), &[], post);
        assert_eq!(t, "bad:quick bro, here");
    }

    #[test]
    fn rule4_patterns() {
        assert_eq!(
            msg("go https://ex.com/a?b=1 or WWW.ex.com/x now"),
            "go <url> or <url> now"
        );
        assert_eq!(msg("mail me.x@ex.co.jp now"), "mail <email> now");
        assert_eq!(
            msg("Cookie: abc; auth_token=zzz9 ct0 = q"),
            "Cookie=<secret>; auth_token=<secret> ct0=<secret>"
        );
        assert_eq!(
            msg("Authorization: Bearer abcdef rest"),
            "Authorization=<secret> rest"
        );
        assert_eq!(
            msg("at 10.0.0.1:8080 and 10.0.0.2 and proxy.ex.net:3128"),
            "at <host> and <host> and <host>"
        );
        assert_eq!(msg("by @some_user, ok"), "by <user>, ok");
        assert_eq!(
            msg("tok Abc12345Z and 12345678 and 123456 end"),
            "tok <token> and <n> and <n> end"
        );
        assert_eq!(
            msg(&format!("long {} end", "a".repeat(32))),
            "long <token> end"
        );
    }

    #[test]
    fn short_numbers_non_english_and_plain_words_are_kept() {
        let text = "Error 4040 code 1234 ProxyError-private-code 日本語のエラー Ünïcode word";
        assert_eq!(msg(text), text);
    }

    #[test]
    fn rule5_truncates_at_500_chars() {
        let v = capture(&json!({"message": "あ ".repeat(400)}), &[], None).to_value();
        assert_eq!(v["texts"][0]["truncated"], true);
        assert_eq!(v["texts"][0]["text"].as_str().unwrap().chars().count(), 500);
        let v = capture(&json!({"message": "short"}), &[], None).to_value();
        assert_eq!(v["texts"][0]["truncated"], false);
    }

    #[test]
    fn rule6_drops_text_when_value_or_fragment_remains() {
        // 4文字未満の値は対象外、4文字以上は置換される。
        let v = capture(
            &json!({"message": "hello zed-9 world"}),
            &["ZED-9", "xy"],
            None,
        );
        assert_eq!(v.to_value()["texts"][0]["text"], "hello <secret> world");
        // 置換記号の内側は壊さず、置換記号と同じ綴りの値があっても捨てない。
        let v = capture(
            &json!({"message": "ct0=abc"}),
            &["secret", "post_text"],
            None,
        );
        assert_eq!(v.to_value()["texts"][0]["text"], "ct0=<secret>");
        // 最後の検査は、置換後に残った断片を見つけたら text を null にする。
        let needles = Needles::new(["leakvalue"].into_iter(), Some("0123456789abcdef"));
        assert!(needles.leaks("x leakvalue y"));
        assert!(needles.leaks("x 456789abcd y"));
        assert!(!needles.leaks("x <secret> <post_text> 45678 y"));
        let mut entry = CapturedText {
            field: "message".into(),
            text: Some("a LEAKVALUE b".into()),
            truncated: false,
        };
        assert!(verify(&mut entry, &needles));
        assert!(entry.text.is_none());
        let v = RejectionCapture {
            text_dropped: Some("verification_failed"),
            texts: vec![entry],
            ..capture(&json!({}), &[], None)
        }
        .to_value();
        assert_eq!(v["textDropped"], "verification_failed");
        assert!(v["texts"][0]["text"].is_null());
    }

    #[test]
    fn known_secrets_never_remain_in_output() {
        let known = ["hunter2pass", "abcd-user", "10.1.2.3", "zzcookie"];
        let body = json!({
            "message": "Login failed for ABCD-USER with HUNTER2PASS via 10.1.2.3 (ct0=zzcookie)",
            "errors": [{"message": "x hunter2pass y"}, {"message": "pro\u{0}xy ABCD-\nUSER"}],
            "error": {"message": "cookie zzCOOKIE"},
        });
        let out = capture(&body, &known, Some("my post body text"))
            .to_value()
            .to_string()
            .to_lowercase();
        for k in known {
            assert!(!out.contains(k), "{k} leaked: {out}");
        }
    }

    #[test]
    fn known_values_win_over_patterns_and_survive_clipping() {
        // パターンに部分的に当たる値（英数字の連続・空白を含む）も、1つの既知の値として先に伏せる。
        let v = capture(
            &json!({"message": "bad login: P4ssword with spaces ok"}),
            &["P4ssword with spaces"],
            None,
        )
        .to_value();
        assert_eq!(v["texts"][0]["text"], "bad login: <secret> ok");
        assert!(v.get("textDropped").is_none());
        // 500文字の境目をまたぐ値は、切る前に伏せるので一部も残らない。
        let message = format!("{}zqvalue99x tail", "ab ".repeat(165));
        let v = capture(&json!({"message": message}), &["zqvalue99x"], None).to_value();
        let text = v["texts"][0]["text"].as_str().unwrap();
        assert!(!text.to_lowercase().contains("zqval"));
        assert!(text.chars().count() <= 500);
        assert_eq!(v["texts"][0]["truncated"], true);
    }

    #[test]
    fn debug_hides_contents() {
        let cap = capture(&json!({"message": "very-private-text"}), &[], None);
        assert!(!format!("{cap:?}").contains("private"));
    }
}
