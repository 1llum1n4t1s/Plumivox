use crate::{
    client::TwitterApiClient,
    config::AccountStore,
    error::{AppError, Result},
    proxy::SharedProxyManager,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub struct Options {
    pub api_base_url: String,
    pub secrets_path: Option<PathBuf>,
    pub state_directory: Option<PathBuf>,
    pub profile: Option<String>,
    pub timeout: Duration,
    pub image_timeout: Duration,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.twitterapi.io".into(),
            secrets_path: None,
            state_directory: None,
            profile: None,
            timeout: Duration::from_secs(45),
            image_timeout: Duration::from_secs(120),
        }
    }
}

fn tool_definitions() -> &'static Value {
    static TOOLS: OnceLock<Value> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(include_str!("tool-definitions.json")).expect("bundled MCP schemas")
    })
}

#[cfg(feature = "test-fixture")]
static FIXTURE_MODE: OnceLock<String> = OnceLock::new();
#[cfg(feature = "test-fixture")]
pub fn configure_fixture_mode(mode: String) {
    let _ = FIXTURE_MODE.set(mode);
}

/// Zodと同じ入力境界。未知のプロパティは実処理に渡さず、秘密入力をエラーに転載しない。
fn validate(input: &Value, schema: &Value) -> std::result::Result<Value, ()> {
    if let Some(values) = schema["enum"].as_array() {
        if !values.contains(input) {
            return Err(());
        }
    }
    match schema["type"].as_str() {
        Some("object") => {
            let object = input.as_object().ok_or(())?;
            let mut result = json!({});
            if let Some(properties) = schema["properties"].as_object() {
                for (key, child) in properties {
                    if let Some(value) = object.get(key) {
                        result[key] = validate(value, child)?;
                    } else if let Some(value) = child.get("default") {
                        result[key] = value.clone();
                    } else if schema["required"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(key)))
                    {
                        return Err(());
                    }
                }
            }
            Ok(result)
        }
        Some("string") => {
            let text = input.as_str().ok_or(())?;
            let length = text.encode_utf16().count() as u64;
            if let Some(pattern) = schema["pattern"].as_str() {
                if !regex::RegexBuilder::new(pattern)
                    .unicode(false)
                    .build()
                    .map_err(|_| ())?
                    .is_match(text)
                {
                    return Err(());
                }
            }
            if schema["minLength"].as_u64().is_some_and(|min| length < min)
                || schema["maxLength"].as_u64().is_some_and(|max| length > max)
            {
                return Err(());
            }
            Ok(input.clone())
        }
        Some("boolean") if input.is_boolean() => Ok(input.clone()),
        Some("integer") | Some("number") => {
            let number = input.as_f64().ok_or(())?;
            if !number.is_finite()
                || (schema["type"] == "integer" && number.fract() != 0.0)
                || schema["minimum"].as_f64().is_some_and(|min| number < min)
                || schema["maximum"].as_f64().is_some_and(|max| number > max)
            {
                return Err(());
            }
            Ok(input.clone())
        }
        Some("array") => {
            let values = input.as_array().ok_or(())?;
            if schema["minItems"]
                .as_u64()
                .is_some_and(|min| (values.len() as u64) < min)
                || schema["maxItems"]
                    .as_u64()
                    .is_some_and(|max| values.len() as u64 > max)
            {
                return Err(());
            }
            Ok(Value::Array(
                values
                    .iter()
                    .map(|value| validate(value, &schema["items"]))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            ))
        }
        _ => Err(()),
    }
}

struct Server {
    accounts: AccountStore,
    api: TwitterApiClient,
    proxy: SharedProxyManager,
    authenticating: std::sync::Mutex<HashSet<String>>,
}

struct Authenticating<'a> {
    name: String,
    active: &'a std::sync::Mutex<HashSet<String>>,
}
impl Drop for Authenticating<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.name);
        }
    }
}

