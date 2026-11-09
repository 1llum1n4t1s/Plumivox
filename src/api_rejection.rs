//! 提供元の拒否応答を、秘密値を含まない固定の診断候補へ変換する。
use serde_json::{Value, json};

// 生のmessage・任意のcode・request IDは返さない。分類名だけを許可し、
// 未知の文面を伏字処理で外へ持ち出す経路も作らない。
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
];

pub(crate) fn details(http_status: u16, body: Option<&Value>) -> Value {
    let mut reasons = Vec::new();
    if let Some(body) = body {
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
        for (reason, patterns) in MESSAGE_RULES {
            if messages
                .iter()
                .any(|message| patterns.iter().any(|pattern| message.contains(*pattern)))
            {
                reasons.push(*reason);
            }
        }
    } else {
        // 非2xxの本文はこれまでどおり読まずに破棄する。固定分類は公開された
        // HTTPステータスの意味だけから付け、ストリーム停止・再試行条件を保つ。
        reasons.push(match http_status {
            400 => "invalid_request",
            401 => "unauthorized",
            402 => "credits",
            403 => "forbidden",
            404 => "endpoint_not_found",
            429 => "rate_limit",
            504 => "upstream_timeout",
            500 | 502 | 503 => "upstream_unavailable",
            _ => "unknown",
        });
    }
    if reasons.is_empty() {
        reasons.push("unknown");
    }
    json!({"httpStatus":http_status,"rejectionReasons":reasons,
        "rejectionReasonSource":if body.is_some(){"provider_message"}else{"http_status"}})
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
