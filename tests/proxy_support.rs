//! 共有プロキシ結合テスト専用の認証付きSSH/VPS。外向き接続は作らない。
use crate::support::{HttpServer, McpClient, Result, free_port, json_response};
use anyhow::{Context, ensure};
use russh::{Disconnect, keys, server};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::{JoinHandle, JoinSet},
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

pub const API_KEY: &str = "synthetic-native-proxy-api-key";
pub const COOKIE: &str = "synthetic-native-proxy-cookie";
pub const PASSWORD: &str = "synthetic-native-proxy-password";

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Accept,
    Stall,
    AuthReject,
    BindReject,
    UnforwardStall,
}

struct State {
    mode: AtomicU8,
    changed: Notify,
    attempts: AtomicUsize,
    active: AtomicUsize,
    stalled: AtomicUsize,
    unforwarded: AtomicUsize,
    public_key: keys::PublicKey,
    trace: Mutex<Vec<Value>>,
    clients: Mutex<HashMap<usize, (server::Handle, CancellationToken)>>,
}
impl State {
    fn mode(&self) -> u8 {
        self.mode.load(Ordering::SeqCst)
    }
    fn event(&self, value: Value) {
        self.trace.lock().unwrap().push(value);
    }
}

struct ActiveClient {
    state: Arc<State>,
    id: usize,
    life: CancellationToken,
}
impl Drop for ActiveClient {
    fn drop(&mut self) {
        self.life.cancel();
        self.state.clients.lock().unwrap().remove(&self.id);
        self.state.active.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Stalled(Arc<State>);
impl Drop for Stalled {
    fn drop(&mut self) {
        self.0.stalled.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Handler {
    state: Arc<State>,
    life: CancellationToken,
    forwards: HashMap<u32, Forward>,
}
struct Forward {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}
impl Drop for Handler {
    fn drop(&mut self) {
        self.life.cancel();
    }
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: server::VerifiedPublicKey<'_>,
    ) -> std::result::Result<server::Auth, Self::Error> {
        if self.state.mode() == Mode::Stall as u8 {
            self.state.stalled.fetch_add(1, Ordering::SeqCst);
            let _stalled = Stalled(self.state.clone());
            loop {
                let changed = self.state.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self.state.mode() != Mode::Stall as u8 {
                    break;
                }
                tokio::select! { _ = self.life.cancelled() => return Ok(server::Auth::reject()), _ = changed => {} }
            }
        }
        Ok(
            if self.state.mode() != Mode::AuthReject as u8
                && user == "fixture"
                && key.public_key == &self.state.public_key
            {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        self.state
            .event(json!({"type":"ssh-request","request":"tcpip-forward"}));
        if address != "127.0.0.1"
            || self.state.mode() == Mode::BindReject as u8
            || *port > u16::MAX as u32
        {
            return Ok(false);
        }
        let Ok(listener) = TcpListener::bind(("127.0.0.1", *port as u16)).await else {
            return Ok(false);
        };
        let remote = listener.local_addr().map_err(russh::Error::from)?.port();
        *port = remote.into();
        let cancel = self.life.child_token();
        let owned_cancel = cancel.clone();
        let handle = session.handle();
        let task = tokio::spawn(async move {
            let mut relays = JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    Some(_) = relays.join_next(), if !relays.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((mut stream, origin)) = accepted else { break };
                        let handle = handle.clone(); let cancel = cancel.clone();
                        relays.spawn(async move {
                            tokio::select! {
                                _ = cancel.cancelled() => {},
                                _ = async {
                                    if let Ok(channel) = handle.channel_open_forwarded_tcpip("127.0.0.1", remote.into(), origin.ip().to_string(), origin.port().into()).await {
                                        let mut channel = channel.into_stream();
                                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel).await;
                                    }
                                } => {}
                            }
                        });
                    }
                }
            }
            relays.abort_all();
            while relays.join_next().await.is_some() {}
        });
        self.forwards.insert(
            *port,
            Forward {
                cancel: owned_cancel,
                task,
            },
        );
        Ok(true)
    }
    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let stalled = self.state.mode() == Mode::UnforwardStall as u8;
        self.state.event(
            json!({"type":"ssh-request","request":"cancel-tcpip-forward",
            "address":address,"port":port,"mode":if stalled {"unforward-stall"} else {"normal"}}),
        );
        if stalled {
            // 応答期限を超えて保留し、有限のfixture寿命で回収する。
            tokio::select! {_=self.life.cancelled()=>{},_=tokio::time::sleep(Duration::from_secs(6))=>{}}
            return Ok(false);
        }
        if address != "127.0.0.1" {
            return Ok(false);
        }
        if let Some(forward) = self.forwards.remove(&port) {
            forward.cancel.cancel();
            let _ = forward.task.await;
        }
        self.state.unforwarded.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }
}

