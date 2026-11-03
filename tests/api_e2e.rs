mod api_support;
mod support;
use anyhow::ensure;
use api_support::*;
use serde_json::{Value, json};
use support::Result;
macro_rules! case {
    ($h:ident,$name:expr,$body:block) => {{
        let name = $name.to_string();
        if $h.selected(&name) {
            let outcome: Result<()> = async {
                $body;
                Ok(())
            }
            .await;
            $h.suite.record(&name, outcome);
        }
    }};
}
mod api_auth_cases;
mod api_post_cases;
mod api_read_cases;

#[tokio::test]
async fn api_e2e() -> Result<()> {
    let mut h = Harness::new().await?;
    api_read_cases::basics(&mut h).await?;
    api_post_cases::run(&mut h).await?;
    api_read_cases::feed_search(&mut h).await?;
    api_auth_cases::run(&mut h).await?;
    case!(
        h,
        "Authentication, HTTP payloads and clean MCP diagnostics",
        {
            // 終了時の出力も含め、stderrのEOF回収を終えてから秘密値を検査する。
            h.client.close().await?;
            let trace = h.trace();
            ensure!(!trace.is_empty(), "No HTTP evidence");
            for r in &trace {
                ensure!(
                    r["authenticated"] == true && r["cookieHeaderPresent"] == false,
                    "Unsafe request headers"
                );
                let post = ["/create_tweet_v2", "/upload_media_v2", "/user_login_v2"]
                    .iter()
                    .any(|p| r["path"].as_str().unwrap().ends_with(p));
                ensure!(
                    r["method"] == if post { "POST" } else { "GET" },
                    "Wrong HTTP method"
                );
                if r["path"].as_str().unwrap().ends_with("/create_tweet_v2") {
                    ensure!(r["postingPayloadValid"] == true, "Invalid posting payload");
                }
                if r["path"].as_str().unwrap().ends_with("/user_login_v2") {
                    ensure!(
                        r["loginPayloadValid"] == true && r["totpSeedExact"] == true,
                        "Invalid login payload"
                    );
                }
            }
            let stderr = format!("{}{}", h.diagnostics.join(""), h.client.stderr());
            for s in [KEY, COOKIE, PASSWORD, SEED, PROXY] {
                ensure!(!stderr.contains(s), "Secret in diagnostics");
            }
            ensure!(
                tokio::fs::read(&h.secrets).await? == h.initial,
                "Configuration was changed"
            );
        }
    );
    h.finish().await
}
