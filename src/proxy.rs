//! Native managed reverse SSH proxy and authenticated shared daemon client.
use crate::{
    error::{AppError, Result},
    proxy_egress,
    proxy_ipc::{self, Secret, Session},
};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use russh::{
    Channel, client,
    keys::{self, PrivateKeyWithHashAlg},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore, watch},
    task::JoinHandle,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub(crate) fn error(code: &str) -> AppError {
    let (code, message) = match code {
        "PROXY_CONFIG" => ("PROXY_CONFIG", "管理プロキシの設定が不正です。"),
        "PROXY_KEY_UNAVAILABLE" => (
            "PROXY_KEY_UNAVAILABLE",
            "管理プロキシの秘密鍵を読み取れません。",
        ),
        "PROXY_KEY_TOO_LARGE" => (
            "PROXY_KEY_TOO_LARGE",
            "管理プロキシの秘密鍵がサイズ上限を超えています。",
        ),
        "PROXY_HOST_KEY_MISMATCH" => (
            "PROXY_HOST_KEY_MISMATCH",
            "管理プロキシの接続先ホスト鍵が一致しません。",
        ),
        "PROXY_AUTH_FAILED" => ("PROXY_AUTH_FAILED", "管理プロキシのSSH認証に失敗しました。"),
        "PROXY_PORT_BUSY" => ("PROXY_PORT_BUSY", "管理プロキシのポートは使用中です。"),
        "PROXY_REMOTE_BIND_FAILED" => (
            "PROXY_REMOTE_BIND_FAILED",
            "管理プロキシの転送ポートを確保できません。",
        ),
        "PROXY_CONNECT_FAILED" => ("PROXY_CONNECT_FAILED", "管理プロキシに接続できません。"),
        "PROXY_TIMEOUT" => (
            "PROXY_TIMEOUT",
            "管理プロキシの準備が時間内に完了しませんでした。",
        ),
        "PROXY_CANCELLED" => (
            "PROXY_CANCELLED",
            "管理プロキシの準備待機を取り消しました。",
        ),
        "PROXY_STOPPED" => ("PROXY_STOPPED", "管理プロキシを停止しました。"),
        "PROXY_CLOSED" => ("PROXY_CLOSED", "共有プロキシの利用を終了しました。"),
        "PROXY_IPC_AUTH_FAILED" => (
            "PROXY_IPC_AUTH_FAILED",
            "共有プロキシとの通信を認証できません。",
        ),
        "PROXY_DAEMON_UNAVAILABLE" => (
            "PROXY_DAEMON_UNAVAILABLE",
            "共有プロキシの管理プロセスを利用できません。",
        ),
        "PROXY_START_FAILED" => (
            "PROXY_START_FAILED",
            "共有プロキシの管理プロセスを起動できません。",
        ),
        _ => ("PROXY_IPC_FAILED", "共有プロキシとの通信に失敗しました。"),
    };
    AppError::new(code, message)
}

