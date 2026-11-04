//! Native session protection. The lock owner thread also performs the final replacement.

use crate::error::{AppError, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rand::Rng;
#[cfg(target_os = "macos")]
use rand::TryRng;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, mpsc},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

// 宣言順を保存形式のJSONキー順として維持し、Cookieの中間Valueを作らない。
#[derive(serde::Serialize)]
struct SessionPlain<'a> {
    schema_version: u8,
    cookie: &'a str,
    binding: &'a str,
    user_name: &'a str,
    acquired_at: &'a str,
    source: &'a str,
}

#[cfg(test)]
mod serialization_tests {
    use super::*;

    #[test]
    fn saved_json_preserves_key_order_types_and_escaping() {
        let plain = SessionPlain {
            schema_version: 2,
            cookie: "a\"\\日本語",
            binding: "binding",
            user_name: "Poster",
            acquired_at: "2026-11-03T00:00:00.000Z",
            source: "plumivox_api",
        };
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            r#"{"schema_version":2,"cookie":"a\"\\日本語","binding":"binding","user_name":"Poster","acquired_at":"2026-11-03T00:00:00.000Z","source":"plumivox_api"}"#
        );
    }
}

fn failure(code: &str) -> AppError {
    let message = match code {
        "AUTH_BUSY" => {
            "このアカウントは別のプロセスでログイン処理中です。完了後に再実行してください。"
        }
        "AUTH_CANCELLED" => {
            "ログインは中止されました。この処理では保存セッションを変更していません。"
        }
        "AUTH_LOCK_LOST" => {
            "認証更新の排他を維持できませんでした。この処理では保存セッションを変更していません。"
        }
        "LOGIN_FAILED" => {
            "ログインCookieを確認できません。この処理では保存セッションを変更していません。"
        }
        "AUTH_SAVE_UNCERTAIN" => {
            "認証更新の完了を確認できません。保存セッションが更新されている可能性があります。list_accountsで状態を確認してください。自動再送していません。"
        }
        _ => "セッション保存先とOSの資格情報ストアを確認してください。",
    };
    AppError::new(code, message)
}
fn storage() -> AppError {
    failure("SESSION_STORAGE_UNAVAILABLE")
}
fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(storage)
}
fn binding(account: &Value) -> Result<String> {
    Ok(hash(&format!(
        "{}\n{}",
        text(account, "apiKey")?,
        text(account, "proxy")?
    )))
}
fn valid_cookie(cookie: &str) -> bool {
    !cookie.trim().is_empty()
        && cookie.len() <= MAX_BYTES as usize / 2
        && !cookie.chars().any(|c| matches!(c, '\0' | '\r' | '\n'))
}

#[derive(Clone)]
pub struct SessionStorage {
    root: Option<PathBuf>,
}

struct Commit {
    staging: Pending,
    destination: PathBuf,
    cancel: CancellationToken,
    reply: oneshot::Sender<Result<()>>,
}
enum LockCommand {
    Commit(Commit),
    Release,
}
struct LeaseInner {
    directory: PathBuf,
    tx: mpsc::Sender<LockCommand>,
    lost: CancellationToken,
}
/// Dropping the last lease closes the command channel and releases the native lock.
pub struct SessionLease {
    inner: Arc<LeaseInner>,
    owner: Option<std::thread::JoinHandle<()>>,
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        let _ = self.inner.tx.send(LockCommand::Release);
        if let Some(owner) = self.owner.take() {
            let _ = owner.join();
        }
    }
}
impl SessionLease {
    pub fn assert_owned(&self) -> Result<()> {
        if self.inner.lost.is_cancelled() {
            Err(failure("AUTH_LOCK_LOST"))
        } else {
            Ok(())
        }
    }
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.lost.clone()
    }
}

