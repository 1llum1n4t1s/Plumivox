//! 管理プロキシの中継（共有 daemon と SSH 逆転送）の出来事の診断記録。
//!
//! 投稿の拒否が増えた時期に経路が変わったかを後から切り分けるため、daemon の起動・終了と中継の状態の
//! 移り変わり（starting・ready・reconnecting・failed・stopped）を、固定のコードと時刻だけで残す。
//! 保存先は状態ルート（[`session::state_root`]）の下の `relay\events.jsonl`。常に記録し、有効化の操作は無い。
//! ホスト・ポート・鍵・ユーザー名・IP・提供元や SSH の生エラー文は記録しない（設定は `config` の
//! SHA-256 の先頭12桁で見分ける）。古さ14日・500件・128KiB を超えたら古い順に消す。
//! 書き込みは1つの書き手タスクが1件ずつ `spawn_blocking` で行い、1件を1秒で打ち切る。失敗は無視し、
//! 中継の処理を止めない・遅らせない。閲覧は CLI（`--relay-log show`）だけで、MCP ツールは無い。
use crate::{
    error::Result,
    locale::{tr, tr_with},
    session,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use std::{
    fs::{self, File, TryLockError},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

const DIRECTORY: &str = "relay";
const LOG_FILE: &str = "events.jsonl";
const LOCK_FILE: &str = "events.lock";
const SCHEMA: u32 = 1;
const MAX_AGE_SECS: i64 = 14 * 24 * 60 * 60;
const MAX_ENTRIES: usize = 500;
const MAX_BYTES: usize = 128 * 1024;
/// 排他を取る試行の間隔と回数（合計約500ms）。
const LOCK_STEP: Duration = Duration::from_millis(20);
const LOCK_ATTEMPTS: u32 = 25;
const WRITE_DEADLINE: Duration = Duration::from_secs(1);
const FLUSH_DEADLINE: Duration = Duration::from_secs(2);
/// 書き手タスクへの待ち行列の長さ。あふれた分は捨てる（中継を待たせないため）。
const QUEUE: usize = 64;
/// 設定の識別値の桁数。
const CONFIG_DIGITS: usize = 12;

/// daemon が記録を書く状態ルート。本番は常に `state_root(None)`。試験用ビルドは
/// `PLUMIVOX_STATE_DIR` が設定されているときだけ書く（未設定のまま本物の保存先へ書かないため）。
pub(crate) fn daemon_root() -> Option<PathBuf> {
    #[cfg(feature = "test-fixture")]
    std::env::var_os("PLUMIVOX_STATE_DIR").filter(|value| !value.is_empty())?;
    session::state_root(None)
}

/// 検査済みの `relay\`。`create` が偽でフォルダーが無ければ None。
/// `create` が真なら、無ければ作り、作成後・既存のどちらでも本人限定にする。
/// 状態ルート・フォルダーがリンク・検査失敗なら Err（書かない）。
fn directory(root: &Path, create: bool) -> Result<Option<PathBuf>> {
    session::validate_directory(root)?;
    let dir = root.join(DIRECTORY);
    session::validate_directory(&dir)?;
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_dir() && !session::is_link(&meta) => {}
        Ok(_) => return Err(session::storage()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            fs::create_dir_all(&dir).map_err(|_| session::storage())?;
            session::validate_directory(&dir)?;
        }
        Err(_) => return Err(session::storage()),
    }
    if create {
        session::private_directory(&session::normalized_canonical(&dir)?)?;
    }
    Ok(Some(dir))
}

/// 排他ロック（`events.lock`）。リンクは開かない。約500msまで待って取れなければ Err。
fn lock(dir: &Path) -> Result<File> {
    let mut options = session::read_options();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(dir.join(LOCK_FILE))
        .map_err(|_| session::storage())?;
    let meta = file.metadata().map_err(|_| session::storage())?;
    if !meta.is_file() || session::is_link(&meta) {
        return Err(session::storage());
    }
    for _ in 0..LOCK_ATTEMPTS {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => std::thread::sleep(LOCK_STEP),
            Err(TryLockError::Error(_)) => return Err(session::storage()),
        }
    }
    Err(session::storage())
}