pub fn normalize_proxy_config(v: &Value) -> Result<Value> {
    let fail = || error("PROXY_CONFIG");
    let host = v["sshHost"].as_str().ok_or_else(fail)?;
    let user = v["sshUser"].as_str().ok_or_else(fail)?;
    let key = v["privateKeyFile"].as_str().ok_or_else(fail)?;
    let pin = v["hostKeySha256"]
        .as_str()
        .and_then(|s| s.strip_prefix("SHA256:"))
        .ok_or_else(fail)?;
    let pin = pin.strip_suffix('=').unwrap_or(pin);
    let bytes = STANDARD_NO_PAD.decode(pin).map_err(|_| fail())?;
    if host.is_empty()
        || host.len() > 253
        || host.chars().any(|c| c.is_whitespace() || c.is_control())
        || user.is_empty()
        || user.len() > 64
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || !Path::new(key).is_absolute()
        || key.chars().any(|c| c == '\0')
        || bytes.len() != 32
        || STANDARD_NO_PAD.encode(&bytes) != pin
    {
        return Err(fail());
    }
    let port = |name: &str, min| -> Result<u64> {
        let p = v[name].as_u64().ok_or_else(fail)?;
        if p < min || p > 65535 {
            Err(fail())
        } else {
            Ok(p)
        }
    };
    let mut key_path = std::path::PathBuf::new();
    for component in Path::new(key).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                key_path.pop();
            }
            other => key_path.push(other.as_os_str()),
        }
    }
    let key = key_path.to_str().ok_or_else(fail)?;
    Ok(
        json!({"sshHost":host.to_ascii_lowercase(),"sshPort":port("sshPort",1)?,"sshUser":user,
        "privateKeyFile":key,"hostKeySha256":format!("SHA256:{pin}"),"localPort":port("localPort",1024)?,"remotePort":port("remotePort",1024)?}),
    )
}
async fn read_key(config: &Value) -> Result<Secret> {
    let path = config["privateKeyFile"]
        .as_str()
        .ok_or_else(|| error("PROXY_CONFIG"))?;
    if !tokio::fs::metadata(path)
        .await
        .map_err(|_| error("PROXY_KEY_UNAVAILABLE"))?
        .is_file()
    {
        return Err(error("PROXY_KEY_UNAVAILABLE"));
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let mut file = options
        .open(path)
        .await
        .map_err(|_| error("PROXY_KEY_UNAVAILABLE"))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|_| error("PROXY_KEY_UNAVAILABLE"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(error("PROXY_KEY_UNAVAILABLE"));
    }
    if metadata.len() > 1024 * 1024 {
        return Err(error("PROXY_KEY_TOO_LARGE"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    (&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| error("PROXY_KEY_UNAVAILABLE"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(error("PROXY_KEY_TOO_LARGE"));
    }
    if bytes.is_empty() {
        return Err(error("PROXY_KEY_UNAVAILABLE"));
    }
    Ok(bytes)
}
async fn secret(config: &Value) -> Result<Secret> {
    let key = read_key(config).await?;
    let mut hash = Sha256::new();
    hash.update(b"Plumivox native shared proxy IPC v3\0");
    hash.update(&*key);
    Ok(Zeroizing::new(hash.finalize().to_vec()))
}
fn context(config: &Value) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(config).expect("JSON config"),
    ))
}
fn socket_path(config: &Value) -> Result<String> {
    let id = context(config);
    #[cfg(windows)]
    {
        Ok(format!(r"\\.\pipe\plumivox-native-v3-{id}"))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        // A fixed private directory avoids socket paths supplied by bootstrap or peers.
        #[cfg(target_os = "macos")]
        let base = std::path::PathBuf::from("/private/tmp");
        #[cfg(not(target_os = "macos"))]
        let base = std::env::temp_dir();
        let dir = base.join(format!("plumivox-native-{}", unsafe { libc::geteuid() }));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(error("PROXY_START_FAILED")),
        }
        let m = std::fs::symlink_metadata(&dir).map_err(|_| error("PROXY_START_FAILED"))?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
        {
            return Err(error("PROXY_START_FAILED"));
        }
        Ok(dir
            .join(format!("{}.sock", &id[..40]))
            .to_string_lossy()
            .into_owned())
    }
}
fn stopped() -> Value {
    json!({"state":"stopped","managed":false,"activeConnections":0,"lastErrorCode":null})
}

struct Record {
    config: Value,
    secret: Secret,
    session: Option<Session>,
    bound: bool,
    launch: Option<Launch>,
}
struct Launch {
    result: watch::Receiver<Option<Result<()>>>,
    exited: CancellationToken,
}
impl Launch {
    async fn completed(&mut self) -> Result<()> {
        loop {
            if let Some(result) = self.result.borrow().clone() {
                return result;
            }
            self.result
                .changed()
                .await
                .map_err(|_| error("PROXY_START_FAILED"))?;
        }
    }
}
struct ClientInner {
    records: std::collections::HashMap<String, Record>,
    closed: bool,
}
#[derive(Default)]
struct Selection {
    last: Option<String>,
    states: std::collections::HashMap<String, ReadyState>,
}
struct ReadyState {
    token: CancellationToken,
    stops: Arc<AtomicUsize>,
}
#[derive(Clone)]
pub struct SharedProxyManager {
    inner: Arc<Mutex<ClientInner>>,
    closed: CancellationToken,
    selection: Arc<std::sync::Mutex<Selection>>,
}
impl Default for SharedProxyManager {
    fn default() -> Self {
        Self::new()
    }
}
impl SharedProxyManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ClientInner {
                records: std::collections::HashMap::new(),
                closed: false,
            })),
            closed: CancellationToken::new(),
            selection: Arc::new(std::sync::Mutex::new(Selection::default())),
        }
    }
    pub async fn ensure_ready(&self, config: &Value, cancel: CancellationToken) -> Result<Value> {
        if config.is_null() {
            return Ok(json!({"managed":false,"state":"external"}));
        }
        let config = normalize_proxy_config(config)?;
        let id = context(&config);
        let (generation, stops) = {
            let mut selected = self.selection.lock().unwrap_or_else(|p| p.into_inner());
            selected.last = Some(id.clone());
            let state = selected.states.entry(id).or_insert_with(|| ReadyState {
                token: CancellationToken::new(),
                stops: Arc::new(AtomicUsize::new(0)),
            });
            (state.token.clone(), state.stops.clone())
        };
        if stops.load(Ordering::SeqCst) > 0 {
            return Err(error("PROXY_STOPPED"));
        }
        tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(error("PROXY_CANCELLED")),
            _=self.closed.cancelled()=>Err(error("PROXY_CLOSED")),
            _=generation.cancelled()=>Err(error("PROXY_STOPPED")),
            result=timeout(startup_duration(),self.ready_inner(&config))=>result.map_err(|_|error("PROXY_TIMEOUT")).and_then(|r|r),
        }
    }
    pub async fn start(&self, config: &Value, cancel: CancellationToken) -> Result<Value> {
        self.ensure_ready(config, cancel).await
    }
    async fn ready_inner(&self, config: &Value) -> Result<Value> {
        let config = normalize_proxy_config(config)?;
        let key = secret(&config).await?;
        let mut inner = self.inner.lock().await;
        if inner.closed {
            return Err(error("PROXY_CLOSED"));
        }
        let id = context(&config);
        if !inner.records.get(&id).is_some_and(|r| *r.secret == *key) {
            inner.records.insert(
                id.clone(),
                Record {
                    config,
                    secret: key,
                    session: None,
                    bound: false,
                    launch: None,
                },
            );
        }
        let record = inner.records.get_mut(&id).unwrap();
        if record
            .launch
            .as_ref()
            .is_some_and(|l| l.exited.is_cancelled())
        {
            record.launch = None;
            record.session = None;
            record.bound = false;
        }
        let deadline = Instant::now() + startup_duration();
        let path = socket_path(&record.config)?;
        let mut spawned = record.launch.is_some();
        if let Some(launch) = record.launch.as_mut() {
            launch.completed().await?;
        }
        loop {
            if record.session.is_none() {
                let connection = timeout(Duration::from_secs(3), async {
                    Session::client(
                        proxy_ipc::connect(&path).await?,
                        context(&record.config),
                        &record.secret,
                    )
                    .await
                })
                .await
                .map_err(|_| error("PROXY_TIMEOUT"))?;
                match connection {
                    Ok(session) => {
                        record.bound = true;
                        record.session = Some(session);
                    }
                    Err(e) if e.code == "PROXY_DAEMON_UNAVAILABLE" => {
                        if !spawned {
                            record.bound = false;
                            let (done, result) = watch::channel(None);
                            let exited = CancellationToken::new();
                            let exit_token = exited.clone();
                            let config = record.config.clone();
                            // Launch ownership survives cancellation of an individual ready waiter.
                            tokio::spawn(async move {
                                done.send_replace(Some(spawn_daemon(&config, exit_token).await));
                            });
                            record.launch = Some(Launch { result, exited });
                            record.launch.as_mut().unwrap().completed().await?;
                            spawned = true;
                            // A concurrent daemon may have won bind while our child exited.
                            // Always try that shared endpoint before interpreting child exit.
                            continue;
                        }
                        if Instant::now() >= deadline {
                            return Err(error("PROXY_DAEMON_UNAVAILABLE"));
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            // Take ownership so cancellation or a malformed reply always detaches this channel.
            let mut session = record.session.take().unwrap();
            let result = timeout(
                deadline.saturating_duration_since(Instant::now()),
                session.request("ready", &record.secret),
            )
            .await
            .map_err(|_| error("PROXY_TIMEOUT"))?;
            match result {
                Ok(status) => {
                    record.session = Some(session);
                    if status["state"] != "ready" {
                        return Err(error("PROXY_CONNECT_FAILED"));
                    }
                    return Ok(status);
                }
                Err(e) => return Err(e),
            }
        }
    }
    pub async fn get_status(&self) -> Result<Value> {
        let id = self
            .selection
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last
            .clone();
        self.operation("status", id).await
    }
    pub async fn stop(&self) -> Result<Value> {
        let (id, stops) = {
            let mut selected = self.selection.lock().unwrap_or_else(|p| p.into_inner());
            let Some(id) = selected.last.clone() else {
                return Ok(stopped());
            };
            let state = selected.states.get_mut(&id).unwrap();
            state.stops.fetch_add(1, Ordering::SeqCst);
            state.token.cancel();
            state.token = CancellationToken::new();
            (id, state.stops.clone())
        };
        let _guard = StopGuard(stops);
        let result = self.operation("stop", Some(id.clone())).await;
        if result.is_ok() {
            let mut selected = self.selection.lock().unwrap_or_else(|p| p.into_inner());
            if selected.last.as_ref() == Some(&id) {
                selected.last = None;
            }
        }
        result
    }
    async fn operation(&self, op: &str, id: Option<String>) -> Result<Value> {
        let mut inner = self.inner.lock().await;
        if inner.closed {
            return Ok(stopped());
        }
        let Some(id) = id else {
            return Ok(stopped());
        };
        let Some(record) = inner.records.get_mut(&id) else {
            return Ok(stopped());
        };
        let mut session = match record.session.take() {
            Some(s) => s,
            None => {
                let path = socket_path(&record.config)?;
                let deadline = Instant::now() + startup_duration();
                loop {
                    match timeout(Duration::from_secs(3), async {
                        Session::client(
                            proxy_ipc::connect(&path).await?,
                            context(&record.config),
                            &record.secret,
                        )
                        .await
                    })
                    .await
                    {
                        Ok(Ok(s)) => {
                            record.bound = true;
                            break s;
                        }
                        Ok(Err(e)) if e.code == "PROXY_DAEMON_UNAVAILABLE" => {
                            let pending = record.launch.is_some() && !record.bound;
                            if op == "stop" && pending {
                                if Instant::now() >= deadline {
                                    return Err(error("PROXY_TIMEOUT"));
                                }
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                continue;
                            }
                            return Ok(stopped());
                        }
                        Ok(Err(e)) => return Err(e),
                        Err(_) => return Err(error("PROXY_TIMEOUT")),
                    }
                }
            }
        };
        let result = timeout(Duration::from_secs(10), session.request(op, &record.secret))
            .await
            .map_err(|_| error("PROXY_TIMEOUT"))?;
        if op == "status"
            && result.as_ref().is_err_and(|e| e.code == "PROXY_IPC_FAILED")
            && record.bound
        {
            let path = socket_path(&record.config)?;
            if let Err(e) = proxy_ipc::connect(&path).await {
                if e.code == "PROXY_DAEMON_UNAVAILABLE" {
                    return Ok(stopped());
                }
            }
        }
        if result.is_ok() && op != "stop" {
            record.session = Some(session);
        }
        if result.is_ok() && op == "stop" {
            // A stopped daemon is exiting. Do not reconnect to its draining pipe for status.
            inner.records.remove(&id);
        }
        result
    }
    pub async fn close(&self) -> Result<()> {
        self.closed.cancel();
        let mut inner = self.inner.lock().await;
        inner.closed = true;
        inner.records.clear();
        self.selection
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last = None;
        Ok(())
    }
}
struct StopGuard(Arc<AtomicUsize>);
impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct DaemonSpawnError {
    // 継承フラグ復元に失敗した場合も、bootstrap前の自分の子だけを回収する。
    child: Option<tokio::process::Child>,
}

#[cfg(not(windows))]
fn spawn_daemon_process(
    command: std::process::Command,
) -> std::result::Result<tokio::process::Child, DaemonSpawnError> {
    tokio::process::Command::from(command)
        .spawn()
        .map_err(|_| DaemonSpawnError { child: None })
}

#[cfg(windows)]
fn spawn_daemon_process(
    command: std::process::Command,
) -> std::result::Result<tokio::process::Child, DaemonSpawnError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };

    // stdioの継承フラグはプロセス全体の状態なので、daemon生成同士を直列化する。
    // この同期関数から戻った後にだけ、呼出し元は子のI/Oや終了をawaitする。
    static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _spawn = SPAWN.lock().map_err(|_| DaemonSpawnError { child: None })?;
    let mut handles = Vec::new();
    for handle in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if handle.is_null()
            || handle == INVALID_HANDLE_VALUE
            || handles.iter().any(|(seen, _)| *seen == handle)
        {
            continue;
        }
        let mut flags = 0;
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            return Err(DaemonSpawnError { child: None });
        }
        handles.push((handle, flags));
    }
    let suppressed = handles.iter().all(|(handle, flags)| {
        flags & HANDLE_FLAG_INHERIT == 0
            || unsafe { SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, 0) } != 0
    });
    let attempt = if suppressed {
        Some(tokio::process::Command::from(command).spawn())
    } else {
        None
    };
    // 一つの復元が失敗しても残るハンドルをすべて復元し、成功として返さない。
    let mut restored = true;
    for (handle, flags) in handles {
        if flags & HANDLE_FLAG_INHERIT != 0
            && unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags & HANDLE_FLAG_INHERIT)
            } == 0
        {
            restored = false;
        }
    }
    if !restored {
        return Err(DaemonSpawnError {
            child: attempt.and_then(std::result::Result::ok),
        });
    }
    match attempt {
        Some(Ok(child)) => Ok(child),
        _ => Err(DaemonSpawnError { child: None }),
    }
}

