//! Native-only authenticated IPC. The wire namespace is separate from legacy JS daemons.
use crate::{error::Result, proxy::error};
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub(crate) type Pipe = Box<dyn Stream>;
pub(crate) type Secret = Zeroizing<Vec<u8>>;

pub(crate) fn nonce() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
fn valid_nonce(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let mut result = serde_json::Map::new();
            for key in keys {
                result.insert(key.clone(), canonical(&map[key]));
            }
            Value::Object(result)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        _ => v.clone(),
    }
}
fn message_mac(secret: &[u8], kind: &str, body: &Value) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any length");
    mac.update(
        &serde_json::to_vec(&canonical(&json!([
            "Plumivox native proxy IPC v3",
            kind,
            body
        ])))
        .expect("JSON value"),
    );
    mac
}
pub(crate) fn sign(secret: &[u8], kind: &str, body: &Value) -> String {
    hex::encode(message_mac(secret, kind, body).finalize().into_bytes())
}
fn verify(secret: &[u8], kind: &str, body: &Value, proof: &Value) -> bool {
    let Some(proof) = proof.as_str().and_then(|s| hex::decode(s).ok()) else {
        return false;
    };
    message_mac(secret, kind, body).verify_slice(&proof).is_ok()
}
fn exact(v: &Value, keys: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)))
}
pub(crate) async fn read(pipe: &mut Pipe) -> Result<Value> {
    let size = pipe
        .read_u32()
        .await
        .map_err(|_| error("PROXY_IPC_FAILED"))? as usize;
    if size == 0 || size > 8192 {
        return Err(error("PROXY_IPC_FAILED"));
    }
    let mut bytes = vec![0; size];
    pipe.read_exact(&mut bytes)
        .await
        .map_err(|_| error("PROXY_IPC_FAILED"))?;
    serde_json::from_slice(&bytes).map_err(|_| error("PROXY_IPC_FAILED"))
}
pub(crate) async fn write(pipe: &mut Pipe, frame: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(frame).map_err(|_| error("PROXY_IPC_FAILED"))?;
    if bytes.len() > 8192 {
        return Err(error("PROXY_IPC_FAILED"));
    }
    pipe.write_u32(bytes.len() as u32)
        .await
        .map_err(|_| error("PROXY_IPC_FAILED"))?;
    pipe.write_all(&bytes)
        .await
        .map_err(|_| error("PROXY_IPC_FAILED"))?;
    pipe.flush().await.map_err(|_| error("PROXY_IPC_FAILED"))
}
async fn signed_write(pipe: &mut Pipe, secret: &[u8], kind: &str, mut body: Value) -> Result<()> {
    body["proof"] = json!(sign(secret, kind, &body));
    write(pipe, &body).await
}
fn signed_body(mut frame: Value, secret: &[u8], kind: &str) -> Result<Value> {
    let proof = frame
        .as_object_mut()
        .and_then(|m| m.remove("proof"))
        .ok_or_else(|| error("PROXY_IPC_AUTH_FAILED"))?;
    if !verify(secret, kind, &frame, &proof) {
        return Err(error("PROXY_IPC_AUTH_FAILED"));
    }
    Ok(frame)
}