impl Server {
    async fn ensure_proxy(&self, selected: &Value, cancel: CancellationToken) -> Result<()> {
        #[cfg(feature = "test-fixture")]
        if FIXTURE_MODE
            .get()
            .is_some_and(|mode| mode == "--unknown-proxy-error")
        {
            return Err(AppError::new(
                "INTERNAL_ERROR",
                "処理に失敗しました。設定とAPIの状態を確認してください。",
            ));
        }
        let name = selected["account"].as_str();
        let expected = selected["proxy"].as_str();
        let config = self
            .accounts
            .get_proxy_configuration(name, expected)
            .await?;
        self.proxy.ensure_ready(&config, cancel).await?;
        let current = self
            .accounts
            .get_proxy_configuration(name, expected)
            .await?;
        if current != config || {
            #[cfg(feature = "test-fixture")]
            {
                FIXTURE_MODE
                    .get()
                    .is_some_and(|mode| mode == "--proxy-config-change")
            }
            #[cfg(not(feature = "test-fixture"))]
            {
                false
            }
        } {
            return Err(AppError::new(
                "AUTH_CONFIG_CHANGED",
                "中継の準備中に設定が変更されました。再実行してください。",
            ));
        }
        Ok(())
    }

    async fn authenticate(
        &self,
        name: Option<&str>,
        refresh: bool,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let selected = self.accounts.get_read_account(name).await?;
        let name = selected["account"].as_str().unwrap_or_default().to_owned();
        {
            let mut active = self
                .authenticating
                .lock()
                .map_err(|_| AppError::new("INTERNAL_ERROR", "処理に失敗しました。"))?;
            if !active.insert(name.clone()) {
                return Err(AppError::new(
                    "AUTH_BUSY",
                    "このアカウントの認証更新は実行中です。",
                ));
            }
        }
        let _active = Authenticating {
            name: name.clone(),
            active: &self.authenticating,
        };
        if !refresh {
            let metadata = self.accounts.get_account_metadata(Some(&name)).await?;
            if metadata["postConfigured"] == true {
                return Ok(
                    json!({"account":metadata["account"], "userName":metadata["userName"],
                    "sessionSource":metadata["sessionSource"], "acquiredAt":metadata["sessionAcquiredAt"],
                    "postConfigured":true, "reused":true}),
                );
            }
        }
        let (locked, lease) = self.accounts.lock_session(Some(&name)).await?;
        let credentials = self.accounts.get_login_credentials(Some(&name)).await?;
        if credentials["account"] != locked["account"]
            || credentials["userName"].as_str().map(str::to_lowercase)
                != locked["userName"].as_str().map(str::to_lowercase)
        {
            return Err(AppError::new(
                "AUTH_CONFIG_CHANGED",
                "認証の準備中にアカウント設定が変更されました。保存セッションは変更していません。",
            ));
        }
        self.accounts.prepare_session_storage(Some(&name)).await?;
        let auth_cancel = cancel.child_token();
        let loss = lease.cancellation_token();
        let linked = auth_cancel.clone();
        let watch = tokio::spawn(async move {
            tokio::select! { _ = loss.cancelled() => linked.cancel(), _ = linked.cancelled() => {} }
        });
        struct Watch(tokio::task::JoinHandle<()>);
        impl Drop for Watch {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _watch = Watch(watch);
        let active = || -> Result<()> {
            lease.assert_owned()?;
            if cancel.is_cancelled() {
                return Err(AppError::new(
                    "AUTH_CANCELLED",
                    "ログインは中止されました。この処理では保存セッションを変更していません。",
                ));
            }
            Ok(())
        };
        active()?;
        self.ensure_proxy(&credentials, auth_cancel.clone()).await?;
        active()?;
        let cookie = self
            .api
            .login_account(&credentials, auth_cancel.clone())
            .await;
        active()?;
        let cookie = cookie?;
        let mut saved = self
            .accounts
            .save_session(Some(&name), &cookie, &credentials, &lease, auth_cancel)
            .await?;
        saved["postConfigured"] = json!(true);
        saved["reused"] = json!(false);
        Ok(saved)
    }

    async fn call(&self, name: &str, input: &Value, cancel: CancellationToken) -> Result<Value> {
        let account = input["account"].as_str();
        match name {
            "list_accounts" => Ok(json!({"accounts": self.accounts.list_accounts().await?})),
            "get_proxy_status" => self.proxy.get_status().await,
            "start_proxy" => {
                let config = self.accounts.get_proxy_configuration(account, None).await?;
                if config.is_null() {
                    return Err(AppError::new(
                        "PROXY_EXTERNAL",
                        "externalモードのプロキシはPlumivoxの起動管理対象ではありません。",
                    ));
                }
                self.proxy.start(&config, cancel).await
            }
            "stop_proxy" => {
                self.proxy.stop().await?;
                self.proxy.get_status().await
            }
            "login_account" => self.authenticate(account, false, cancel).await,
            "refresh_session" => self.authenticate(account, true, cancel).await,
            "post_tweet" => {
                let selected = self.accounts.get_account(account, true).await?;
                let prepared = self
                    .api
                    .prepare_post(&selected, input, cancel.clone())
                    .await?;
                self.ensure_proxy(&selected, cancel.clone()).await?;
                self.api.send_post(&selected, prepared, cancel).await
            }
            _ => {
                let selected = self.accounts.get_read_account(account).await?;
                match name {
                    "get_my_posts" => self.api.get_my_posts(&selected, input, cancel).await,
                    "get_tweets" => self.api.get_tweets(&selected, input, cancel).await,
                    "get_following_feed" => {
                        self.api.get_following_feed(&selected, input, cancel).await
                    }
                    "search_tweets" => self.api.search_tweets(&selected, input, cancel).await,
                    _ => Err(AppError::new("INTERNAL_ERROR", "処理に失敗しました。")),
                }
            }
        }
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
}
async fn write_response(writer: &Mutex<tokio::io::Stdout>, response: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(response)
        .map_err(|_| AppError::new("MCP_FAILED", "MCP通信に失敗しました。"))?;
    bytes.push(b'\n');
    let mut output = writer.lock().await;
    output
        .write_all(&bytes)
        .await
        .map_err(|_| AppError::new("MCP_FAILED", "MCP通信に失敗しました。"))?;
    output
        .flush()
        .await
        .map_err(|_| AppError::new("MCP_FAILED", "MCP通信に失敗しました。"))
}

pub async fn run_stdio(options: Options) -> Result<()> {
    let server = Arc::new(Server {
        accounts: AccountStore::new(
            options.secrets_path,
            options.profile,
            options.state_directory,
        ),
        api: TwitterApiClient::new(options.api_base_url, options.timeout, options.image_timeout),
        proxy: SharedProxyManager::new(),
        authenticating: std::sync::Mutex::new(HashSet::new()),
    });
    let writer = Arc::new(Mutex::new(tokio::io::stdout()));
    let requests = Arc::new(Mutex::new(HashMap::<String, CancellationToken>::new()));
    let mut tasks = JoinSet::new();
    let mut input = BufReader::new(tokio::io::stdin());
    let mut initialized = false;
    loop {
        let mut line = Vec::new();
        // read_until自体の無制限確保を避け、改行前に上限を確認する。
        let mut eof = false;
        loop {
            let chunk = input
                .fill_buf()
                .await
                .map_err(|_| AppError::new("MCP_FAILED", "MCP通信に失敗しました。"))?;
            if chunk.is_empty() {
                eof = true;
                break;
            }
            let take = chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|position| position + 1)
                .unwrap_or(chunk.len());
            if line.len() + take > 2 * 1024 * 1024 {
                return Err(AppError::new("MCP_FAILED", "MCP入力が長すぎます。"));
            }
            line.extend_from_slice(&chunk[..take]);
            let complete = chunk[take - 1] == b'\n';
            input.consume(take);
            if complete {
                break;
            }
        }
        if eof && line.is_empty() {
            break;
        }
        let request: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                write_response(&writer, &rpc_error(Value::Null, -32700, "Parse error")).await?;
                continue;
            }
        };
        let id = request.get("id").cloned();
        let method = request["method"].as_str().unwrap_or_default();
        if request["jsonrpc"] != "2.0"
            || method.is_empty()
            || id
                .as_ref()
                .is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64()))
        {
            write_response(
                &writer,
                &rpc_error(id.unwrap_or(Value::Null), -32600, "Invalid Request"),
            )
            .await?;
            continue;
        }
        if id.is_none() {
            if method == "notifications/cancelled" {
                if let Some(cancel_id) = request["params"].get("requestId") {
                    if let Some(cancel) = requests.lock().await.get(&cancel_id.to_string()) {
                        cancel.cancel();
                    }
                }
            }
            continue;
        }
        let id = id.unwrap();
        let response = match method {
            "initialize" => {
                initialized = true;
                let proposed = request["params"]["protocolVersion"]
                    .as_str()
                    .unwrap_or_default();
                let protocol = if ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"]
                    .contains(&proposed)
                {
                    proposed
                } else {
                    "2025-11-25"
                };
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":protocol,"capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"plumivox","version":env!("CARGO_PKG_VERSION")}}})
            }
            "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
            _ if !initialized => rpc_error(id, -32000, "Server is not initialized"),
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":tool_definitions()}}),
            "tools/call" => {
                let name = request["params"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let definition = tool_definitions()
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|tool| tool["name"] == name);
                let Some(definition) = definition else {
                    write_response(&writer, &rpc_error(id, -32602, "Unknown tool")).await?;
                    continue;
                };
                let empty = json!({});
                let arguments = request["params"].get("arguments").unwrap_or(&empty);
                let input = match validate(arguments, &definition["inputSchema"]) {
                    Ok(input) => input,
                    Err(_) => {
                        write_response(&writer, &rpc_error(id, -32602, "Invalid tool arguments"))
                            .await?;
                        continue;
                    }
                };
                let cancel = CancellationToken::new();
                let key = id.to_string();
                let mut pending = requests.lock().await;
                if pending.len() >= 64 || pending.contains_key(&key) {
                    drop(pending);
                    write_response(
                        &writer,
                        &rpc_error(id, -32600, "Request limit or duplicate id"),
                    )
                    .await?;
                    continue;
                }
                pending.insert(key.clone(), cancel.clone());
                drop(pending);
                let server = server.clone();
                let writer = writer.clone();
                let requests = requests.clone();
                tasks.spawn(async move {
                    #[cfg(feature = "test-fixture")]
                    let snapshot = crate::test_support::begin_call();
                    let result = match server.call(&name, &input, cancel.clone()).await {
                        Ok(result) => json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result}),
                        Err(error) => {
                            let mut error_value = error.value();
                            if name == "post_tweet" && error.code != "INTERNAL_ERROR" {
                                let object = error_value.as_object_mut().unwrap();
                                object.entry("stage").or_insert(json!("pre_send"));
                                object.entry("postAttempted").or_insert(json!(false));
                                object.entry("uploadAttempts").or_insert(json!(0));
                            }
                            let result = json!({"error":error_value});
                            json!({"isError":true,"content":[{"type":"text","text":result.to_string()}]})
                        }
                    };
                    #[cfg(feature = "test-fixture")]
                    crate::test_support::end_call(&name, &result, snapshot);
                    let sent = if cancel.is_cancelled() { Ok(()) } else {
                        write_response(&writer, &json!({"jsonrpc":"2.0","id":id,"result":result})).await
                    };
                    requests.lock().await.remove(&key);
                    sent
                });
                while tasks.try_join_next().is_some() {}
                continue;
            }
            _ => rpc_error(id, -32601, "Method not found"),
        };
        write_response(&writer, &response).await?;
    }
    for cancel in requests.lock().await.values() {
        cancel.cancel();
    }
    while tasks.join_next().await.is_some() {}
    server.proxy.close().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schemas_reject_invalid_boundaries_and_strip_secret_properties() {
        for tool in tool_definitions().as_array().unwrap() {
            assert!(tool["name"].is_string());
            if tool["name"] == "post_tweet" {
                assert!(validate(&json!({"text":"", "images":[]}), &tool["inputSchema"]).is_err());
                let input = validate(
                    &json!({"text":"hello", "password":"do-not-forward"}),
                    &tool["inputSchema"],
                )
                .unwrap();
                assert!(input.get("password").is_none());
                assert_eq!(input["images"], json!([]));
            }
            if tool["name"] == "get_tweets" {
                assert!(validate(&json!({"tweetIds":["١٢٣"]}), &tool["inputSchema"]).is_err());
            }
        }
    }
}