async fn spawn_daemon(config: &Value, exited: CancellationToken) -> Result<()> {
    let exe = std::env::current_exe().map_err(|_| {
        exited.cancel();
        error("PROXY_START_FAILED")
    })?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("--proxy-daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200 | 0x00000008);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = match spawn_daemon_process(command) {
        Ok(c) => c,
        Err(failed) => {
            if let Some(mut child) = failed.child {
                drop(child.stdin.take());
                // 元stdioの復元に失敗した子へ設定や秘密を渡さない。
                let _killed = child.start_kill();
                if matches!(
                    timeout(Duration::from_secs(2), child.wait()).await,
                    Ok(Ok(_))
                ) {
                    exited.cancel();
                } else {
                    // 終了を確認できない場合は監視を残し、終了済みと断定しない。
                    tokio::spawn(async move {
                        if child.wait().await.is_ok() {
                            exited.cancel();
                        }
                    });
                }
            } else {
                exited.cancel();
            }
            return Err(error("PROXY_START_FAILED"));
        }
    };
    let input = child.stdin.take();
    tokio::spawn(async move {
        let _ = child.wait().await;
        exited.cancel();
    });
    let bytes = serde_json::to_vec(&json!({"config":config})).map_err(|_| error("PROXY_CONFIG"))?;
    if bytes.len() > 8192 {
        return Err(error("PROXY_CONFIG"));
    }
    let mut input = input.ok_or_else(|| error("PROXY_START_FAILED"))?;
    input
        .write_all(&bytes)
        .await
        .map_err(|_| error("PROXY_START_FAILED"))?;
    input
        .shutdown()
        .await
        .map_err(|_| error("PROXY_START_FAILED"))?;
    drop(input);
    Ok(())
}

