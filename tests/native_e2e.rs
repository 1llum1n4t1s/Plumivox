mod support;
use anyhow::{Context, ensure};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    fs,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::{HttpServer, McpClient, Result, Suite, json_response, raw_response};

const KEY: &str = "synthetic-native-api-key-never-display";
const COOKIE: &str = "synthetic-native-cookie-never-display";
#[derive(Default)]
struct State {
    scenario: String,
    trace: Vec<Value>,
    next_post: u64,
    uploads: usize,
}
fn tweet(id: &str) -> Value {
    json!({"id":id,"text":"日本語 🫱🏻‍🫲🏽","createdAt":"2026-10-12T12:00:00Z","author":{"userName":"PublicUser","name":"Public User"},"url":format!("https://x.com/PublicUser/status/{id}")})
}
async fn call(
    client: &McpClient,
    state: &Arc<Mutex<State>>,
    name: &str,
    args: Value,
    failure: bool,
    offline: bool,
) -> Result<Value> {
    let before = state.lock().unwrap().trace.len();
    let raw = match client.call_tool(name, args).await {
        Ok(raw) => raw,
        Err(error) if failure && error.to_string().contains("JSON-RPC error -32602") => {
            if offline {
                ensure!(
                    state.lock().unwrap().trace.len() == before,
                    "Rejected input caused HTTP traffic"
                );
            }
            return Ok(json!({"error":{"code":"MCP_INVALID_PARAMS"}}));
        }
        Err(error) => return Err(error),
    };
    let serialized = raw.to_string();
    ensure!(
        !serialized.contains(KEY) && !serialized.contains(COOKIE),
        "Response exposed synthetic credentials"
    );
    if offline {
        ensure!(
            state.lock().unwrap().trace.len() == before,
            "Rejected input caused HTTP traffic"
        );
    }
    if raw.get("error").is_some() {
        ensure!(
            failure && raw["error"]["code"] == -32602,
            "Unexpected MCP protocol error"
        );
        return Ok(json!({"error":{"code":"MCP_INVALID_PARAMS"}}));
    }
    let data = raw
        .get("structuredContent")
        .cloned()
        .or_else(|| {
            raw["content"]
                .as_array()?
                .iter()
                .find_map(|v| serde_json::from_str(v["text"].as_str()?).ok())
        })
        .unwrap_or_else(|| json!({"error":{"code":"MCP_INVALID_PARAMS"}}));
    ensure!(
        raw["isError"].as_bool().unwrap_or(false) == failure,
        "Unexpected tool status: {data}"
    );
    if failure {
        ensure!(data["error"]["code"].is_string(), "Missing error code");
    }
    Ok(data)
}