/// 同じフォルダーの一時ファイル（本人限定）へ書いてから置き換える。置換先が通常ファイル以外なら書かない。
fn replace_file(dir: &Path, bytes: &[u8]) -> Result<()> {
    let destination = dir.join(LOG_FILE);
    match fs::symlink_metadata(&destination) {
        Ok(meta) if meta.is_file() && !session::is_link(&meta) => {}
        Ok(_) => return Err(session::storage()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(session::storage()),
    }
    let temporary = dir.join(format!(".relay-{:016x}.tmp", rand::random::<u64>()));
    let written = (|| -> Result<()> {
        let mut file = session::new_private_file(&temporary)?;
        file.write_all(bytes).map_err(|_| session::storage())?;
        file.sync_all().map_err(|_| session::storage())?;
        drop(file);
        fs::rename(&temporary, &destination).map_err(|_| session::storage())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// 書込みの途中で止まって残った一時ファイル（`relay\` 直下の `.relay-*.tmp`）のうち、リンクでない
/// 通常ファイルだけを消す。排他を取った状態で呼ぶ（書込み中のものを消さないため）。
fn remove_temporaries(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(".relay-") && name.ends_with(".tmp")) {
            continue;
        }
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file() && !session::is_link(&meta))
        {
            let _ = fs::remove_file(&path);
        }
    }
}

/// 記録ファイルの全文。無ければ空。通常ファイル・非リンク・サイズ上限内でなければ Err。
fn read_log(dir: &Path) -> Result<Vec<u8>> {
    let path = dir.join(LOG_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => session::bounded_read(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(session::storage()),
    }
}

struct Entry {
    at: i64,
    line: String,
}

/// 1行ずつ読み、JSON でない行・`at` を読めない行は捨てる。
fn parse_entries(bytes: &[u8]) -> Vec<Entry> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let at = DateTime::parse_from_rfc3339(value.get("at")?.as_str()?)
                .ok()?
                .timestamp();
            Some(Entry {
                at,
                line: line.to_owned(),
            })
        })
        .collect()
}

/// 上限を超えた分を古い順に消す。古さ（14日）→件数（500）→合計バイト数（128KiB）の順。
fn enforce_limits(entries: &mut Vec<Entry>, now: i64) {
    entries.retain(|entry| entry.at >= now - MAX_AGE_SECS);
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    let mut total: usize = entries.iter().map(|entry| entry.line.len() + 1).sum();
    let mut drop_count = 0;
    while total > MAX_BYTES && drop_count < entries.len() {
        total -= entries[drop_count].line.len() + 1;
        drop_count += 1;
    }
    entries.drain(..drop_count);
}