struct ForwardHandler {
    pin: String,
    rejected: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    remote: u32,
    local: u16,
    cancel: CancellationToken,
    connections: Arc<Semaphore>,
}
impl client::Handler for ForwardHandler {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &keys::PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let valid = key.certificate().is_none()
            && key
                .public_key()
                .fingerprint(keys::HashAlg::Sha256)
                .to_string()
                == self.pin;
        if !valid {
            self.rejected.store(true, Ordering::SeqCst);
        }
        Ok(valid)
    }
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<client::Msg>,
        address: &str,
        port: u32,
        origin: &str,
        _origin_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> std::result::Result<(), Self::Error> {
        if !self.ready.load(Ordering::SeqCst)
            || self.cancel.is_cancelled()
            || address != "127.0.0.1"
            || port != self.remote
            || !origin
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.is_loopback())
        {
            drop(reply);
            return Ok(());
        }
        let Ok(permit) = self.connections.clone().try_acquire_owned() else {
            drop(reply);
            return Ok(());
        };
        reply.accept().await;
        let cancel = self.cancel.clone();
        let port = self.local;
        tokio::spawn(async move {
            let _permit = permit;
            let mut stream = channel.into_stream();
            tokio::select! {_ = cancel.cancelled()=>{},_ = async{
                if let Ok(Ok(mut socket))=timeout(Duration::from_secs(15),TcpStream::connect(("127.0.0.1",port))).await {let _=proxy_egress::relay(&mut stream,&mut socket).await;}
            }=>{}}
        });
        Ok(())
    }
}