impl SessionStorage {
    pub fn new(state_directory: Option<PathBuf>) -> Self {
        let root = std::env::var_os("PLUMIVOX_STATE_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or(state_directory)
            .or_else(default_root);
        Self { root }
    }
    fn directory(&self, account: &Value) -> Result<PathBuf> {
        let root = self.root.as_ref().ok_or_else(storage)?;
        Ok(root.join(hash(&format!(
            "{}\n{}",
            text(account, "account")?,
            text(account, "userName")?.to_lowercase()
        ))))
    }
    fn prepare_sync(&self, account: &Value) -> Result<()> {
        let root = self.root.as_ref().ok_or_else(storage)?;
        validate_directory(root)?;
        // Create and protect the root before creating account data beneath it.
        fs::create_dir_all(root).map_err(|_| storage())?;
        validate_directory(root)?;
        platform::private(&normalized_canonical(root)?)?;
        let directory = self.directory(account)?;
        validate_directory(&directory)?;
        fs::create_dir_all(&directory).map_err(|_| storage())?;
        validate_directory(&directory)?;
        platform::private(&normalized_canonical(&directory)?)?;
        let destination = directory.join(platform::FILE_NAME);
        #[cfg(feature = "test-fixture")]
        crate::test_support::count("sessionChecks");
        validate_optional_file(&destination)?;
        platform::prepare(root)?;
        Ok(())
    }
    pub async fn prepare(&self, account: &Value) -> Result<()> {
        self.prepare_sync(account)
    }
    pub async fn read(&self, account: &Value) -> Value {
        let result = (|| -> Result<Value> {
            let directory = self.directory(account)?;
            validate_directory(&directory)?;
            let file = directory.join(platform::FILE_NAME);
            #[cfg(feature = "test-fixture")]
            crate::test_support::count("sessionChecks");
            match fs::symlink_metadata(&file) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(json!({"status":"missing"}));
                }
                Err(_) => return Err(storage()),
                Ok(_) => (),
            }
            let session = decode(
                &file,
                &binding(account)?,
                text(account, "userName")?,
                true,
                self.root.as_deref(),
            )?;
            Ok(
                json!({"status":"valid", "cookie": session["cookie"], "acquiredAt": session["acquired_at"]}),
            )
        })();
        if self.root.is_none() {
            json!({"status":"missing"})
        } else {
            result.unwrap_or_else(|_| json!({"status":"invalid"}))
        }
    }
    pub async fn lock(&self, account: &Value) -> Result<SessionLease> {
        self.prepare_sync(account)?;
        let directory = normalized_canonical(&self.directory(account)?)?;
        let thread_directory = directory.clone();
        let (tx, rx) = mpsc::channel::<LockCommand>();
        let (ready_tx, ready_rx) = oneshot::channel();
        let lost = CancellationToken::new();
        let thread_lost = lost.clone();
        let owner = std::thread::Builder::new()
            .name("plumivox-auth-lock".into())
            .spawn(move || {
                // Windows mutexes are thread-affine: acquisition, commit and release stay here.
                let guard = match platform::Lock::acquire(&thread_directory) {
                    Ok(guard) => guard,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                struct LostOnExit(CancellationToken);
                impl Drop for LostOnExit {
                    fn drop(&mut self) {
                        self.0.cancel();
                    }
                }
                let _lost_on_exit = LostOnExit(thread_lost.clone());
                if ready_tx.send(Ok(())).is_err() {
                    return;
                }
                loop {
                    #[cfg(feature = "test-fixture")]
                    let command = loop {
                        if crate::test_support::lock_lost_requested() {
                            drop(guard);
                            thread_lost.cancel();
                            return;
                        }
                        match rx.recv_timeout(std::time::Duration::from_millis(25)) {
                            Ok(command) => break command,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    };
                    #[cfg(not(feature = "test-fixture"))]
                    let command = match rx.recv() {
                        Ok(command) => command,
                        Err(_) => break,
                    };
                    let mut command = match command {
                        LockCommand::Commit(command) => command,
                        LockCommand::Release => return,
                    };
                    let result = if thread_lost.is_cancelled() {
                        Err(failure("AUTH_LOCK_LOST"))
                    } else {
                        commit(&thread_directory, &command, &guard)
                    };
                    if result
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.code == "AUTH_LOCK_LOST")
                    {
                        thread_lost.cancel();
                    }
                    // Commitには暗号文の全量書込み・sync完了後だけ到達する。
                    // 取消と所有喪失以外の保存失敗では復旧用の実体を残す。
                    command.staging.retain = result
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.code != "AUTH_CANCELLED" && e.code != "AUTH_LOCK_LOST");
                    let _ = command.reply.send(result);
                }
            })
            .map_err(|_| storage())?;
        ready_rx.await.map_err(|_| storage())??;
        Ok(SessionLease {
            owner: Some(owner),
            inner: Arc::new(LeaseInner {
                directory,
                tx,
                lost,
            }),
        })
    }
    pub async fn save(
        &self,
        account: &Value,
        cookie: &str,
        lease: &SessionLease,
        cancel: CancellationToken,
    ) -> Result<Value> {
        lease.assert_owned()?;
        if cancel.is_cancelled() {
            return Err(failure("AUTH_CANCELLED"));
        }
        if !valid_cookie(cookie) {
            return Err(failure("LOGIN_FAILED"));
        }
        self.prepare_sync(account)?;
        let directory = normalized_canonical(&self.directory(account)?)?;
        if directory != lease.inner.directory {
            return Err(failure("AUTH_LOCK_LOST"));
        }
        let acquired_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let plain = Zeroizing::new(
            serde_json::to_string(&SessionPlain {
                schema_version: 2,
                cookie,
                binding: &binding(account)?,
                user_name: text(account, "userName")?,
                acquired_at: &acquired_at,
                source: "plumivox_api",
            })
            .map_err(|_| storage())?,
        );
        let encrypted = platform::encrypt(plain.as_bytes(), self.root.as_deref())?;
        #[cfg(feature = "test-fixture")]
        {
            let result = crate::test_support::boundary("encrypt", &cancel).await;
            if result
                .as_ref()
                .err()
                .is_some_and(|e| e.code == "AUTH_LOCK_LOST")
            {
                lease.inner.lost.cancelled().await;
            }
            result?;
        }
        if encrypted.len() > MAX_BYTES as usize {
            return Err(storage());
        }
        lease.assert_owned()?;
        if cancel.is_cancelled() {
            return Err(failure("AUTH_CANCELLED"));
        }
        let staging = directory.join(format!("session-{}.pending", random_id()));
        let mut file = new_private_file(&staging)?;
        let pending = Pending {
            path: staging,
            retain: false,
        };
        let middle = encrypted.len() / 2;
        file.write_all(&encrypted[..middle])
            .map_err(|_| storage())?;
        #[cfg(feature = "test-fixture")]
        {
            let result = crate::test_support::boundary("writing", &cancel).await;
            if result
                .as_ref()
                .err()
                .is_some_and(|e| e.code == "AUTH_LOCK_LOST")
            {
                lease.inner.lost.cancelled().await;
            }
            result?;
        }
        file.write_all(&encrypted[middle..])
            .and_then(|_| file.sync_all())
            .map_err(|_| storage())?;
        drop(file);
        #[cfg(feature = "test-fixture")]
        {
            let result = crate::test_support::boundary("staged", &cancel).await;
            if result
                .as_ref()
                .err()
                .is_some_and(|e| e.code == "AUTH_LOCK_LOST")
            {
                lease.inner.lost.cancelled().await;
            }
            result?;
        }
        let (reply, response) = oneshot::channel();
        lease.assert_owned()?;
        if cancel.is_cancelled() {
            return Err(failure("AUTH_CANCELLED"));
        }
        lease
            .inner
            .tx
            .send(LockCommand::Commit(Commit {
                staging: pending,
                destination: directory.join(platform::FILE_NAME),
                cancel: cancel.clone(),
                reply,
            }))
            .map_err(|_| failure("AUTH_LOCK_LOST"))?;
        // After dispatch, only the owner can determine whether replacement happened.
        // Keep staging alive even if the caller drops this future while commit is in flight.
        response
            .await
            .map_err(|_| failure("AUTH_SAVE_UNCERTAIN"))??;
        #[cfg(feature = "test-fixture")]
        {
            if crate::test_support::boundary("saved", &cancel)
                .await
                .is_err()
                || lease.assert_owned().is_err()
                || cancel.is_cancelled()
            {
                return Err(failure("AUTH_SAVE_UNCERTAIN"));
            }
        }
        Ok(
            json!({"account":account["account"],"userName":account["userName"],
            "sessionSource":platform::SOURCE,"acquiredAt":acquired_at}),
        )
    }
}