pub struct FakeSsh {
    pub port: u16,
    pub pin: String,
    pub pem: String,
    state: Arc<State>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
impl FakeSsh {
    pub async fn start() -> Result<Self> {
        // 元のssh2 fixtureと同じRSA公開鍵認証をRustだけで構築する。
        let pair = keys::ssh_key::private::RsaKeypair::random(&mut rand::rng(), 2048)?;
        let key = keys::PrivateKey::new(pair.into(), "")?;
        let pin = key
            .public_key()
            .fingerprint(keys::HashAlg::Sha256)
            .to_string();
        let pem = key.to_openssh(keys::ssh_key::LineEnding::LF)?.to_string();
        let state = Arc::new(State {
            mode: AtomicU8::new(Mode::Accept as u8),
            changed: Notify::new(),
            attempts: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            stalled: AtomicUsize::new(0),
            unforwarded: AtomicUsize::new(0),
            public_key: key.public_key().clone(),
            trace: Mutex::new(Vec::new()),
            clients: Mutex::new(HashMap::new()),
        });
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            let mut sessions = JoinSet::new();
            loop {
                tokio::select! {
                    _ = task_stop.cancelled() => break,
                    Some(_) = sessions.join_next(), if !sessions.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let id = task_state.attempts.fetch_add(1, Ordering::SeqCst) + 1;
                        task_state.active.fetch_add(1, Ordering::SeqCst);
                        task_state.event(json!({"type":"ssh-connection","attempts":id}));
                        let state = task_state.clone(); let config = config.clone(); let life = task_stop.child_token();
                        let active = ActiveClient { state: state.clone(), id, life: life.clone() };
                        sessions.spawn(async move {
                            let _active = active;
                            let handler = Handler { state: state.clone(), life: life.clone(), forwards: HashMap::new() };
                            let Ok(mut running) = server::run_stream(config, stream, handler).await else { return };
                            let handle = running.handle(); state.clients.lock().unwrap().insert(id, (handle.clone(), life.clone()));
                            let ended = tokio::select! { _ = life.cancelled() => false, _ = &mut running => true };
                            if !ended {
                                let _ = timeout(Duration::from_secs(1), handle.disconnect(Disconnect::ByApplication, String::new(), "en".into())).await;
                                let _ = timeout(Duration::from_secs(2), &mut running).await;
                            }
                        });
                    }
                }
            }
            task_stop.cancel();
            let _ = timeout(Duration::from_secs(4), async {
                while sessions.join_next().await.is_some() {}
            })
            .await;
            sessions.abort_all();
            while sessions.join_next().await.is_some() {}
        });
        Ok(Self {
            port,
            pin,
            pem,
            state,
            stop,
            task,
        })
    }
    pub fn set_mode(&self, mode: Mode) {
        self.state.mode.store(mode as u8, Ordering::SeqCst);
        self.state.changed.notify_waiters();
    }
    pub fn attempts(&self) -> usize {
        self.state.attempts.load(Ordering::SeqCst)
    }
    pub fn active(&self) -> usize {
        self.state.active.load(Ordering::SeqCst)
    }
    pub fn stalled(&self) -> usize {
        self.state.stalled.load(Ordering::SeqCst)
    }
    pub fn unforwarded(&self) -> usize {
        self.state.unforwarded.load(Ordering::SeqCst)
    }
    pub fn stalled_unforward_requests(&self, port: u16) -> usize {
        self.state
            .trace
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event["type"] == "ssh-request"
                    && event["request"] == "cancel-tcpip-forward"
                    && event["address"] == "127.0.0.1"
                    && event["port"].as_u64() == Some(u64::from(port))
                    && event["mode"] == "unforward-stall"
            })
            .count()
    }
    pub fn trace(&self) -> Value {
        json!(*self.state.trace.lock().unwrap())
    }
    pub async fn disconnect_all(&self) {
        let clients: Vec<_> = self
            .state
            .clients
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        for (handle, life) in clients {
            life.cancel();
            let _ = timeout(
                Duration::from_secs(1),
                handle.disconnect(Disconnect::ByApplication, String::new(), "en".into()),
            )
            .await;
        }
    }
    pub async fn close(&mut self) -> Result<()> {
        self.set_mode(Mode::Accept);
        self.stop.cancel();
        self.disconnect_all().await;
        timeout(Duration::from_secs(6), &mut self.task)
            .await
            .context("Fake SSH did not close")??;
        Ok(())
    }
}