struct Core {
    config: Value,
    status: watch::Sender<Value>,
    active: Arc<AtomicUsize>,
    running: Mutex<Option<(CancellationToken, JoinHandle<()>)>>,
}
impl Core {
    fn new(config: Value) -> Arc<Self> {
        let (status, _) = watch::channel(stopped());
        Arc::new(Self {
            config,
            status,
            active: Arc::new(AtomicUsize::new(0)),
            running: Mutex::new(None),
        })
    }
    fn status(&self) -> Value {
        let mut v = self.status.borrow().clone();
        v["activeConnections"] = json!(self.active.load(Ordering::Relaxed).min(16));
        v
    }
    fn publish(&self, state: &str, code: Option<&str>) {
        self.status.send_replace(json!({"state":state,"managed":state!="stopped","activeConnections":0,"lastErrorCode":code}));
    }
    async fn ensure(self: &Arc<Self>) -> Result<Value> {
        {
            let mut running = self.running.lock().await;
            if running.is_none() {
                self.publish("starting", None);
                let cancel = CancellationToken::new();
                let core = self.clone();
                let token = cancel.clone();
                *running = Some((
                    cancel,
                    tokio::spawn(async move {
                        core.run(token).await;
                    }),
                ));
            }
        }
        let mut status = self.status.subscribe();
        loop {
            let value = status.borrow().clone();
            match value["state"].as_str() {
                Some("ready") => return Ok(self.status()),
                Some("failed") => {
                    return Err(error(
                        value["lastErrorCode"]
                            .as_str()
                            .unwrap_or("PROXY_CONNECT_FAILED"),
                    ));
                }
                Some("stopped") => return Err(error("PROXY_STOPPED")),
                _ => {}
            }
            status.changed().await.map_err(|_| error("PROXY_STOPPED"))?;
        }
    }
    async fn stop(&self) -> Result<Value> {
        let mut running = self.running.lock().await;
        if let Some((cancel, mut task)) = running.take() {
            cancel.cancel();
            if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
        self.publish("stopped", None);
        Ok(stopped())
    }
    async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let mut retry = 0u32;
        loop {
            if cancel.is_cancelled() {
                return;
            }
            let result = self.attempt(cancel.clone()).await;
            if self.status.borrow()["state"] == "ready" {
                retry = 0;
            }
            if cancel.is_cancelled() {
                return;
            }
            let code = result
                .err()
                .map(|e| e.code)
                .unwrap_or_else(|| "PROXY_CONNECT_FAILED".into());
            if retry == 6
                || [
                    "PROXY_KEY_UNAVAILABLE",
                    "PROXY_KEY_TOO_LARGE",
                    "PROXY_HOST_KEY_MISMATCH",
                    "PROXY_AUTH_FAILED",
                    "PROXY_PORT_BUSY",
                    "PROXY_REMOTE_BIND_FAILED",
                ]
                .contains(&code.as_str())
            {
                self.publish("failed", Some(&code));
                return;
            }
            self.publish("reconnecting", Some(&code));
            tokio::select! {_=cancel.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs((1u64<<retry).min(30)))=>{}}
            retry += 1;
        }
    }
    async fn attempt(&self, cancel: CancellationToken) -> Result<()> {
        let bytes = read_key(&self.config).await?;
        let key = keys::decode_secret_key(
            std::str::from_utf8(&bytes).map_err(|_| error("PROXY_AUTH_FAILED"))?,
            None,
        )
        .map_err(|_| error("PROXY_AUTH_FAILED"))?;
        drop(bytes);
        self.connected_attempt(cancel, key).await
    }
    async fn connected_attempt(
        &self,
        cancel: CancellationToken,
        key: keys::PrivateKey,
    ) -> Result<()> {
        let local = self.config["localPort"].as_u64().unwrap() as u16;
        let remote = self.config["remotePort"].as_u64().unwrap() as u32;
        let listener = TcpListener::bind(("127.0.0.1", local)).await.map_err(|e| {
            error(if e.kind() == std::io::ErrorKind::AddrInUse {
                "PROXY_PORT_BUSY"
            } else {
                "PROXY_CONNECT_FAILED"
            })
        })?;
        let attempt_cancel = cancel.child_token();
        let egress_cancel = attempt_cancel.clone();
        let active = self.active.clone();
        let egress = tokio::spawn(async move {
            proxy_egress::serve(listener, egress_cancel, active).await;
        });
        let ready = Arc::new(AtomicBool::new(false));
        let rejected = Arc::new(AtomicBool::new(false));
        let handler = ForwardHandler {
            pin: self.config["hostKeySha256"].as_str().unwrap().into(),
            rejected: rejected.clone(),
            ready: ready.clone(),
            remote,
            local,
            cancel: attempt_cancel.clone(),
            connections: Arc::new(Semaphore::new(16)),
        };
        let ssh_config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(15)),
            keepalive_max: 3,
            window_size: 65536,
            maximum_packet_size: 32768,
            channel_buffer_size: 4,
            ..Default::default()
        });
        let setup = async {
            let mut ssh = client::connect(
                ssh_config,
                (
                    self.config["sshHost"].as_str().unwrap(),
                    self.config["sshPort"].as_u64().unwrap() as u16,
                ),
                handler,
            )
            .await
            .map_err(|_| {
                error(if rejected.load(Ordering::SeqCst) {
                    "PROXY_HOST_KEY_MISMATCH"
                } else {
                    "PROXY_CONNECT_FAILED"
                })
            })?;
            let hash = ssh
                .best_supported_rsa_hash()
                .await
                .map_err(|_| error("PROXY_AUTH_FAILED"))?
                .flatten();
            let auth = ssh
                .authenticate_publickey(
                    self.config["sshUser"].as_str().unwrap(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
                .map_err(|_| error("PROXY_AUTH_FAILED"))?;
            if !auth.success() {
                return Err(error("PROXY_AUTH_FAILED"));
            }
            let bound = ssh
                .tcpip_forward("127.0.0.1", remote)
                .await
                .map_err(|_| error("PROXY_REMOTE_BIND_FAILED"))?;
            // RFC 4254: fixed-port success has no payload; russh represents it as zero.
            if bound != 0 && bound != remote {
                return Err(error("PROXY_REMOTE_BIND_FAILED"));
            }
            Ok(ssh)
        };
        let result = tokio::select! {_=cancel.cancelled()=>Err(error("PROXY_CANCELLED")),result=timeout(Duration::from_secs(10),setup)=>result.map_err(|_|error("PROXY_TIMEOUT")).and_then(|r|r)};
        let result = match result {
            Ok(mut ssh) => {
                ready.store(true, Ordering::SeqCst);
                self.publish("ready", None);
                let ended = tokio::select! {_=cancel.cancelled()=>false,_= &mut ssh=>true};
                ready.store(false, Ordering::SeqCst);
                if !ended {
                    let _ = timeout(
                        Duration::from_secs(1),
                        ssh.cancel_tcpip_forward("127.0.0.1", remote),
                    )
                    .await;
                    let _ = timeout(
                        Duration::from_secs(1),
                        ssh.disconnect(russh::Disconnect::ByApplication, "", "en"),
                    )
                    .await;
                    let _ = timeout(Duration::from_secs(1), &mut ssh).await;
                }
                Err(error("PROXY_CONNECT_FAILED"))
            }
            Err(e) => Err(e),
        };
        attempt_cancel.cancel();
        let _ = egress.await;
        result
    }
}