struct Pending {
    path: PathBuf,
    retain: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.retain {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn random_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn commit(directory: &Path, command: &Commit, guard: &platform::Lock) -> Result<()> {
    validate_directory(directory)?;
    if command.staging.path.parent() != Some(directory)
        || command.destination != directory.join(platform::FILE_NAME)
    {
        return Err(storage());
    }
    validate_optional_file(&command.destination)?;
    validate_optional_file(&command.staging.path)?;
    // Keep an encrypted backup, matching the existing Windows session writer.
    if command.destination.exists() {
        let previous = bounded_read(&command.destination)?;
        let backup = directory.join(format!(
            "session-{}.backup{}",
            random_id(),
            platform::BACKUP_SUFFIX
        ));
        let mut file = new_private_file(&backup)?;
        let mut temporary = Pending {
            path: backup,
            retain: false,
        };
        file.write_all(&previous)
            .and_then(|_| file.sync_all())
            .map_err(|_| storage())?;
        drop(file);
        temporary.retain = true;
    }
    if command.cancel.is_cancelled() {
        return Err(failure("AUTH_CANCELLED"));
    }
    guard.assert_owned()?;
    platform::replace(&command.staging.path, &command.destination)?;
    // A cancellation or durability failure after rename cannot promise unchanged state.
    if command.cancel.is_cancelled() {
        return Err(failure("AUTH_SAVE_UNCERTAIN"));
    }
    platform::sync_directory(directory).map_err(|_| failure("AUTH_SAVE_UNCERTAIN"))?;
    Ok(())
}

pub async fn read_legacy_protected_cookie(
    file: &Path,
    binding: &str,
    user_name: &str,
) -> Option<String> {
    #[cfg(feature = "test-fixture")]
    crate::test_support::count("sessionChecks");
    #[cfg(windows)]
    {
        decode(file, binding, user_name, false, None)
            .ok()?
            .get("cookie")?
            .as_str()
            .map(str::to_owned)
    }
    #[cfg(not(windows))]
    {
        let _ = (file, binding, user_name);
        None
    }
}

fn decode(
    file: &Path,
    expected_binding: &str,
    user_name: &str,
    own: bool,
    root: Option<&Path>,
) -> Result<Value> {
    validate_ancestors(file.parent().ok_or_else(storage)?)?;
    #[cfg(feature = "test-fixture")]
    crate::test_support::count("sessionReads");
    let encrypted = bounded_read(file)?;
    #[cfg(feature = "test-fixture")]
    crate::test_support::count("decryptSpawns");
    #[cfg(feature = "test-fixture")]
    let plain = match crate::test_support::decrypted_fixture(&encrypted) {
        Some(plain) => Zeroizing::new(plain),
        None => platform::decrypt(&encrypted, root)?,
    };
    #[cfg(not(feature = "test-fixture"))]
    let plain = platform::decrypt(&encrypted, root)?;
    if plain.len() > MAX_BYTES as usize {
        return Err(storage());
    }
    let session: Value = serde_json::from_slice(&plain).map_err(|_| storage())?;
    if session["schema_version"] != 2
        || session["binding"].as_str() != Some(expected_binding)
        || text(&session, "user_name")?.to_lowercase() != user_name.to_lowercase()
        || !valid_cookie(text(&session, "cookie")?)
    {
        return Err(storage());
    }
    if own
        && (session["source"] != "plumivox_api"
            || DateTime::parse_from_rfc3339(text(&session, "acquired_at")?).is_err())
    {
        return Err(storage());
    }
    Ok(session)
}

fn default_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Kagayoi").join("Plumivox"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|p| PathBuf::from(p).join("Library/Application Support/Kagayoi/Plumivox"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
            .map(|p| p.join("Kagayoi/Plumivox"))
    }
}
fn normalized_canonical(path: &Path) -> Result<PathBuf> {
    let resolved = fs::canonicalize(path).map_err(|_| storage())?;
    #[cfg(windows)]
    {
        let s = resolved.to_string_lossy();
        if let Some(p) = s.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{p}")));
        }
        if let Some(p) = s.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(p));
        }
    }
    Ok(resolved)
}
fn path_inside(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        PathBuf::from(path.to_string_lossy().to_lowercase())
            .starts_with(PathBuf::from(root.to_string_lossy().to_lowercase()))
    }
    #[cfg(not(windows))]
    {
        path.starts_with(root)
    }
}
// Distributed binaries live under <plugin>/dist. Do not depend solely on the
// build machine's manifest path when rejecting session storage inside a plugin.
fn runtime_plugin_root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let dist = executable.parent()?;
    if !dist
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("dist")
    {
        return None;
    }
    let root = dist.parent()?;
    for marker in ["mcp.json", "plugin.json"] {
        let metadata = fs::symlink_metadata(root.join(marker)).ok()?;
        if !metadata.is_file() || is_link(&metadata) {
            return None;
        }
    }
    Some(root.to_owned())
}
// Canonicalize the existing ancestor even when the intended directory has not
// been created yet. This also closes Windows short-name/prefix alias bypasses.
fn canonical_with_missing_tail(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path.to_owned();
    let mut tail = Vec::new();
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(_) => {
                let mut canonical = normalized_canonical(&ancestor)?;
                for component in tail.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tail.push(ancestor.file_name().ok_or_else(storage)?.to_owned());
                ancestor = ancestor.parent().ok_or_else(storage)?.to_owned();
            }
            Err(_) => return Err(storage()),
        }
    }
}
fn validate_directory(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(storage());
    }
    validate_ancestors(path)?;
    let canonical = canonical_with_missing_tail(path)?;
    if canonical.parent().is_none() {
        return Err(storage());
    }
    let mut blocked = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    if let Some(codex) = std::env::var_os("CODEX_HOME") {
        blocked.push(codex.into());
    }
    if let Some(plugin) = runtime_plugin_root() {
        blocked.push(plugin);
    }
    if blocked.iter().any(|root| {
        path_inside(path, root)
            || canonical_with_missing_tail(root).is_ok_and(|root| path_inside(&canonical, &root))
    }) || [path, canonical.as_path()].iter().any(|candidate| {
        candidate.components().any(|c| {
            c.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(".codex")
        })
    }) {
        return Err(storage());
    }
    Ok(())
}
fn validate_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.is_dir() && !is_link(&meta) => (),
            Ok(meta) if is_link(&meta) => {
                // A safe junction/symlink ancestor is supported. validate_directory
                // compares the fully resolved target with every forbidden root.
                let resolved = normalized_canonical(ancestor)?;
                if !fs::metadata(&resolved).map_err(|_| storage())?.is_dir() {
                    return Err(storage());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(storage()),
        }
    }
    Ok(())
}
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
fn validate_optional_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !is_link(&meta) && meta.len() <= MAX_BYTES => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(storage()),
    }
}
fn read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    options
}
fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    validate_optional_file(path)?;
    let file = read_options().open(path).map_err(|_| storage())?;
    let meta = file.metadata().map_err(|_| storage())?;
    if !meta.is_file() || is_link(&meta) || meta.len() > MAX_BYTES {
        return Err(storage());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| storage())?;
    if bytes.len() > MAX_BYTES as usize {
        return Err(storage());
    }
    Ok(bytes)
}
fn new_private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path).map_err(|_| storage())
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        ffi::OsStr,
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Security::{Authorization::*, Cryptography::*, *},
        Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
        System::Threading::{
            CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex, WaitForSingleObject,
        },
    };
    pub const FILE_NAME: &str = "session.dpapi";
    pub const BACKUP_SUFFIX: &str = ".dpapi";
    pub const SOURCE: &str = "plumivox_dpapi";
    fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(Some(0)).collect()
    }
    // 内部のMutex名・署名の正規化は維持し、Win32境界だけ拡張絶対パスにする。
    fn file_path(path: &Path) -> Result<Vec<u16>> {
        Ok(wide(fs::canonicalize(path).map_err(|_| storage())?))
    }
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Local(*mut core::ffi::c_void);
    impl Drop for Local {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    fn current_user() -> Result<Vec<usize>> {
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(storage());
            }
            let token = Handle(token);
            let mut size = 0;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
            if size == 0 || size > 65536 {
                return Err(storage());
            }
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            ) == 0
            {
                return Err(storage());
            }
            Ok(buffer)
        }
    }
    fn check_owner(path: &Path, sid: PSID) -> Result<()> {
        unsafe {
            let mut owner = null_mut();
            let mut descriptor = null_mut();
            if GetNamedSecurityInfoW(
                file_path(path)?.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            ) != 0
            {
                return Err(storage());
            }
            let _descriptor = Local(descriptor);
            if owner.is_null() || EqualSid(owner, sid) == 0 {
                return Err(storage());
            }
            Ok(())
        }
    }
    pub fn private(path: &Path) -> Result<()> {
        validate_ancestors(path)?;
        let user = current_user()?;
        let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        check_owner(path, sid)?;
        unsafe {
            let mut sid_text = null_mut();
            if ConvertSidToStringSidW(sid, &mut sid_text) == 0 {
                return Err(storage());
            }
            let _sid_text = Local(sid_text.cast());
            let mut len = 0;
            while *sid_text.add(len) != 0 {
                len += 1;
            }
            let sid_string = String::from_utf16(std::slice::from_raw_parts(sid_text, len))
                .map_err(|_| storage())?;
            let sddl = wide(format!("D:P(A;OICI;FA;;;{sid_string})"));
            let mut descriptor = null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            ) == 0
            {
                return Err(storage());
            }
            let _descriptor = Local(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = null_mut();
            if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0
                || present == 0
                || dacl.is_null()
            {
                return Err(storage());
            }
            if SetNamedSecurityInfoW(
                file_path(path)?.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            ) != 0
            {
                return Err(storage());
            }
            check_owner(path, sid)?;
            // Verify the applied protected DACL, rather than treating a successful setter as proof.
            let mut actual = null_mut();
            let mut actual_dacl = null_mut();
            if GetNamedSecurityInfoW(
                file_path(path)?.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut actual_dacl,
                null_mut(),
                &mut actual,
            ) != 0
            {
                return Err(storage());
            }
            let _actual = Local(actual);
            let mut control = 0;
            let mut revision = 0;
            if GetSecurityDescriptorControl(actual, &mut control, &mut revision) == 0
                || control & SE_DACL_PROTECTED == 0
                || actual_dacl.is_null()
                || (*actual_dacl).AceCount != 1
            {
                return Err(storage());
            }
            let mut ace = null_mut();
            if GetAce(actual_dacl, 0, &mut ace) == 0 {
                return Err(storage());
            }
            let allowed = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
            let allowed_sid = (&allowed.SidStart as *const u32).cast_mut().cast();
            if allowed.Header.AceType != 0
                || allowed.Mask & 0x1f01ff != 0x1f01ff
                || EqualSid(allowed_sid, sid) == 0
            {
                return Err(storage());
            }
        }
        Ok(())
    }
    fn protect(input: &[u8], encrypt: bool) -> Result<Zeroizing<Vec<u8>>> {
        if input.len() > MAX_BYTES as usize {
            return Err(storage());
        }
        unsafe {
            let input = CRYPT_INTEGER_BLOB {
                cbData: input.len() as u32,
                pbData: input.as_ptr().cast_mut(),
            };
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: null_mut(),
            };
            let ok = if encrypt {
                CryptProtectData(
                    &input,
                    null(),
                    null(),
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    null_mut(),
                    null(),
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            };
            if ok == 0 {
                return Err(storage());
            }
            let _output = Local(output.pbData.cast());
            if output.cbData > MAX_BYTES as u32 {
                return Err(storage());
            }
            let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
            let copied = Zeroizing::new(bytes.to_vec());
            use zeroize::Zeroize;
            bytes.zeroize();
            Ok(copied)
        }
    }
    pub fn prepare(_root: &Path) -> Result<()> {
        let probe = encrypt(b"Plumivox DPAPI", None)?;
        if decrypt(&probe, None)?.as_slice() != b"Plumivox DPAPI" {
            return Err(storage());
        }
        Ok(())
    }
    pub fn encrypt(plain: &[u8], _root: Option<&Path>) -> Result<Vec<u8>> {
        let text = std::str::from_utf8(plain).map_err(|_| storage())?;
        let utf16 = Zeroizing::new(
            text.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        Ok(hex::encode(protect(&utf16, true)?.as_slice()).into_bytes())
    }
    pub fn decrypt(encrypted: &[u8], _root: Option<&Path>) -> Result<Zeroizing<Vec<u8>>> {
        let value = std::str::from_utf8(encrypted)
            .map_err(|_| storage())?
            .trim();
        let bytes = hex::decode(value).map_err(|_| storage())?;
        let plain = protect(&bytes, false)?;
        if plain.len() % 2 != 0 {
            return Err(storage());
        }
        let words = Zeroizing::new(
            plain
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        );
        let decoded = Zeroizing::new(String::from_utf16(&words).map_err(|_| storage())?);
        Ok(Zeroizing::new(decoded.as_bytes().to_vec()))
    }
    pub struct Lock(Handle);
    impl Lock {
        pub fn assert_owned(&self) -> Result<()> {
            unsafe {
                // The owner can re-enter the mutex; balance that recursion immediately.
                if WaitForSingleObject(self.0.0, 0) != WAIT_OBJECT_0 {
                    return Err(failure("AUTH_LOCK_LOST"));
                }
                if ReleaseMutex(self.0.0) == 0 {
                    return Err(failure("AUTH_LOCK_LOST"));
                }
            }
            Ok(())
        }
        pub fn acquire(directory: &Path) -> Result<Self> {
            // Match Node realpath + lowercase + SHA-256 and setup's Global mutex exactly.
            let name = wide(format!(
                r"Global\Kagayoi.Plumivox.Auth.{}",
                hash(&directory.to_string_lossy().to_lowercase())
            ));
            unsafe {
                let handle = CreateMutexW(null(), 0, name.as_ptr());
                if handle.is_null() {
                    return Err(storage());
                }
                let handle = Handle(handle);
                match WaitForSingleObject(handle.0, 0) {
                    WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
                    WAIT_TIMEOUT => Err(failure("AUTH_BUSY")),
                    _ => Err(storage()),
                }
            }
        }
    }
    impl Drop for Lock {
        fn drop(&mut self) {
            unsafe {
                ReleaseMutex(self.0.0);
            }
        }
    }
    pub fn replace(staging: &Path, destination: &Path) -> Result<()> {
        let staging = file_path(staging)?;
        let destination = fs::canonicalize(destination.parent().ok_or_else(storage)?)
            .map_err(|_| storage())?
            .join(destination.file_name().ok_or_else(storage)?);
        let destination = wide(destination);
        unsafe {
            if MoveFileExW(
                staging.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            ) == 0
            {
                return Err(failure("AUTH_SAVE_UNCERTAIN"));
            }
        }
        Ok(())
    }
    pub fn sync_directory(_directory: &Path) -> Result<()> {
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn powershell_securestring_uses_utf16le_dpapi_hex() {
            let original = r#"{"schema_version":2,"cookie":"auth_token=fake; ct0=日本語"}"#;
            let encrypted = encrypt(original.as_bytes(), None).unwrap();
            let binary = hex::decode(&encrypted).unwrap();
            let utf16 = protect(&binary, false).unwrap();
            let expected: Vec<u8> = original.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert_eq!(utf16.as_slice(), expected);
            assert_eq!(
                decrypt(&encrypted, None).unwrap().as_slice(),
                original.as_bytes()
            );
            assert!(decrypt(b"not hex", None).is_err());
        }

        #[test]
        fn native_mutex_excludes_another_owner_thread() {
            let directory = PathBuf::from(format!(r"C:\Plumivox-mutex-test-{}", random_id()));
            let owner = Lock::acquire(&directory).unwrap();
            owner.assert_owned().unwrap();
            let other_directory = directory.clone();
            let error =
                std::thread::spawn(move || Lock::acquire(&other_directory).err().map(|e| e.code))
                    .join()
                    .unwrap();
            assert_eq!(error.as_deref(), Some("AUTH_BUSY"));
            drop(owner);
            Lock::acquire(&directory).unwrap();
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use fs2::FileExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    pub const FILE_NAME: &str = "session.keychain";
    pub const BACKUP_SUFFIX: &str = ".keychain";
    pub const SOURCE: &str = "plumivox_keychain";
    pub fn private(path: &Path) -> Result<()> {
        let meta = fs::symlink_metadata(path).map_err(|_| storage())?;
        if !meta.is_dir() || is_link(&meta) || meta.uid() != unsafe { libc::geteuid() } {
            return Err(storage());
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| storage())?;
        let after = fs::symlink_metadata(path).map_err(|_| storage())?;
        if after.uid() != meta.uid() || after.mode() & 0o777 != 0o700 || is_link(&after) {
            return Err(storage());
        }
        Ok(())
    }
    pub struct Lock(File, PathBuf);
    impl Lock {
        pub fn assert_owned(&self) -> Result<()> {
            let held = self.0.metadata().map_err(|_| failure("AUTH_LOCK_LOST"))?;
            let current = fs::symlink_metadata(&self.1).map_err(|_| failure("AUTH_LOCK_LOST"))?;
            if is_link(&current)
                || held.dev() != current.dev()
                || held.ino() != current.ino()
                || held.nlink() != 1
            {
                return Err(failure("AUTH_LOCK_LOST"));
            }
            Ok(())
        }
        pub fn acquire(directory: &Path) -> Result<Self> {
            let path = directory.join(".auth.lock");
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)
                .map_err(|_| storage())?;
            let meta = file.metadata().map_err(|_| storage())?;
            if !meta.is_file()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o077 != 0
                || meta.nlink() != 1
            {
                return Err(storage());
            }
            file.try_lock_exclusive().map_err(|e| {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    failure("AUTH_BUSY")
                } else {
                    storage()
                }
            })?;
            Ok(Self(file, path))
        }
    }
    impl Drop for Lock {
        fn drop(&mut self) {
            let _ = FileExt::unlock(&self.0);
        }
    }
    pub fn replace(staging: &Path, destination: &Path) -> Result<()> {
        fs::rename(staging, destination).map_err(|_| failure("AUTH_SAVE_UNCERTAIN"))
    }
    pub fn sync_directory(directory: &Path) -> Result<()> {
        File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| storage())
    }

    #[cfg(target_os = "macos")]
    enum KeychainBackend {
        System,
        #[cfg(feature = "test-fixture")]
        Fixture(security_framework::os::macos::keychain::SecKeychain),
    }
    #[cfg(target_os = "macos")]
    impl KeychainBackend {
        fn open() -> Result<Self> {
            #[cfg(feature = "test-fixture")]
            if let Some(path) = std::env::var_os("PLUMIVOX_TEST_KEYCHAIN") {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err(storage());
                }
                let canonical = normalized_canonical(&path)?;
                if path != canonical {
                    return Err(storage());
                }
                let parent = canonical.parent().ok_or_else(storage)?;
                validate_directory(parent)?;
                let temporary = [std::env::temp_dir(), PathBuf::from("/private/tmp")]
                    .iter()
                    .filter_map(|root| normalized_canonical(root).ok())
                    .any(|root| canonical.starts_with(root));
                let metadata = fs::symlink_metadata(&canonical).map_err(|_| storage())?;
                let parent_metadata = fs::metadata(parent).map_err(|_| storage())?;
                let uid = unsafe { libc::geteuid() };
                if !temporary
                    || !metadata.is_file()
                    || is_link(&metadata)
                    || metadata.uid() != uid
                    || parent_metadata.uid() != uid
                    || parent_metadata.mode() & 0o077 != 0
                {
                    return Err(storage());
                }
                let keychain =
                    security_framework::os::macos::keychain::SecKeychain::open(&canonical)
                        .map_err(|_| storage())?;
                return Ok(Self::Fixture(keychain));
            }
            Ok(Self::System)
        }
        fn get(&self, service: &str, account: &str) -> security_framework::base::Result<Vec<u8>> {
            match self {
                Self::System => {
                    security_framework::passwords::get_generic_password(service, account)
                }
                #[cfg(feature = "test-fixture")]
                Self::Fixture(keychain) => keychain
                    .find_generic_password(service, account)
                    .map(|(password, _)| password.as_ref().to_vec()),
            }
        }
        fn create(
            &self,
            service: &str,
            account: &str,
            bytes: &[u8],
        ) -> security_framework::base::Result<()> {
            match self {
                Self::System => {
                    security_framework::passwords::set_generic_password(service, account, bytes)
                }
                #[cfg(feature = "test-fixture")]
                Self::Fixture(keychain) => keychain.add_generic_password(service, account, bytes),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn key(root: &Path, create: bool) -> Result<Zeroizing<Vec<u8>>> {
        let keychain = KeychainBackend::open()?;
        let account = hash(&normalized_canonical(root)?.to_string_lossy());
        let service = "com.kagayoi.plumivox.session.aes256gcm.v1";
        // Reading an existing immutable key needs no filesystem lock. Normal account
        // reads must not become invalid merely because another client reads a key.
        match keychain.get(service, &account) {
            Ok(bytes) if bytes.len() == 32 => return Ok(Zeroizing::new(bytes)),
            Err(error) if error.code() == -25300 && create => (),
            _ => return Err(storage()),
        }
        // Only first creation is serialized. Recheck after acquiring the lock so
        // a competing initializer cannot cause replacement of an existing key.
        let _lock = Lock::acquire(root)?;
        match keychain.get(service, &account) {
            Ok(bytes) if bytes.len() == 32 => Ok(Zeroizing::new(bytes)),
            Err(error) if error.code() == -25300 && create => {
                for entry in fs::read_dir(root).map_err(|_| storage())? {
                    let entry = entry.map_err(|_| storage())?;
                    if entry.file_type().map_err(|_| storage())?.is_dir()
                        && entry
                            .path()
                            .join(FILE_NAME)
                            .try_exists()
                            .map_err(|_| storage())?
                    {
                        return Err(storage());
                    }
                }
                let mut bytes = Zeroizing::new(vec![0u8; 32]);
                rand::rngs::SysRng
                    .try_fill_bytes(&mut bytes)
                    .map_err(|_| storage())?;
                keychain
                    .create(service, &account, &bytes)
                    .map_err(|_| storage())?;
                let saved = Zeroizing::new(keychain.get(service, &account).map_err(|_| storage())?);
                if *saved != *bytes {
                    return Err(storage());
                }
                Ok(saved)
            }
            _ => Err(storage()),
        }
    }
    #[cfg(target_os = "macos")]
    pub fn prepare(root: &Path) -> Result<()> {
        key(root, true).map(|_| ())
    }
    #[cfg(target_os = "macos")]
    pub fn encrypt(plain: &[u8], root: Option<&Path>) -> Result<Vec<u8>> {
        use aes_gcm::{
            Aes256Gcm, KeyInit, Nonce,
            aead::{Aead, Payload},
        };
        let key = key(root.ok_or_else(storage)?, false)?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| storage())?;
        let mut nonce = [0u8; 12];
        rand::rngs::SysRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| storage())?;
        let ciphertext = cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plain,
                    aad: b"Plumivox session v1",
                },
            )
            .map_err(|_| storage())?;
        let mut output = b"PLUMIVOX-KEYCHAIN-1\n".to_vec();
        output.extend_from_slice(&nonce);
        output.extend(ciphertext);
        Ok(output)
    }
    #[cfg(target_os = "macos")]
    pub fn decrypt(encrypted: &[u8], root: Option<&Path>) -> Result<Zeroizing<Vec<u8>>> {
        decrypt_with_key(encrypted, || key(root.ok_or_else(storage)?, false))
    }
    #[cfg(target_os = "macos")]
    fn decrypt_with_key(
        encrypted: &[u8],
        load_key: impl FnOnce() -> Result<Zeroizing<Vec<u8>>>,
    ) -> Result<Zeroizing<Vec<u8>>> {
        use aes_gcm::{
            Aes256Gcm, KeyInit, Nonce,
            aead::{Aead, Payload},
        };
        let body = encrypted
            .strip_prefix(b"PLUMIVOX-KEYCHAIN-1\n")
            .filter(|b| b.len() >= 28)
            .ok_or_else(storage)?;
        let (nonce, ciphertext) = body.split_at(12);
        let key = load_key()?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| storage())?;
        Ok(Zeroizing::new(
            cipher
                .decrypt(
                    &Nonce::try_from(nonce).map_err(|_| storage())?,
                    Payload {
                        msg: ciphertext,
                        aad: b"Plumivox session v1",
                    },
                )
                .map_err(|_| storage())?,
        ))
    }
    #[cfg(not(target_os = "macos"))]
    pub fn prepare(_root: &Path) -> Result<()> {
        Err(storage())
    }
    #[cfg(not(target_os = "macos"))]
    pub fn encrypt(_plain: &[u8], _root: Option<&Path>) -> Result<Vec<u8>> {
        Err(storage())
    }
    #[cfg(not(target_os = "macos"))]
    pub fn decrypt(_encrypted: &[u8], _root: Option<&Path>) -> Result<Zeroizing<Vec<u8>>> {
        Err(storage())
    }

    #[cfg(all(test, target_os = "macos"))]
    mod tests {
        use super::*;
        #[cfg(feature = "test-fixture")]
        use std::os::unix::fs::DirBuilderExt;

        #[test]
        fn reads_existing_node_session_through_product_decoder() {
            // Node標準cryptoが生成した既存暗号文と16byte tagをそのまま維持する。
            let mut encrypted = b"PLUMIVOX-KEYCHAIN-1\n".to_vec();
            encrypted.extend(0u8..12);
            encrypted.extend(hex::decode("347bb86fad80b672ee6cf2f3d89a0c04edb1aa4795082c155709429abf0620d16a4779e1658b9fd0992b").unwrap());
            assert_eq!(
                decrypt_with_key(&encrypted, || Ok(Zeroizing::new((0u8..32).collect())))
                    .unwrap()
                    .as_slice(),
                b"synthetic-existing-session"
            );
            for invalid in [
                b"invalid-prefix".as_slice(),
                b"PLUMIVOX-KEYCHAIN-1\nshort".as_slice(),
            ] {
                let loaded = std::cell::Cell::new(false);
                let result = decrypt_with_key(invalid, || {
                    loaded.set(true);
                    Err(storage())
                });
                assert_eq!(result.unwrap_err().code, "SESSION_STORAGE_UNAVAILABLE");
                assert!(
                    !loaded.get(),
                    "invalid format must be rejected before loading a key"
                );
            }
        }

        #[cfg(feature = "test-fixture")]
        #[test]
        fn existing_key_reads_do_not_contend_with_key_creation_lock() {
            // Use the real OS Keychain backend, including an explicitly selected
            // fixture keychain when present. An unavailable Keychain is a failure.
            let parent = normalized_canonical(&std::env::temp_dir()).unwrap();
            let root = parent.join(format!("plumivox-keychain-lock-test-{}", random_id()));
            validate_directory(&root).unwrap();
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            private(&root).unwrap();
            println!("retained synthetic keychain test root: {}", root.display());
            prepare(&root).unwrap();
            let cookie = b"synthetic-cookie-keychain-lock-test";
            let encrypted = encrypt(cookie, Some(&root)).unwrap();

            // This deterministically reproduces the previous AUTH_BUSY failure:
            // normal decryption must read the existing key without taking this lock.
            let creation_lock = Lock::acquire(&root).unwrap();
            assert_eq!(decrypt(&encrypted, Some(&root)).unwrap().as_slice(), cookie);
            std::thread::scope(|threads| {
                for _ in 0..4 {
                    let root = &root;
                    let encrypted = &encrypted;
                    threads.spawn(move || {
                        for _ in 0..4 {
                            assert_eq!(decrypt(encrypted, Some(root)).unwrap().as_slice(), cookie);
                        }
                    });
                }
            });
            creation_lock.assert_owned().unwrap();
        }
    }
}