#[tokio::test]
async fn native_stdio_contract() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_NATIVE_E2E_OUTPUT_DIR", "native")?;
    suite.add_secret(KEY.into());
    suite.add_secret(COOKIE.into());
    let config = json!({"twitterapi_io":{"posting":{"user_name":"Poster","api_key":KEY,"proxy":"http://127.0.0.1:8888","login_cookies":COOKIE},"reading":{"user_name":"Reader","api_key":KEY}}});
    let initial = config.to_string();
    let secrets = suite.output().join("synthetic-secrets.json");
    fs::write(&secrets, &initial)?;
    let png = suite.output().join("one-pixel.png");
    fs::write(&png,base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=")?)?;
    let state = Arc::new(Mutex::new(State {
        next_post: 900,
        ..Default::default()
    }));
    let accepted = Arc::new(tokio::sync::Notify::new());
    let closed = Arc::new(tokio::sync::Notify::new());
    struct Closed(Arc<tokio::sync::Notify>);
    impl Drop for Closed {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }
    let (s, a, c) = (state.clone(), accepted.clone(), closed.clone());
    let mut server = HttpServer::start(move |request| { let (s,a,c)=(s.clone(),a.clone(),c.clone()); async move {
        let query: std::collections::BTreeMap<String,String> = url::form_urlencoded::parse(request.query.as_bytes()).into_owned().collect();
        let body: Option<Value> = serde_json::from_slice(&request.body).ok();
        let scenario = { let mut s=s.lock().unwrap(); s.trace.push(json!({"method":request.method,"path":request.path,"query":query,"authenticated":request.headers.get("x-api-key").and_then(|v|v.to_str().ok())==Some(KEY),"cookieHeaderPresent":request.headers.contains_key("cookie"),"postingPayloadValid":body.as_ref().is_some_and(|v|v["login_cookies"]==COOKIE && v["proxy"]=="http://127.0.0.1:8888" && v["tweet_text"].is_string())})); s.scenario.clone() };
        if scenario=="cancel" { let _guard=Closed(c); a.notify_one(); std::future::pending::<()>().await; }
        if scenario=="timeout" { tokio::time::sleep(Duration::from_secs(1)).await; }
        if let Some(code)=scenario.strip_prefix("http").and_then(|v|v.parse::<u16>().ok()) { return Ok(json_response(code,json!({"message":COOKIE,"api_key":KEY}))); }
        if scenario=="html" { return Ok(raw_response(200,format!("<html>{COOKIE}</html>").into_bytes())); }
        if scenario=="jsonerror" { return Ok(json_response(200,json!({"status":"error","message":COOKIE}))); }
        let path=request.path.as_str();
        let value=if path.ends_with("/upload_media_v2") { let mut s=s.lock().unwrap(); s.uploads+=1; if scenario=="upload-fail" || (scenario=="upload-recover" && s.uploads==1) { return Ok(json_response(502,json!({"message":COOKIE}))); } json!({"status":"success","media_id":"700","media_id_string":"700"}) }
        else if path.ends_with("/create_tweet_v2") { let mut s=s.lock().unwrap(); let id=s.next_post;s.next_post+=1;if scenario=="missing-id" {json!({"status":"success"})}else{json!({"status":"success","tweet_id":id.to_string()})} }
        else if path.ends_with("/user/info") {json!({"data":{"id":"101","userName":"Reader"}})}
        else if path.ends_with("/tweets") {json!({"tweets":[tweet("502")]})}
        else if path.ends_with("/tweet/advanced_search") {let more=!query.contains_key("cursor");json!({"tweets":[tweet("603")],"has_next_page":more,"next_cursor":if more{"native-search-next"}else{""}})}
        else if path.ends_with("/user/last_tweets") || path.ends_with("/user/tweet_timeline") {json!({"tweets":[tweet("501")],"has_next_page":false,"next_cursor":""})}
        else if path.ends_with("/user/followings") {json!({"followings":[],"has_next_page":false,"next_cursor":""})}
        else { return Ok(json_response(404,json!({"message":"Unhandled local fixture endpoint"}))); };
        Ok(json_response(200,value))
    }}).await?;
    let mut client = McpClient::spawn(&[
        server.base_url.clone(),
        secrets.display().to_string(),
        suite.output().join("auth-state").display().to_string(),
        "250".into(),
        "--slow-image-upload".into(),
    ])
    .await?;
    macro_rules! case { ($name:expr,$body:block) => {{ state.lock().unwrap().scenario.clear(); let result:Result<()> = async $body.await; suite.record($name,result); }}; }
    macro_rules! call {
        ($name:expr,$args:expr) => {
            call(&client, &state, $name, $args, false, false).await?
        };
        ($name:expr,$args:expr,$fail:expr,$off:expr) => {
            call(&client, &state, $name, $args, $fail, $off).await?
        };
    }
    case!("MCP discovery", {
        let raw = client.request("tools/list", json!({})).await?;
        let mut names: Vec<_> = raw["tools"]
            .as_array()
            .context("tools array")?
            .iter()
            .filter_map(|v| v["name"].as_str())
            .collect();
        names.sort();
        ensure!(
            names
                == [
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
        Ok(())
    });
    case!(
        "List accounts preserves order, filters metadata and rereads without provider access",
        {
            fs::write(&secrets,json!({"twitterapi_io":{"reading":config["twitterapi_io"]["reading"],"ignored":17,"default_account":{},"array":[],"posting":config["twitterapi_io"]["posting"]}}).to_string())?;
            let checked:Result<()> = async {let data=call!("list_accounts",json!({}),false,true);ensure!(data["accounts"].as_array().unwrap().iter().map(|v|v["account"].as_str().unwrap()).collect::<Vec<_>>()==["reading","posting"]);ensure!(data["accounts"].as_array().unwrap().iter().all(|v|v["isDefault"]==false));
        for account in ["ignored","array","default_account"] {ensure!(call!("get_tweets",json!({"account":account,"tweetIds":["502"]}),true,true)["error"]["code"]=="ACCOUNT_UNKNOWN");}
        ensure!(call!("get_tweets",json!({"tweetIds":["502"]}),true,true)["error"]["code"]=="ACCOUNT_DEFAULT_INVALID");
        fs::write(&secrets,json!({"twitterapi_io":{"ignored":false,"reading":config["twitterapi_io"]["reading"],"array":[]}}).to_string())?;
        let data=call!("list_accounts",json!({}),false,true);ensure!(data["accounts"].as_array().unwrap().len()==1 && data["accounts"][0]["account"]=="reading" && data["accounts"][0]["isDefault"]==true);
        ensure!(call!("post_tweet",json!({"text":"Read-only default"}),true,true)["error"]["code"]=="POSTING_UNAVAILABLE");Ok(())}.await;
            fs::write(&secrets, &initial)?;
            checked?;
            ensure!(
                call!("list_accounts", json!({}), false, true)["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v["account"].as_str().unwrap())
                    .collect::<Vec<_>>()
                    == ["posting", "reading"]
            );
            Ok(())
        }
    );
    for (name, tool, args, code) in [
        (
            "Ambiguous account rejected",
            "search_tweets",
            json!({"keyword":"needle"}),
            "ACCOUNT_REQUIRED",
        ),
        (
            "Unknown account rejected",
            "get_tweets",
            json!({"account":"missing","tweetIds":["502"]}),
            "ACCOUNT_UNKNOWN",
        ),
        (
            "Read-only account cannot post",
            "post_tweet",
            json!({"account":"reading","text":"test"}),
            "POSTING_UNAVAILABLE",
        ),
    ] {
        case!(name, {
            ensure!(call!(tool, args, true, true)["error"]["code"] == code);
            Ok(())
        });
    }
    case!("Tweet lookup preserves UTF-8", {
        ensure!(
            call!(
                "get_tweets",
                json!({"account":"reading","tweetIds":["502"]})
            )["tweets"][0]["text"]
                == tweet("502")["text"]
        );
        Ok(())
    });
    case!("Own timeline", {
        ensure!(call!("get_my_posts", json!({"account":"reading"}))["tweets"][0]["id"] == "501");
        Ok(())
    });
    case!("Empty following feed", {
        ensure!(call!("get_following_feed", json!({"account":"reading"}))["tweets"] == json!([]));
        Ok(())
    });
    for ids in [json!([]), json!(["bad"]), json!(["502", "bad"])] {
        case!(format!("Invalid lookup IDs {ids}"), {
            call!(
                "get_tweets",
                json!({"account":"reading","tweetIds":ids}),
                true,
                true
            );
            Ok(())
        });
    }
    for text in [String::new(), " ".into(), "あ".repeat(141)] {
        case!(
            format!("Invalid post text length {}", text.chars().count()),
            {
                call!(
                    "post_tweet",
                    json!({"account":"posting","text":text}),
                    true,
                    true
                );
                Ok(())
            }
        );
    }
    for (name, mode, expected, fail) in [
        (
            "Image upload precedes one post",
            "",
            vec!["upload_media_v2", "create_tweet_v2"],
            false,
        ),
        (
            "Recoverable upload retries without resending post",
            "upload-recover",
            vec!["upload_media_v2", "upload_media_v2", "create_tweet_v2"],
            false,
        ),
        (
            "Failed upload never creates a post",
            "upload-fail",
            vec![],
            true,
        ),
    ] {
        case!(name, {
            let n = {
                let mut s = state.lock().unwrap();
                s.scenario = mode.into();
                s.uploads = 0;
                s.trace.len()
            };
            call!(
                "post_tweet",
                json!({"account":"posting","text":"image","images":[png]}),
                fail,
                false
            );
            let s = state.lock().unwrap();
            let paths: Vec<_> = s.trace[n..]
                .iter()
                .map(|v| v["path"].as_str().unwrap().rsplit('/').next().unwrap())
                .collect();
            if fail {
                ensure!(!paths.is_empty() && paths.iter().all(|p| *p == "upload_media_v2"));
            } else {
                ensure!(paths == expected);
            }
            Ok(())
        });
    }
    case!("Search cursor continuation", {
        let first = call!(
            "search_tweets",
            json!({"account":"reading","keyword":"needle"})
        );
        ensure!(first["nextCursor"].as_str().is_some_and(|v| !v.is_empty()));
        call!(
            "search_tweets",
            json!({"account":"reading","keyword":"needle","cursor":first["nextCursor"]})
        );
        ensure!(
            state.lock().unwrap().trace.last().unwrap()["query"]["cursor"] == "native-search-next"
        );
        call!(
            "search_tweets",
            json!({"account":"reading","keyword":"changed","cursor":first["nextCursor"]}),
            true,
            true
        );
        Ok(())
    });
    for (name, value, invalid) in [
        (
            "Default account reread and explicit selection retained",
            json!({"twitterapi_io":{"posting":config["twitterapi_io"]["posting"],"reading":config["twitterapi_io"]["reading"],"default_account":"posting"}}),
            false,
        ),
        (
            "Invalid default does not silently select another account",
            json!({"twitterapi_io":{"posting":config["twitterapi_io"]["posting"],"reading":config["twitterapi_io"]["reading"],"default_account":"missing"}}),
            true,
        ),
    ] {
        case!(name, {
            fs::write(&secrets, value.to_string())?;
            let checked: Result<()> = async {
                let data = call!(
                    "search_tweets",
                    json!({"keyword":"needle"}),
                    invalid,
                    invalid
                );
                if invalid {
                    ensure!(data["error"]["code"] == "ACCOUNT_DEFAULT_INVALID");
                } else {
                    ensure!(data["account"] == "posting");
                    call!(
                        "post_tweet",
                        json!({"account":"reading","text":"test"}),
                        true,
                        true
                    );
                }
                Ok(())
            }
            .await;
            fs::write(&secrets, &initial)?;
            checked
        });
    }
    case!("Unreadable selected configuration does not fall back", {
        fs::write(&secrets, "{invalid synthetic JSON")?;
        let r = call(
            &client,
            &state,
            "get_tweets",
            json!({"account":"reading","tweetIds":["502"]}),
            true,
            true,
        )
        .await;
        fs::write(&secrets, &initial)?;
        r?;
        Ok(())
    });
    for mode in [
        "http401",
        "http429",
        "http500",
        "http502",
        "http503",
        "http504",
        "html",
        "jsonerror",
        "timeout",
    ] {
        case!(
            format!("Safe provider failure without read retry: {mode}"),
            {
                state.lock().unwrap().scenario = mode.into();
                let n = state.lock().unwrap().trace.len();
                call!(
                    "get_tweets",
                    json!({"account":"reading","tweetIds":["502"]}),
                    true,
                    false
                );
                ensure!(state.lock().unwrap().trace.len() == n + 1);
                Ok(())
            }
        );
    }
    for mode in ["http502", "missing-id", "timeout"] {
        case!(format!("Uncertain post is not resent: {mode}"), {
            state.lock().unwrap().scenario = mode.into();
            let n = state.lock().unwrap().trace.len();
            call!(
                "post_tweet",
                json!({"account":"posting","text":"test"}),
                true,
                false
            );
            ensure!(state.lock().unwrap().trace.len() == n + 1);
            Ok(())
        });
    }
    case!("Cancellation reaches actual request and does not resend", {
        state.lock().unwrap().scenario = "cancel".into();
        let n = state.lock().unwrap().trace.len();
        let pending=client.start_request("tools/call",json!({"name":"search_tweets","arguments":{"account":"reading","keyword":"needle"}})).await?;
        tokio::time::timeout(Duration::from_secs(3), accepted.notified()).await?;
        client.cancel(pending.id).await?;
        tokio::time::timeout(Duration::from_secs(2), closed.notified()).await?;
        state.lock().unwrap().scenario.clear();
        call!(
            "get_tweets",
            json!({"account":"reading","tweetIds":["502"]})
        );
        ensure!(state.lock().unwrap().trace.len() == n + 2);
        Ok(())
    });
    // closeでstderr readerのEOFまで回収してから、末尾を含む診断を検査する。
    let shutdown = client.close().await;
    let diagnostics = client.stderr();
    let provider_shutdown = server.close().await;
    let mut transport_diagnostics = Vec::new();
    case!("Wire authentication and sanitized diagnostics", {
        let mut transport_failures = Vec::new();
        for (mode, expected_error) in [
            ("--stderr-after-eof", None),
            (
                "--stdout-invalid-after-eof",
                Some("Cannot read native MCP stdout"),
            ),
            (
                "--stdout-nonmcp-after-eof",
                Some("Native MCP wrote an invalid JSON-RPC message"),
            ),
            (
                "--stdout-unknown-id-after-eof",
                Some("Native MCP wrote an unexpected response ID"),
            ),
            (
                "--stderr-hold-after-eof",
                Some("Native MCP stderr EOF collection timed out"),
            ),
        ] {
            let mut terminal_stderr = String::new();
            let mut terminal_error = None;
            let result: Result<()> = async {
                // spawnはinitialize完了まで待つ。起動失敗を終了時の拒否の代用にしない。
                let mut transport = McpClient::spawn(&[
                    server.base_url.clone(),
                    secrets.display().to_string(),
                    suite
                        .output()
                        .join(mode.trim_start_matches('-'))
                        .display()
                        .to_string(),
                    "250".into(),
                    mode.into(),
                ])
                .await
                .context("Transport fixture initialization failed")?;
                let closed = transport.close().await;
                terminal_stderr = transport.stderr();
                ensure!(
                    !terminal_stderr.contains(KEY) && !terminal_stderr.contains(COOKIE),
                    "Transport diagnostics exposed synthetic credentials"
                );
                match expected_error {
                    None => {
                        closed?;
                        ensure!(
                            terminal_stderr.contains("synthetic-transport-tail"),
                            "stderr emitted after stdin EOF was not collected"
                        );
                    }
                    Some(reason) => {
                        let error = closed
                            .err()
                            .context("Invalid transport shutdown was accepted")?;
                        let message = format!("{error:#}");
                        terminal_error = Some(message.clone());
                        ensure!(
                            message.contains(reason),
                            "Wrong transport shutdown failure: {message}"
                        );
                    }
                }
                Ok(())
            }
            .await;
            let passed = result.is_ok();
            if let Err(error) = result {
                transport_failures.push(format!("{mode}: {error:#}"));
            }
            transport_diagnostics.push(json!({"mode":mode,"passed":passed,"stderr":terminal_stderr,"closeError":terminal_error}));
        }
        ensure!(
            transport_failures.is_empty(),
            "{}",
            transport_failures.join("; ")
        );
        ensure!(!diagnostics.contains(KEY) && !diagnostics.contains(COOKIE));
        shutdown?;
        provider_shutdown?;
        let s = state.lock().unwrap();
        ensure!(
            !s.trace.is_empty()
                && s.trace
                    .iter()
                    .all(|v| v["authenticated"] == true && v["cookieHeaderPresent"] == false)
        );
        ensure!(
            s.trace
                .iter()
                .filter(|v| v["path"].as_str().unwrap().ends_with("/create_tweet_v2"))
                .all(|v| v["method"] == "POST" && v["postingPayloadValid"] == true)
        );
        ensure!(fs::read_to_string(&secrets)? == initial);
        Ok(())
    });
    suite.write_json(
        "diagnostics.json",
        &json!({"stderr":diagnostics,"transport":transport_diagnostics}),
    )?;
    suite.write_json("http-trace.json", &json!(state.lock().unwrap().trace))?;
    suite.finish()
}
