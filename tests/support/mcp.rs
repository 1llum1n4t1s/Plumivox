use super::Result;
use anyhow::{Context, anyhow, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

type Replies = Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

fn valid_message(value: &Value) -> bool {
    if !value.is_object() || value["jsonrpc"] != "2.0" {
        return false;
    }
    let id_valid = |id: &Value| id.is_string() || id.as_f64().is_some_and(|id| id.fract() == 0.0);
    if let Some(method) = value.get("method") {
        return method.is_string()
            && value.get("params").is_none_or(Value::is_object)
            && value.get("id").is_none_or(id_valid)
            && value.get("result").is_none()
            && value.get("error").is_none();
    }
    if !value.get("id").is_some_and(id_valid) {
        return false;
    }
    match (value.get("result"), value.get("error")) {
        (Some(result), None) => result.is_object(),
        (None, Some(error)) => {
            error.is_object()
                && error["code"]
                    .as_f64()
                    .is_some_and(|code| code.fract() == 0.0)
                && error["message"].is_string()
        }
        _ => false,
    }
}

pub struct PendingCall {
    pub id: u64,
    receiver: oneshot::Receiver<Result<Value>>,
    replies: Replies,
}

impl PendingCall {
    pub async fn wait(self) -> Result<Value> {
        self.wait_timeout(Duration::from_secs(90)).await
    }

    pub async fn wait_timeout(mut self, duration: Duration) -> Result<Value> {
        tokio::time::timeout(duration, &mut self.receiver)
            .await
            .context("MCP request timed out")?
            .context("MCP reply channel closed")?
    }
}

impl Drop for PendingCall {
    fn drop(&mut self) {
        self.replies.lock().unwrap().remove(&self.id);
    }
}

pub struct McpClient {
    child: Option<Child>,
    input: Arc<Mutex<Option<ChildStdin>>>,
    replies: Replies,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
    diagnostics: Arc<StdMutex<Vec<u8>>>,
    reader: Option<JoinHandle<Result<()>>>,
    stderr_reader: Option<JoinHandle<Result<()>>>,
    close_error: Option<String>,
}

impl McpClient {
    pub async fn spawn(args: &[String]) -> Result<Self> {
        Self::spawn_with_env(args, &[]).await
    }

    pub async fn spawn_with_env(args: &[String], environment: &[(String, String)]) -> Result<Self> {
        let executable = std::env::var_os("PLUMIVOX_NATIVE_FIXTURE")
            .or_else(|| option_env!("CARGO_BIN_EXE_plumivox-fixture").map(Into::into))
            .context("Native fixture executable is unavailable; build with test-fixture")?;
        Self::spawn_command(Path::new(&executable), args, environment).await
    }

    pub async fn spawn_command(
        executable: &Path,
        args: &[String],
        environment: &[(String, String)],
    ) -> Result<Self> {
        let mut command = Command::new(executable);
        // テストの子プロセスごとにコンソール窓が点滅しないようにする。
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for key in [
            "PLUMIVOX_SECRETS_FILE",
            "PLUMIVOX_DEFAULT_ACCOUNT",
            "PLUMIVOX_STATE_DIR",
            "PLUMIVOX_TEST_HOOKS",
        ] {
            command.env_remove(key);
        }
        command.envs(environment.iter().map(|(key, value)| (key, value)));
        let mut child = command.spawn().context("Cannot start native MCP process")?;
        let input = child.stdin.take().context("Missing MCP stdin")?;
        let stdout = child.stdout.take().context("Missing MCP stdout")?;
        let mut stderr = child.stderr.take().context("Missing MCP stderr")?;
        let replies: Replies = Arc::default();
        let reader_replies = replies.clone();
        let closed = Arc::new(AtomicBool::new(false));
        let reader_closed = closed.clone();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            let (reason, failed) = loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let value: Value = match serde_json::from_str(&line) {
                            Ok(value) => value,
                            Err(_) => break ("Native MCP wrote invalid JSON to stdout", true),
                        };
                        if !valid_message(&value) {
                            break ("Native MCP wrote an invalid JSON-RPC message", true);
                        }
                        if value.get("method").is_none() {
                            let id = value["id"].as_u64().or_else(|| {
                                value["id"]
                                    .as_f64()
                                    .filter(|id| *id >= 0.0 && *id <= 9_007_199_254_740_991.0)
                                    .map(|id| id as u64)
                            });
                            let sender =
                                id.and_then(|id| reader_replies.lock().unwrap().remove(&id));
                            if let Some(sender) = sender {
                                let response = if let Some(result) = value.get("result") {
                                    Ok(result.clone())
                                } else {
                                    Err(anyhow!("JSON-RPC error {}", value["error"]["code"]))
                                };
                                let _ = sender.send(response);
                            } else {
                                break ("Native MCP wrote an unexpected response ID", true);
                            }
                        }
                    }
                    Ok(None) => break ("Native MCP stdout closed", false),
                    Err(_) => break ("Cannot read native MCP stdout", true),
                }
            };
            reader_closed.store(true, Ordering::Release);
            for (_, sender) in reader_replies.lock().unwrap().drain() {
                let _ = sender.send(Err(anyhow!(reason)));
            }
            ensure!(!failed, "{reason}");
            Ok(())
        });
        let diagnostics: Arc<StdMutex<Vec<u8>>> = Arc::default();
        let captured = diagnostics.clone();
        let stderr_reader = tokio::spawn(async move {
            let mut buffer = [0; 8192];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => return Ok(()),
                    Ok(length) => {
                        let mut output = captured.lock().unwrap();
                        // 全出力を検査対象にし、上限以降の秘密値漏洩を見逃さない。
                        output.extend_from_slice(&buffer[..length]);
                    }
                    Err(_) => return Err(anyhow!("Cannot read native MCP stderr")),
                }
            }
        });
        let mut client = Self {
            child: Some(child),
            input: Arc::new(Mutex::new(Some(input))),
            replies,
            next_id: AtomicU64::new(1),
            closed,
            diagnostics,
            reader: Some(reader),
            stderr_reader: Some(stderr_reader),
            close_error: None,
        };
        if let Err(error) = client
            .request(
                "initialize",
                json!({"protocolVersion":"2024-11-05",
            "capabilities":{},"clientInfo":{"name":"plumivox-rust-e2e","version":"1.0.0"}}),
            )
            .await
        {
            let _ = client.close().await;
            return Err(error);
        }
        client
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(client)
    }

    async fn send(&self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let mut guard = self.input.lock().await;
        let input = guard.as_mut().context("MCP stdin already closed")?;
        input
            .write_all(&bytes)
            .await
            .context("Cannot send MCP request")?;
        input.flush().await.context("Cannot flush MCP request")
    }

    pub async fn start_request(&self, method: &str, params: Value) -> Result<PendingCall> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        {
            let mut replies = self.replies.lock().unwrap();
            ensure!(
                !self.closed.load(Ordering::Acquire),
                "MCP stdout already closed"
            );
            replies.insert(id, sender);
        }
        let pending = PendingCall {
            id,
            receiver,
            replies: self.replies.clone(),
        };
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        Ok(pending)
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.start_request(method, params).await?.wait().await
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await
    }

    pub async fn cancel(&self, id: u64) -> Result<()> {
        self.notify(
            "notifications/cancelled",
            json!({"requestId":id,"reason":"Synthetic E2E cancellation"}),
        )
        .await
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.request("tools/call", json!({"name":name,"arguments":args}))
            .await
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.diagnostics.lock().unwrap()).into_owned()
    }

    pub async fn close(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        self.closed.store(true, Ordering::Release);
        self.input.lock().await.take();
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(result) => {
                    if result.is_err() {
                        failures.push("Cannot wait for native MCP shutdown".to_owned());
                    }
                }
                Err(_) => {
                    failures.push("Native MCP shutdown timed out".to_owned());
                    if child.start_kill().is_err() {
                        failures.push("Cannot stop owned MCP process".to_owned());
                    }
                    if !matches!(
                        tokio::time::timeout(Duration::from_secs(5), child.wait()).await,
                        Ok(Ok(_))
                    ) {
                        failures.push("Owned MCP process did not stop".to_owned());
                    }
                }
            }
        }
        // 子が終了した後も、stdout/stderrのEOFまで回収してから成否を判定する。
        for (reader, label) in [
            (&mut self.reader, "stdout"),
            (&mut self.stderr_reader, "stderr"),
        ] {
            if let Err(error) = collect_reader(reader, label).await {
                failures.push(error.to_string());
            }
        }
        for (_, sender) in self.replies.lock().unwrap().drain() {
            let _ = sender.send(Err(anyhow!("MCP client closed")));
        }
        if !failures.is_empty() {
            self.close_error = Some(failures.join("; "));
        }
        match &self.close_error {
            Some(error) => Err(anyhow!(error.clone())),
            None => Ok(()),
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Some(reader) = &self.reader {
            reader.abort();
        }
        if let Some(stderr_reader) = &self.stderr_reader {
            stderr_reader.abort();
        }
        for (_, sender) in self.replies.lock().unwrap().drain() {
            let _ = sender.send(Err(anyhow!("MCP client dropped")));
        }
        // Child::kill_on_dropは、このクライアントが起動した子だけを終了する。
    }
}

async fn collect_reader(
    reader: &mut Option<JoinHandle<Result<()>>>,
    label: &'static str,
) -> Result<()> {
    let Some(mut reader) = reader.take() else {
        return Ok(());
    };
    match tokio::time::timeout(Duration::from_secs(1), &mut reader).await {
        Ok(Ok(Ok(()))) => Ok(()),
        // reader自身が作る固定分類だけを保持し、生の通信内容は追加しない。
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) => Err(anyhow!("Native MCP {label} reader task failed")),
        Err(_) => {
            reader.abort();
            let _ = reader.await;
            Err(anyhow!("Native MCP {label} EOF collection timed out"))
        }
    }
}