pub async fn run_daemon(bootstrap: Value) -> Result<()> {
    if !bootstrap
        .as_object()
        .is_some_and(|o| o.len() == 1 && o.contains_key("config"))
    {
        return Err(error("PROXY_CONFIG"));
    }
    let config = normalize_proxy_config(&bootstrap["config"])?;
    let path = socket_path(&config)?;
    let binding = context(&config);
    let key = Arc::new(secret(&config).await?);
    #[cfg(feature = "test-fixture")]
    if let Some(gate) = std::env::var_os("PLUMIVOX_PROXY_TEST_BIND_GATE") {
        let gate = std::path::PathBuf::from(gate);
        let mut marker = gate.as_os_str().to_os_string();
        marker.push(format!(".{}.waiting", std::process::id()));
        let mut marker = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(std::path::PathBuf::from(marker))
            .await
            .map_err(|_| error("PROXY_START_FAILED"))?;
        marker
            .write_all(b"waiting")
            .await
            .map_err(|_| error("PROXY_START_FAILED"))?;
        drop(marker);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !gate.with_extension("release").exists() {
            if Instant::now() >= deadline {
                return Err(error("PROXY_TIMEOUT"));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let mut listener = proxy_ipc::Listener::bind(&path)?;
    let core = Core::new(config);
    let shutdown = CancellationToken::new();
    let stopping = Arc::new(AtomicBool::new(false));
    let authenticated = Arc::new(AtomicUsize::new(0));
    let clients = Arc::new(Semaphore::new(64));
    let (departure, mut departures) = tokio::sync::mpsc::channel::<()>(64);
    let idle_duration = daemon_idle_duration();
    let mut idle = Instant::now() + idle_duration;
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _=shutdown.cancelled()=>break,
            _=tokio::time::sleep_until(idle),if authenticated.load(Ordering::SeqCst)==0=>break,
            Some(())=departures.recv()=>{if authenticated.load(Ordering::SeqCst)==0{idle=Instant::now()+idle_duration;}},
            Some(_)=tasks.join_next(),if !tasks.is_empty()=>{},
            accepted=listener.accept()=>{
                let pipe=match accepted{Ok(p)=>p,Err(e) if e.code=="PROXY_IPC_AUTH_FAILED"=>continue,Err(e)=>{core.stop().await?;return Err(e);}};
                let Ok(permit)=clients.clone().try_acquire_owned() else{continue};
                let key=key.clone();let binding=binding.clone();let core=core.clone();let shutdown=shutdown.clone();let authenticated=authenticated.clone();let departure=departure.clone();let stopping=stopping.clone();
                tasks.spawn(async move{
                    let _permit=permit;let mut counted=false;
                    let handling=async{
                        let mut session=timeout(Duration::from_secs(3),Session::server(pipe,binding,&key)).await.map_err(|_|error("PROXY_TIMEOUT"))??;
                        loop {
                            let operation=if counted {session.receive(&key).await?}else{timeout(Duration::from_secs(3),session.receive(&key)).await.map_err(|_|error("PROXY_TIMEOUT"))??};
                            if !counted{
                                authenticated.fetch_add(1,Ordering::SeqCst);counted=true;
                                // Wake the listener to disable an idle timer selected before authentication.
                                let _=departure.try_send(());
                            }
                            let result=match operation.as_str(){
                                "ready" if stopping.load(Ordering::SeqCst)=>Err(error("PROXY_STOPPED")),
                                "ready"=>timeout(Duration::from_secs(12),core.ensure()).await.map_err(|_|error("PROXY_TIMEOUT")).and_then(|r|r),
                                "status"=>Ok(core.status()),
                                "stop"=>{stopping.store(true,Ordering::SeqCst);core.stop().await},
                                _=>Err(error("PROXY_IPC_FAILED"))
                            };
                            let response=timeout(Duration::from_secs(3),session.respond(&operation,result,&key)).await;
                            if operation=="stop"{
                                // Windows pipes may discard unread writes on server close. The client
                                // drops its session after consuming the stop result; wait for that EOF.
                                if matches!(response,Ok(Ok(()))) {let _=timeout(Duration::from_secs(1),proxy_ipc::read(&mut session.pipe)).await;}
                                shutdown.cancel();return Ok::<(),AppError>(());
                            }
                            response.map_err(|_|error("PROXY_TIMEOUT"))??;
                        }
                    };
                    tokio::select!{_=shutdown.cancelled()=>{},_=handling=>{}}
                    if counted{authenticated.fetch_sub(1,Ordering::SeqCst);let _=departure.send(()).await;}
                });
            }
        }
    }
    shutdown.cancel();
    core.stop().await?;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}

fn daemon_idle_duration() -> Duration {
    #[cfg(feature = "test-fixture")]
    if let Some(ms) = std::env::var("PLUMIVOX_PROXY_TEST_IDLE_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| (100..=300000).contains(n))
    {
        return Duration::from_millis(ms);
    }
    Duration::from_secs(300)
}
fn startup_duration() -> Duration {
    #[cfg(feature = "test-fixture")]
    if let Some(ms) = std::env::var("PLUMIVOX_PROXY_TEST_STARTUP_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| (500..=30000).contains(n))
    {
        return Duration::from_millis(ms);
    }
    Duration::from_secs(30)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn fifo_private_key_is_rejected_without_waiting_for_writer() {
        use std::os::unix::fs::DirBuilderExt;
        let directory =
            std::env::temp_dir().join(format!("plumivox-key-test-{}", proxy_ipc::nonce()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let fifo = directory.join("key-fifo");
        let cpath = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let result = timeout(
            Duration::from_secs(2),
            read_key(&json!({"privateKeyFile":fifo})),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(result.code, "PROXY_KEY_UNAVAILABLE");
        std::fs::remove_file(fifo).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    use russh::server;
    #[derive(Clone)]
    struct FakeSsh {
        accept_auth: bool,
        accept_forward: bool,
        auth: Arc<AtomicUsize>,
        unforward: Arc<AtomicUsize>,
        ready: tokio::sync::mpsc::Sender<server::Handle>,
    }
    impl server::Handler for FakeSsh {
        type Error = russh::Error;
        async fn auth_publickey(
            &mut self,
            user: &str,
            _key: server::VerifiedPublicKey<'_>,
        ) -> std::result::Result<server::Auth, Self::Error> {
            self.auth.fetch_add(1, Ordering::SeqCst);
            assert_eq!(user, "fixture");
            Ok(if self.accept_auth {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            })
        }
        async fn tcpip_forward(
            &mut self,
            address: &str,
            _port: &mut u32,
            session: &mut server::Session,
        ) -> std::result::Result<bool, Self::Error> {
            assert_eq!(address, "127.0.0.1");
            if self.accept_forward {
                let _ = self.ready.send(session.handle()).await;
            }
            Ok(self.accept_forward)
        }
        async fn cancel_tcpip_forward(
            &mut self,
            address: &str,
            _port: u32,
            _session: &mut server::Session,
        ) -> std::result::Result<bool, Self::Error> {
            assert_eq!(address, "127.0.0.1");
            self.unforward.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }
    fn fixture_key(seed: u8) -> keys::PrivateKey {
        let pair = keys::ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]);
        keys::PrivateKey::new(pair.into(), "isolated test key").unwrap()
    }
    async fn fixture(
        mismatch: bool,
        accept_auth: bool,
        accept_forward: bool,
    ) -> (
        Arc<Core>,
        CancellationToken,
        JoinHandle<Result<()>>,
        tokio::sync::mpsc::Receiver<server::Handle>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        JoinHandle<()>,
    ) {
        let key = fixture_key(1);
        let host = fixture_key(2);
        let socket = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let ssh_port = socket.local_addr().unwrap().port();
        let reserve = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let local = reserve.local_addr().unwrap().port();
        drop(reserve);
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        let auth = Arc::new(AtomicUsize::new(0));
        let unforward = Arc::new(AtomicUsize::new(0));
        let fake = FakeSsh {
            accept_auth,
            accept_forward,
            auth: auth.clone(),
            unforward: unforward.clone(),
            ready: sender,
        };
        let pin = if mismatch {
            fixture_key(3)
                .public_key()
                .fingerprint(keys::HashAlg::Sha256)
                .to_string()
        } else {
            host.public_key()
                .fingerprint(keys::HashAlg::Sha256)
                .to_string()
        };
        let config = Arc::new(server::Config {
            keys: vec![host],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let server = tokio::spawn(async move {
            if let Ok((stream, _)) = socket.accept().await {
                if let Ok(session) = server::run_stream(config, stream, fake).await {
                    let _ = session.await;
                }
            }
        });
        let core = Core::new(
            json!({"sshHost":"127.0.0.1","sshPort":ssh_port,"sshUser":"fixture","localPort":local,"remotePort":43128,"hostKeySha256":pin}),
        );
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task_core = core.clone();
        let task = tokio::spawn(async move { task_core.connected_attempt(token, key).await });
        (core, cancel, task, receiver, auth, unforward, server)
    }
    #[tokio::test]
    async fn ssh_pin_auth_and_remote_bind_fail_closed() {
        for (mismatch, auth_ok, forward_ok, expected) in [
            (true, true, true, "PROXY_HOST_KEY_MISMATCH"),
            (false, false, true, "PROXY_AUTH_FAILED"),
            (false, true, false, "PROXY_REMOTE_BIND_FAILED"),
        ] {
            let (_core, cancel, task, _events, auth, _unforward, server) =
                fixture(mismatch, auth_ok, forward_ok).await;
            let result = timeout(Duration::from_secs(8), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(result.code, expected);
            assert_eq!(auth.load(Ordering::SeqCst), if mismatch { 0 } else { 1 });
            cancel.cancel();
            server.abort();
            let _ = server.await;
        }
    }
    #[tokio::test]
    async fn ssh_reverse_reaches_connect_filter_and_unforwards_on_stop() {
        let (core, cancel, task, mut events, auth, unforward, server) =
            fixture(false, true, true).await;
        let handle = timeout(Duration::from_secs(8), events.recv())
            .await
            .unwrap()
            .unwrap();
        let mut statuses = core.status.subscribe();
        timeout(Duration::from_secs(3), async {
            while statuses.borrow()["state"] != "ready" {
                statuses.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        // Reject server-initiated channels whose claimed peer is not VPS loopback.
        assert!(
            handle
                .channel_open_forwarded_tcpip("127.0.0.1", 43128, "8.8.8.8", 1234)
                .await
                .is_err()
        );
        assert!(
            handle
                .channel_open_forwarded_tcpip("127.0.0.2", 43128, "127.0.0.1", 1234)
                .await
                .is_err()
        );
        let mut channel = handle
            .channel_open_forwarded_tcpip("127.0.0.1", 43128, "127.0.0.1", 1234)
            .await
            .unwrap()
            .into_stream();
        channel
            .write_all(b"CONNECT localhost:443 HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut response = [0; 128];
        let n = timeout(Duration::from_secs(3), channel.read(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&response[..n]).contains("403 Forbidden"));
        cancel.cancel();
        let _ = timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(auth.load(Ordering::SeqCst), 1);
        assert_eq!(unforward.load(Ordering::SeqCst), 1);
        let local = core.config["localPort"].as_u64().unwrap() as u16;
        assert!(TcpListener::bind(("127.0.0.1", local)).await.is_ok());
        server.abort();
        let _ = server.await;
    }
    #[test]
    fn configuration_rejects_ambiguous_values() {
        let path = if cfg!(windows) {
            r"C:\keys\proxy"
        } else {
            "/keys/proxy"
        };
        let v = json!({"sshHost":"EXAMPLE.com","sshPort":22,"sshUser":"pv-user","privateKeyFile":path,"hostKeySha256":format!("SHA256:{}",STANDARD_NO_PAD.encode([9u8;32])),"localPort":43128,"remotePort":43128});
        assert_eq!(
            normalize_proxy_config(&v).unwrap()["sshHost"],
            "example.com"
        );
        for (field, value) in [
            ("sshPort", json!(0)),
            ("localPort", json!(80)),
            ("sshUser", json!("user name")),
            ("privateKeyFile", json!("relative")),
            ("hostKeySha256", json!("SHA256:invalid")),
            ("sshHost", json!("host\nname")),
        ] {
            let mut c = v.clone();
            c[field] = value;
            assert!(normalize_proxy_config(&c).is_err(), "{field}");
        }
    }
    #[tokio::test]
    async fn external_call_preserves_manager_and_needs_no_key() {
        let m = SharedProxyManager::new();
        let canceled = CancellationToken::new();
        canceled.cancel();
        assert_eq!(
            m.ensure_ready(&Value::Null, canceled).await.unwrap()["state"],
            "external"
        );
        assert_eq!(m.get_status().await.unwrap(), stopped());
        m.close().await.unwrap();
    }
}