pub(crate) struct Session {
    pub pipe: Pipe,
    context: String,
    client: String,
    channel: String,
    challenge: String,
    id: u64,
}
impl Session {
    pub async fn client(mut pipe: Pipe, context: String, secret: &[u8]) -> Result<Self> {
        let client = nonce();
        write(
            &mut pipe,
            &json!({"type":"hello","version":3,"context":context,"client":client}),
        )
        .await?;
        let frame = signed_body(read(&mut pipe).await?, secret, "server")?;
        if !exact(
            &frame,
            &[
                "type",
                "version",
                "context",
                "client",
                "channel",
                "challenge",
            ],
        ) || frame["type"] != "challenge"
            || frame["version"] != 3
            || frame["context"] != context
            || frame["client"] != client
            || !valid_nonce(&frame["channel"])
            || !valid_nonce(&frame["challenge"])
        {
            return Err(error("PROXY_IPC_AUTH_FAILED"));
        }
        Ok(Self {
            pipe,
            context,
            client,
            channel: frame["channel"].as_str().unwrap().into(),
            challenge: frame["challenge"].as_str().unwrap().into(),
            id: 0,
        })
    }
    pub async fn server(mut pipe: Pipe, context: String, secret: &[u8]) -> Result<Self> {
        let hello = read(&mut pipe).await?;
        if !exact(&hello, &["type", "version", "context", "client"])
            || hello["type"] != "hello"
            || hello["version"] != 3
            || hello["context"] != context
            || !valid_nonce(&hello["client"])
        {
            return Err(error("PROXY_IPC_AUTH_FAILED"));
        }
        let mut session = Self {
            pipe,
            context,
            client: hello["client"].as_str().unwrap().into(),
            channel: nonce(),
            challenge: nonce(),
            id: 0,
        };
        let mut body = session.binding();
        body["type"] = json!("challenge");
        signed_write(&mut session.pipe, secret, "server", body).await?;
        Ok(session)
    }
    fn binding(&self) -> Value {
        json!({"version":3,"context":self.context,"client":self.client,"channel":self.channel,"challenge":self.challenge})
    }
    fn same_binding(&self, frame: &Value) -> bool {
        ["version", "context", "client", "channel", "challenge"]
            .iter()
            .all(|k| frame[*k] == self.binding()[*k])
    }
    pub async fn request(&mut self, operation: &str, secret: &[u8]) -> Result<Value> {
        self.id = self
            .id
            .checked_add(1)
            .ok_or_else(|| error("PROXY_IPC_FAILED"))?;
        let mut body = self.binding();
        body["type"] = json!("request");
        body["id"] = json!(self.id);
        body["operation"] = json!(operation);
        signed_write(&mut self.pipe, secret, "request", body).await?;
        let frame = signed_body(read(&mut self.pipe).await?, secret, "response")?;
        if !exact(
            &frame,
            &[
                "version",
                "context",
                "client",
                "channel",
                "challenge",
                "type",
                "id",
                "operation",
                "next",
                "ok",
                "result",
            ],
        ) || !self.same_binding(&frame)
            || frame["type"] != "result"
            || frame["id"] != self.id
            || frame["operation"] != operation
            || !valid_nonce(&frame["next"])
            || !frame["ok"].is_boolean()
        {
            return Err(error("PROXY_IPC_AUTH_FAILED"));
        }
        self.challenge = frame["next"].as_str().unwrap().into();
        if frame["ok"] == false {
            return Err(error(
                frame["result"].as_str().unwrap_or("PROXY_IPC_FAILED"),
            ));
        }
        safe_status(&frame["result"])
    }
    pub async fn receive(&mut self, secret: &[u8]) -> Result<String> {
        let frame = signed_body(read(&mut self.pipe).await?, secret, "request")?;
        if !exact(
            &frame,
            &[
                "version",
                "context",
                "client",
                "channel",
                "challenge",
                "type",
                "id",
                "operation",
            ],
        ) || !self.same_binding(&frame)
            || frame["type"] != "request"
            || frame["id"].as_u64() != self.id.checked_add(1)
            || ![json!("ready"), json!("status"), json!("stop")].contains(&frame["operation"])
        {
            return Err(error("PROXY_IPC_AUTH_FAILED"));
        }
        self.id += 1;
        Ok(frame["operation"].as_str().unwrap().into())
    }
    pub async fn respond(
        &mut self,
        operation: &str,
        result: Result<Value>,
        secret: &[u8],
    ) -> Result<()> {
        let next = nonce();
        let mut body = self.binding();
        body["type"] = json!("result");
        body["id"] = json!(self.id);
        body["operation"] = json!(operation);
        body["next"] = json!(next);
        match result {
            Ok(status) => {
                body["ok"] = json!(true);
                body["result"] = safe_status(&status)?;
            }
            Err(e) => {
                body["ok"] = json!(false);
                body["result"] = json!(e.code);
            }
        }
        signed_write(&mut self.pipe, secret, "response", body).await?;
        self.challenge = next;
        Ok(())
    }
}
pub(crate) fn safe_status(v: &Value) -> Result<Value> {
    if !exact(
        v,
        &["state", "managed", "activeConnections", "lastErrorCode"],
    ) || !v["managed"].is_boolean()
        || !v["activeConnections"].as_u64().is_some_and(|n| n <= 16)
        || ![
            "stopped",
            "starting",
            "ready",
            "reconnecting",
            "failed",
            "external",
        ]
        .iter()
        .any(|s| v["state"] == *s)
        || !(v["lastErrorCode"].is_null()
            || v["lastErrorCode"]
                .as_str()
                .is_some_and(|s| error(s).code == s))
    {
        return Err(error("PROXY_IPC_FAILED"));
    }
    Ok(v.clone())
}