pub struct Settings {
    pub file: PathBuf,
    pub local: u16,
    pub remote: u16,
    pub config: Value,
}
pub struct Harness {
    pub output: PathBuf,
    pub ssh: FakeSsh,
    pub requests: Arc<AtomicUsize>,
    pub clients: Vec<McpClient>,
    pub secrets: Vec<String>,
    pub key_file: PathBuf,
    api: HttpServer,
}
impl Harness {
    pub async fn new(output: &Path) -> Result<Self> {
        let ssh = FakeSsh::start().await?;
        let key_file = output.join("synthetic-key.pem");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&key_file)?.write_all(ssh.pem.as_bytes())?;
        let requests = Arc::new(AtomicUsize::new(0));
        let api_requests = requests.clone();
        let api = HttpServer::start(move |_| {
            let requests = api_requests.clone();
            async move {
                requests.fetch_add(1, Ordering::SeqCst);
                Ok(json_response(
                    200,
                    json!({"status":"success","tweet_id":"901"}),
                ))
            }
        })
        .await?;
        let secrets = vec![
            ssh.pem.clone(),
            API_KEY.into(),
            COOKIE.into(),
            PASSWORD.into(),
        ];
        Ok(Self {
            output: output.into(),
            ssh,
            requests,
            clients: Vec::new(),
            secrets,
            key_file,
            api,
        })
    }
    pub async fn settings(&self, name: &str, overrides: Value) -> Result<Settings> {
        let local = free_port()?;
        let remote = free_port()?;
        let mut relay = json!({"ssh_host":"127.0.0.1","ssh_port":self.ssh.port,"ssh_user":"fixture","private_key_file":self.key_file,"host_key_sha256":self.ssh.pin,"local_port":local,"remote_port":remote});
        if let Some(overrides) = overrides.as_object() {
            for (key, value) in overrides {
                relay[key] = value.clone();
            }
        }
        let config = json!({"twitterapi_io":{
            "managed":{"user_name":"Fixture","api_key":API_KEY,"proxy":format!("http://fixture:{PASSWORD}@127.0.0.1:{remote}"),"proxy_mode":"managed","login_cookies":COOKIE},
            "external":{"user_name":"External","api_key":API_KEY,"proxy":"http://127.0.0.1:8888","proxy_mode":"external","login_cookies":COOKIE}},"plumivox":{"managed_proxy":relay}});
        let file = self.output.join(format!("{name}.json"));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)?
            .write_all(&serde_json::to_vec(&config)?)?;
        Ok(Settings {
            file,
            local,
            remote,
            config,
        })
    }
    pub async fn client_for(
        &mut self,
        config: &Settings,
        extra: &[(String, String)],
    ) -> Result<usize> {
        let mut env = vec![
            (
                "PLUMIVOX_SECRETS_FILE".into(),
                config.file.to_string_lossy().into_owned(),
            ),
            ("PLUMIVOX_PROXY_TEST_IDLE_MS".into(), "2000".into()),
            ("PLUMIVOX_PROXY_TEST_STARTUP_MS".into(), "5000".into()),
        ];
        env.extend_from_slice(extra);
        let args = vec![
            self.api.base_url.clone(),
            config.file.to_string_lossy().into_owned(),
            self.output
                .join(format!("state-{}", self.clients.len()))
                .to_string_lossy()
                .into_owned(),
            "15000".into(),
        ];
        let client = McpClient::spawn_with_env(&args, &env).await?;
        let id = self.clients.len();
        self.clients.push(client);
        Ok(id)
    }
    pub fn business(&self, raw: &Value) -> Result<Value> {
        let serialized = serde_json::to_string(raw)?;
        ensure!(
            self.secrets
                .iter()
                .all(|secret| !serialized.contains(secret)),
            "MCP exposed a synthetic credential"
        );
        if let Some(data) = raw.get("structuredContent") {
            return Ok(data.clone());
        }
        let text = raw["content"]
            .as_array()
            .context("Missing MCP content")?
            .iter()
            .filter(|item| item["type"] == "text")
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        Ok(serde_json::from_str(&text)?)
    }
    pub async fn call(&self, client: usize, name: &str, args: Value) -> Result<(Value, Value)> {
        let raw = timeout(
            Duration::from_secs(20),
            self.clients[client].call_tool(name, args),
        )
        .await
        .context("Proxy MCP call timed out")??;
        let data = self.business(&raw)?;
        Ok((raw, data))
    }
    pub async fn success(&self, client: usize, name: &str, args: Value) -> Result<Value> {
        let (raw, data) = self.call(client, name, args).await?;
        ensure!(
            raw["isError"] != true,
            "MCP {name} failed: {}",
            data["error"]["code"]
        );
        Ok(data)
    }
    pub async fn expect_error(&self, client: usize, code: &str) -> Result<()> {
        let (raw, data) = self
            .call(client, "start_proxy", json!({"account":"managed"}))
            .await?;
        ensure!(
            raw["isError"] == true && data["error"]["code"] == code,
            "Expected {code}, received {}",
            data["error"]["code"]
        );
        Ok(())
    }
    pub async fn close_client(&mut self, client: usize) -> Result<()> {
        self.clients[client].close().await
    }
    pub async fn close(&mut self) -> Result<()> {
        self.ssh.set_mode(Mode::Accept);
        for id in 0..self.clients.len() {
            let _ = self.call(id, "stop_proxy", json!({})).await;
            let _ = self.clients[id].close().await;
        }
        self.ssh.close().await?;
        self.api.close().await
    }
    pub fn diagnostics(&self) -> Result<Value> {
        let diagnostics: Vec<_> = self.clients.iter().map(McpClient::stderr).collect();
        ensure!(
            diagnostics
                .iter()
                .all(|line| self.secrets.iter().all(|secret| !line.contains(secret))),
            "Diagnostics exposed a synthetic credential"
        );
        Ok(json!(diagnostics))
    }
}