fn render(entries: &[Entry]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in entries {
        bytes.extend_from_slice(entry.line.as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

/// 1件を追記して上限を適用する。排他が取れない・保存先の検査に失敗した場合は Err。
fn append_in(root: &Path, entry: Entry, now: i64) -> Result<()> {
    let dir = directory(root, true)?.ok_or_else(session::storage)?;
    let _guard = lock(&dir)?;
    remove_temporaries(&dir);
    let mut entries = parse_entries(&read_log(&dir)?);
    entries.push(entry);
    enforce_limits(&mut entries, now);
    replace_file(&dir, &render(&entries))
}

/// 識別子（固定コード・出来事名・設定の識別値）として記録・表示してよい文字列か。
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

enum Message {
    Write(Entry),
    Flush(oneshot::Sender<()>),
}

/// daemon が持つ記録の書き込み口。複製は同じ書き手タスクへ送る。
#[derive(Clone)]
pub(crate) struct RelayLog {
    sender: mpsc::Sender<Message>,
    config: String,
}

impl RelayLog {
    /// 書き手タスクを始める（tokio のランタイムの中で呼ぶ）。`config` は設定の SHA-256（16進）。
    pub(crate) fn start(root: PathBuf, config: &str) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Message>(QUEUE);
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    Message::Write(entry) => {
                        let root = root.clone();
                        let task = tokio::task::spawn_blocking(move || {
                            let _ = append_in(&root, entry, Utc::now().timestamp());
                        });
                        let _ = tokio::time::timeout(WRITE_DEADLINE, task).await;
                    }
                    Message::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self {
            sender,
            config: config.chars().take(CONFIG_DIGITS).collect(),
        }
    }

    /// 1件を待ち行列へ入れる。待たず、あふれたら捨てる。`code` は固定のエラーコードだけを渡す。
    pub(crate) fn record(&self, event: &str, code: Option<&str>, retry: Option<u32>) {
        let now = Utc::now();
        let mut value = json!({
            "schema": SCHEMA,
            "at": now.to_rfc3339_opts(SecondsFormat::Millis, true),
            "event": event,
            "config": self.config,
        });
        if let Some(code) = code {
            value["code"] = json!(if is_token(code) { code } else { "UNKNOWN" });
        }
        if let Some(retry) = retry {
            value["retry"] = json!(retry);
        }
        if let Ok(line) = serde_json::to_string(&value) {
            let _ = self.sender.try_send(Message::Write(Entry {
                at: now.timestamp(),
                line,
            }));
        }
    }

    /// 待ち行列の分が書かれる（または打ち切られる）まで、最大2秒待つ。daemon の終了の直前に呼ぶ。
    pub(crate) async fn flush(&self) {
        let (done, wait) = oneshot::channel();
        let _ = tokio::time::timeout(FLUSH_DEADLINE, async {
            if self.sender.send(Message::Flush(done)).await.is_ok() {
                let _ = wait.await;
            }
        })
        .await;
    }
}

fn field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| is_token(text))
        .unwrap_or("-")
        .to_owned()
}

/// 記録の1行を表示用の1行にする（辞書の書式）。
fn describe(entry: &Entry) -> String {
    let value: Value = serde_json::from_str(&entry.line).unwrap_or_default();
    let retry = value
        .get("retry")
        .and_then(Value::as_u64)
        .map_or_else(|| "-".to_owned(), |retry| retry.to_string());
    let at = DateTime::from_timestamp(entry.at, 0)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default();
    tr_with(
        "cli.relay_log.line",
        &[
            ("at", &at),
            ("event", &field(&value, "event")),
            ("code", &field(&value, "code")),
            ("retry", &retry),
            ("config", &field(&value, "config")),
        ],
    )
}

fn show(root: &Path, now: i64, out: &mut dyn Write) -> Result<()> {
    let mut entries = match directory(root, false)? {
        Some(dir) => parse_entries(&read_log(&dir)?),
        None => Vec::new(),
    };
    enforce_limits(&mut entries, now);
    if entries.is_empty() {
        let _ = writeln!(out, "{}", tr("cli.relay_log.none"));
        return Ok(());
    }
    let path = root.join(DIRECTORY).join(LOG_FILE).display().to_string();
    let _ = writeln!(out, "{}", tr_with("cli.relay_log.head", &[("path", &path)]));
    for entry in &entries {
        let _ = writeln!(out, "{}", describe(entry));
    }
    Ok(())
}

fn cli(
    root: Option<PathBuf>,
    args: &[String],
    now: i64,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if args != ["show"] {
        let _ = writeln!(err, "{}", tr("cli.usage"));
        return 2;
    }
    match root.map(|root| show(&root, now, out)) {
        Some(Ok(())) => 0,
        _ => {
            let _ = writeln!(err, "{}", tr("cli.relay_log.unavailable"));
            1
        }
    }
}

/// `plumivox --relay-log show`。終了コードは、成功 0、保存先の異常 1、引数の誤り 2。
/// 標準出力を使ってよい（stdio MCP ではない）。
pub fn run_cli(args: &[String]) -> i32 {
    cli(
        session::state_root(None),
        args,
        Utc::now().timestamp(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