#[cfg(windows)]
pub(crate) async fn connect(path: &str) -> Result<Pipe> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(path)
        .map(|p| Box::new(p) as Pipe)
        .map_err(|_| error("PROXY_DAEMON_UNAVAILABLE"))
}
#[cfg(unix)]
pub(crate) async fn connect(path: &str) -> Result<Pipe> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = std::fs::symlink_metadata(path).map_err(|e| {
        error(if e.kind() == std::io::ErrorKind::NotFound {
            "PROXY_DAEMON_UNAVAILABLE"
        } else {
            "PROXY_IPC_AUTH_FAILED"
        })
    })?;
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(error("PROXY_IPC_AUTH_FAILED"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(error("PROXY_IPC_AUTH_FAILED"));
    }
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|_| error("PROXY_DAEMON_UNAVAILABLE"))?;
    if stream
        .peer_cred()
        .map_err(|_| error("PROXY_IPC_AUTH_FAILED"))?
        .uid()
        != unsafe { libc::geteuid() }
    {
        return Err(error("PROXY_IPC_AUTH_FAILED"));
    }
    Ok(Box::new(stream))
}

pub(crate) struct Listener {
    #[cfg(windows)]
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    server: tokio::net::UnixListener,
    path: String,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    _lock: std::fs::File,
}
impl Listener {
    pub fn bind(path: &str) -> Result<Self> {
        #[cfg(windows)]
        {
            let server = tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(path)
                .map_err(|_| error("PROXY_PORT_BUSY"))?;
            Ok(Self {
                server,
                path: path.into(),
            })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
            // An exclusive adjacent lock proves no cooperating daemon owns the stale socket.
            // The containing directory has already been checked as private and user-owned.
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(format!("{path}.lock"))
                .map_err(|_| error("PROXY_PORT_BUSY"))?;
            let metadata = lock.metadata().map_err(|_| error("PROXY_PORT_BUSY"))?;
            if !metadata.is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o077 != 0
                || metadata.nlink() != 1
            {
                return Err(error("PROXY_PORT_BUSY"));
            }
            fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| error("PROXY_PORT_BUSY"))?;
            match std::fs::symlink_metadata(path) {
                Ok(m)
                    if m.uid() == unsafe { libc::geteuid() }
                        && m.file_type().is_socket()
                        && m.mode() & 0o077 == 0 =>
                {
                    std::fs::remove_file(path).map_err(|_| error("PROXY_PORT_BUSY"))?;
                }
                Ok(_) => return Err(error("PROXY_PORT_BUSY")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(error("PROXY_PORT_BUSY")),
            }
            let server =
                tokio::net::UnixListener::bind(path).map_err(|_| error("PROXY_PORT_BUSY"))?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| error("PROXY_START_FAILED"))?;
            let metadata =
                std::fs::symlink_metadata(path).map_err(|_| error("PROXY_START_FAILED"))?;
            if !metadata.file_type().is_socket()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o077 != 0
            {
                return Err(error("PROXY_START_FAILED"));
            }
            let inode = metadata.ino();
            Ok(Self {
                server,
                path: path.into(),
                inode,
                _lock: lock,
            })
        }
    }
    pub async fn accept(&mut self) -> Result<Pipe> {
        #[cfg(windows)]
        {
            self.server
                .connect()
                .await
                .map_err(|_| error("PROXY_IPC_FAILED"))?;
            let next = tokio::net::windows::named_pipe::ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.path)
                .map_err(|_| error("PROXY_IPC_FAILED"))?;
            Ok(Box::new(std::mem::replace(&mut self.server, next)))
        }
        #[cfg(unix)]
        {
            let (stream, _) = self
                .server
                .accept()
                .await
                .map_err(|_| error("PROXY_IPC_FAILED"))?;
            if stream
                .peer_cred()
                .map_err(|_| error("PROXY_IPC_AUTH_FAILED"))?
                .uid()
                != unsafe { libc::geteuid() }
            {
                return Err(error("PROXY_IPC_AUTH_FAILED"));
            }
            Ok(Box::new(stream))
        }
    }
}
#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| {
            m.ino() == self.inode
                && m.uid() == unsafe { libc::geteuid() }
                && m.file_type().is_socket()
        }) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn unix_client_rejects_shared_permissions_and_socket_symlinks() {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
        let dir = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("pv-ipc-{}", &nonce()[..16]));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let path = dir.join("ipc.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(connect(path.to_str().unwrap()).await.is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        let inode = std::fs::symlink_metadata(&path).unwrap().ino();
        assert!(
            matches!(connect(path.to_str().unwrap()).await,Err(e) if e.code=="PROXY_IPC_AUTH_FAILED")
        );
        assert!(
            matches!(Listener::bind(path.to_str().unwrap()),Err(e) if e.code=="PROXY_PORT_BUSY")
        );
        assert_eq!(std::fs::symlink_metadata(&path).unwrap().ino(), inode);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(connect(path.to_str().unwrap()).await.is_ok());
        let alias = dir.join("alias.sock");
        symlink(&path, &alias).unwrap();
        assert!(
            matches!(connect(alias.to_str().unwrap()).await,Err(e) if e.code=="PROXY_IPC_AUTH_FAILED")
        );
        drop(listener);
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(format!("{}.lock", path.display())).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[tokio::test]
    async fn signed_challenge_for_old_nonce_or_other_config_is_rejected() {
        for field in ["context", "client"] {
            let (a, b) = tokio::io::duplex(16384);
            let key = [7u8; 32];
            let server = tokio::spawn(async move {
                let mut pipe = Box::new(b) as Pipe;
                let hello = read(&mut pipe).await.unwrap();
                let mut body = json!({"type":"challenge","version":3,"context":"binding","client":hello["client"],"channel":nonce(),"challenge":nonce()});
                body[field] = json!(nonce());
                signed_write(&mut pipe, &key, "server", body).await.unwrap();
            });
            assert!(
                matches!(Session::client(Box::new(a),"binding".into(),&key).await,Err(e) if e.code=="PROXY_IPC_AUTH_FAILED")
            );
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn signed_response_replay_and_status_modification_are_rejected() {
        for change in ["channel", "id", "status"] {
            let (a, b) = tokio::io::duplex(16384);
            let key = [4u8; 32];
            let server = tokio::spawn(async move {
                let mut s = Session::server(Box::new(b), "binding".into(), &key)
                    .await
                    .unwrap();
                let op = s.receive(&key).await.unwrap();
                let mut body = s.binding();
                body["type"] = json!("result");
                body["id"] = json!(s.id);
                body["operation"] = json!(op);
                body["next"] = json!(nonce());
                body["ok"] = json!(true);
                body["result"] = json!({"state":"ready","managed":true,"activeConnections":0,"lastErrorCode":null});
                if change == "channel" {
                    body["channel"] = json!(nonce());
                }
                if change == "id" {
                    body["id"] = json!(0);
                }
                body["proof"] = json!(sign(&key, "response", &body));
                if change == "status" {
                    body["result"]["state"] = json!("stopped");
                }
                write(&mut s.pipe, &body).await.unwrap();
            });
            let mut c = Session::client(Box::new(a), "binding".into(), &key)
                .await
                .unwrap();
            assert_eq!(
                c.request("status", &key).await.unwrap_err().code,
                "PROXY_IPC_AUTH_FAILED"
            );
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn fragmented_frames_keep_successive_boundaries() {
        let (a, mut b) = tokio::io::duplex(16);
        let writer = tokio::spawn(async move {
            for value in [json!({"one":1}), json!({"two":2})] {
                let bytes = serde_json::to_vec(&value).unwrap();
                for byte in (bytes.len() as u32).to_be_bytes().into_iter().chain(bytes) {
                    b.write_all(&[byte]).await.unwrap();
                    tokio::task::yield_now().await;
                }
            }
        });
        let mut pipe = Box::new(a) as Pipe;
        assert_eq!(read(&mut pipe).await.unwrap(), json!({"one":1}));
        assert_eq!(read(&mut pipe).await.unwrap(), json!({"two":2}));
        writer.await.unwrap();
    }
    #[test]
    fn proof_binds_context_and_operation() {
        let key = [1u8; 32];
        let body = json!({"context":"a","operation":"ready","id":1});
        let proof = json!(sign(&key, "request", &body));
        assert!(verify(&key, "request", &body, &proof));
        assert!(!verify(&key, "response", &body, &proof));
        assert!(!verify(
            &key,
            "request",
            &json!({"context":"b","operation":"ready","id":1}),
            &proof
        ));
        assert!(!verify(
            &key,
            "request",
            &json!({"context":"a","operation":"stop","id":1}),
            &proof
        ));
    }
    #[tokio::test]
    async fn mutual_authentication_and_replay_rejected() {
        let (a, b) = tokio::io::duplex(16384);
        let key = [7u8; 32];
        let server = tokio::spawn(async move {
            let mut s = Session::server(Box::new(b), "binding".into(), &key)
                .await
                .unwrap();
            let op = s.receive(&key).await.unwrap();
            assert_eq!(op, "status");
            s.respond(&op,Ok(json!({"state":"stopped","managed":false,"activeConnections":0,"lastErrorCode":null})),&key).await.unwrap();
            assert!(s.receive(&key).await.is_err());
        });
        let mut c = Session::client(Box::new(a), "binding".into(), &key)
            .await
            .unwrap();
        assert_eq!(c.request("status", &key).await.unwrap()["state"], "stopped");
        c.id = 0; // replayed identifier with current nonce is still rejected
        assert!(c.request("status", &key).await.is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn oversized_frame_is_rejected_before_payload_allocation() {
        let (mut a, b) = tokio::io::duplex(32);
        a.write_u32(8193).await.unwrap();
        let mut pipe = Box::new(b) as Pipe;
        assert_eq!(read(&mut pipe).await.unwrap_err().code, "PROXY_IPC_FAILED");
    }
    #[tokio::test]
    async fn wrong_key_cannot_authenticate_server() {
        let (a, b) = tokio::io::duplex(16384);
        let server = tokio::spawn(async move {
            let _ = Session::server(Box::new(b), "config".into(), &[1u8; 32]).await;
        });
        let result = Session::client(Box::new(a), "config".into(), &[2u8; 32]).await;
        assert!(matches!(result,Err(e) if e.code=="PROXY_IPC_AUTH_FAILED"));
        server.await.unwrap();
    }
}