pub fn state(value: &Value) -> &str {
    value["state"]
        .as_str()
        .or_else(|| value["proxy"]["state"].as_str())
        .or_else(|| value["status"]["state"].as_str())
        .unwrap_or("")
}
pub async fn rejected_tunnel(port: u16) -> Result<()> {
    rejected_tunnel_parts(port, &[b"CONNECT localhost:443 HTTP/1.1\r\n\r\n"], false).await
}
pub async fn rejected_tunnel_parts(port: u16, parts: &[&[u8]], eof: bool) -> Result<()> {
    timeout(Duration::from_secs(3), async {
        let mut socket = TcpStream::connect(("127.0.0.1", port)).await?;
        for (index, part) in parts.iter().enumerate() {
            socket.write_all(part).await?;
            if index + 1 < parts.len() || eof {
                // 次の断片やEOFを送る前に、未完ヘッダーが拒否されていないことを確認する。
                let mut byte = [0];
                ensure!(
                    timeout(Duration::from_millis(100), socket.read(&mut byte))
                        .await
                        .is_err(),
                    "Reverse CONNECT answered before the header boundary or EOF"
                );
            }
        }
        if eof {
            socket.shutdown().await?;
        }
        let mut response = Vec::new();
        let mut bytes = [0; 256];
        while !response.windows(4).any(|v| v == b"\r\n\r\n") {
            let n = socket.read(&mut bytes).await?;
            if n == 0 {
                break;
            }
            response.extend_from_slice(&bytes[..n]);
            ensure!(response.len() < 8192, "CONNECT response too large");
        }
        ensure!(
            String::from_utf8_lossy(&response).contains("403 Forbidden"),
            "Reverse CONNECT did not reach production filter"
        );
        Ok(())
    })
    .await
    .context("Reverse CONNECT did not answer")?
}
pub async fn active_is(ssh: &FakeSsh, count: usize) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while ssh.active() != count {
        ensure!(
            Instant::now() < deadline,
            "Unexpected active SSH count: {} instead of {count}",
            ssh.active()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(())
}
pub fn gate_count(output: &Path, gate: &Path) -> Result<usize> {
    let prefix = format!(
        "{}.",
        gate.file_name().context("Invalid gate")?.to_string_lossy()
    );
    Ok(fs::read_dir(output)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".waiting")
        })
        .count())
}
pub fn release_gate(gate: &Path) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(gate.with_extension("release"))?
        .write_all(b"release")?;
    Ok(())
}
