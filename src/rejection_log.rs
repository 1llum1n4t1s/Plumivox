//! 提供元の拒否文面のローカル記録（利用者が期限付きで有効にしたときだけ）。
//!
//! 伏字処理は `api_rejection` が行い、ここは保存・上限・排他・CLI（`--rejection-log`）だけを担う。
//! 保存先は状態ルート（[`session::state_root`]）の下の `diagnostics\` で、本人だけが読める。
//! 記録は JSON Lines（1行1件）。古さ14日・200件・256KiB を超えたら古い順に消す。
//! 記録の失敗は投稿・再試行の結果に影響させない（[`append`] は真偽値だけを返す）。
//! 記録そのもの（`show` の出力）は人向けの辞書を通さない。
use crate::{
    error::Result,
    locale::{tr, tr_with},
    session,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::{
    fs::{self, File, TryLockError},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

const DIRECTORY: &str = "diagnostics";
const SETTING_FILE: &str = "rejection-log.json";
const LOG_FILE: &str = "provider-rejections.jsonl";
const LOCK_FILE: &str = "provider-rejections.lock";
const MAX_AGE_SECS: i64 = 14 * 24 * 60 * 60;
const MAX_ENTRIES: usize = 200;
const MAX_BYTES: usize = 256 * 1024;
const DEFAULT_DAYS: u64 = 7;
const MAX_DAYS: u64 = 30;
const DAY_SECS: u64 = 24 * 60 * 60;
/// 排他を取る試行の間隔と回数（合計約500ms）。
const LOCK_STEP: Duration = Duration::from_millis(20);
const LOCK_ATTEMPTS: u32 = 25;
const WRITE_DEADLINE: Duration = Duration::from_secs(1);

/// 検査済みの `diagnostics\`。`create` が偽でフォルダーが無ければ None。
/// `create` が真なら、無ければ作り、作成後・既存のどちらでも本人限定にする。
/// 状態ルート・フォルダーがリンク・検査失敗なら Err（書かない・消さない）。
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

/// 排他ロック（`provider-rejections.lock`）。リンクは開かない。約500msまで待って取れなければ Err。
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
fn replace_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let destination = dir.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(meta) if meta.is_file() && !session::is_link(&meta) => {}
        Ok(_) => return Err(session::storage()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(session::storage()),
    }
    let temporary = dir.join(format!(".rejection-{:016x}.tmp", rand::random::<u64>()));
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

/// 1行ずつ読み、JSON でない行・`recordedAt` を読めない行は捨てる。
fn parse_entries(bytes: &[u8]) -> Vec<Entry> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let stamp = value.get("recordedAt")?.as_str()?;
            let at = DateTime::parse_from_rfc3339(stamp).ok()?.timestamp();
            Some(Entry {
                at,
                line: line.to_owned(),
            })
        })
        .collect()
}

/// 上限を超えた分を古い順に消す。古さ（14日）→件数（200）→合計バイト数（256KiB）の順。
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

/// 有効化の設定。読めない・不正・リンクなら None（無効扱い）。
/// 不正の条件: オブジェクトでない、2つの欄が整数でない、`enabled_at` が `enabled_until` 以上、
/// 期間が30日を超える。
fn enabled_until(dir: &Path) -> Option<u64> {
    let path = dir.join(SETTING_FILE);
    fs::symlink_metadata(&path).ok()?;
    let value: Value = serde_json::from_slice(&session::bounded_read(&path).ok()?).ok()?;
    let until = value.get("enabled_until")?.as_u64()?;
    let at = value.get("enabled_at")?.as_u64()?;
    (at < until && until - at <= MAX_DAYS * DAY_SECS).then_some(until)
}

fn is_enabled_in(root: &Path, now: DateTime<Utc>) -> bool {
    let Ok(Some(dir)) = directory(root, false) else {
        return false;
    };
    enabled_until(&dir).is_some_and(|until| now.timestamp() < until as i64)
}

fn stamped(entry: Map<String, Value>, now: DateTime<Utc>) -> Option<String> {
    let mut out = Map::new();
    out.insert("schema".into(), json!(1));
    out.insert(
        "recordedAt".into(),
        json!(now.to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    out.insert("plumivoxVersion".into(), json!(env!("CARGO_PKG_VERSION")));
    out.insert("os".into(), json!(std::env::consts::OS));
    for (key, value) in entry {
        out.entry(key).or_insert(value);
    }
    serde_json::to_string(&Value::Object(out)).ok()
}

/// 1件を追記する。無効・期限切れ・大きすぎる場合は Ok(false)（その1件は捨てる）。
/// 排他が取れない・保存先の検査に失敗した場合は Err。
fn append_in(root: &Path, entry: Value, now: DateTime<Utc>) -> Result<bool> {
    let Value::Object(entry) = entry else {
        return Ok(false);
    };
    if !is_enabled_in(root, now) {
        return Ok(false);
    }
    let Some(line) = stamped(entry, now).filter(|line| line.len() < MAX_BYTES) else {
        return Ok(false);
    };
    let Some(dir) = directory(root, true)? else {
        return Ok(false);
    };
    let _guard = lock(&dir)?;
    let mut entries = parse_entries(&read_log(&dir)?);
    enforce_limits(&mut entries, now.timestamp());
    entries.push(Entry {
        at: now.timestamp(),
        line,
    });
    enforce_limits(&mut entries, now.timestamp());
    replace_file(&dir, LOG_FILE, &render(&entries))?;
    Ok(true)
}

/// 書込みの途中で止まって残った一時ファイル（`diagnostics\` 直下の `.rejection-*.tmp`）のうち、
/// リンクでない通常ファイルだけを消す。排他を取った状態で呼ぶ（書込み中のものを消さないため）。
fn remove_temporaries(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(".rejection-") && name.ends_with(".tmp")) {
            continue;
        }
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file() && !session::is_link(&meta))
        {
            let _ = fs::remove_file(&path);
        }
    }
}

/// 古いものを消す。`diagnostics\` が無ければ何もしない（フォルダーを作らない）。一時ファイルの消し残しも消す。
fn prune_in(root: &Path, now: DateTime<Utc>) -> Result<()> {
    let Some(dir) = directory(root, false)? else {
        return Ok(());
    };
    let _guard = lock(&dir)?;
    remove_temporaries(&dir);
    if fs::symlink_metadata(dir.join(LOG_FILE)).is_err() {
        return Ok(());
    }
    let bytes = read_log(&dir)?;
    let mut entries = parse_entries(&bytes);
    let before = entries.len();
    enforce_limits(&mut entries, now.timestamp());
    let rendered = render(&entries);
    if entries.len() != before || rendered.len() != bytes.len() {
        replace_file(&dir, LOG_FILE, &rendered)?;
    }
    Ok(())
}

/// 記録が有効か（現在時刻で期限を判定する）。読めない・不正・期限切れ・リンクは無効。
/// `root` は MCP サーバーが実際に使う状態ルート（`SessionStorage` と同じ値）。
pub(crate) fn is_enabled(root: &Path) -> bool {
    is_enabled_in(root, Utc::now())
}

/// 伏字処理済みの1件（記録の形の完全なオブジェクト）に `schema`・`recordedAt`・`plumivoxVersion`・`os` を
/// 付けて、状態ルート `root` の下へ追記する。書込み全体は `spawn_blocking` で行い1秒で打ち切る。成功で true。
pub(crate) async fn append(root: PathBuf, entry: Value) -> bool {
    let task =
        tokio::task::spawn_blocking(move || append_in(&root, entry, Utc::now()).unwrap_or(false));
    matches!(
        tokio::time::timeout(WRITE_DEADLINE, task).await,
        Ok(Ok(true))
    )
}

/// stdio の起動時に背景で呼ぶ。`diagnostics\` があるときだけ古いものを消す（失敗は無視する）。
pub(crate) fn prune_on_startup() {
    if let Some(root) = session::state_root(None) {
        let _ = prune_in(&root, Utc::now());
    }
}

/// [`prune_on_startup`] を別スレッドで始める（起動を待たせない）。
pub fn spawn_startup_prune() {
    let _ = std::thread::Builder::new()
        .name("rejection-log-prune".into())
        .spawn(prune_on_startup);
}

enum Command {
    On(u64),
    Off,
    Status,
    Show,
    Clear,
}

enum Rejected {
    Usage,
    Days,
}

fn parse_command(args: &[String]) -> std::result::Result<Command, Rejected> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["status"] => Ok(Command::Status),
        ["off"] => Ok(Command::Off),
        ["show"] => Ok(Command::Show),
        ["clear"] => Ok(Command::Clear),
        ["on"] => Ok(Command::On(DEFAULT_DAYS)),
        ["on", "--days", days] => days
            .parse::<u64>()
            .ok()
            .filter(|days| (1..=MAX_DAYS).contains(days))
            .map(Command::On)
            .ok_or(Rejected::Days),
        _ => Err(Rejected::Usage),
    }
}

fn write_setting(dir: &Path, until: u64, at: u64) -> Result<()> {
    let bytes = serde_json::to_vec(&json!({"enabled_until": until, "enabled_at": at}))
        .map_err(|_| session::storage())?;
    replace_file(dir, SETTING_FILE, &bytes)
}

fn date_text(seconds: u64) -> String {
    DateTime::from_timestamp(seconds as i64, 0)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn record_count(dir: &Path) -> Result<usize> {
    Ok(parse_entries(&read_log(dir)?).len())
}

fn cmd_on(root: &Path, days: u64, now: DateTime<Utc>, out: &mut dyn Write) -> Result<()> {
    let dir = directory(root, true)?.ok_or_else(session::storage)?;
    let at = now.timestamp().max(0) as u64;
    let until = at + days * DAY_SECS;
    write_setting(&dir, until, at)?;
    let text = tr_with(
        "cli.rejection_log.on",
        &[
            ("days", &days.to_string()),
            ("until", &date_text(until)),
            ("path", &dir.join(LOG_FILE).display().to_string()),
        ],
    );
    let _ = writeln!(out, "{text}");
    Ok(())
}

fn cmd_off(root: &Path, now: DateTime<Utc>, out: &mut dyn Write) -> Result<()> {
    if let Some(dir) = directory(root, false)? {
        let until = now.timestamp().max(1) as u64;
        write_setting(&dir, until, until - 1)?;
    }
    let _ = writeln!(out, "{}", tr("cli.rejection_log.off"));
    Ok(())
}

fn cmd_status(root: &Path, now: DateTime<Utc>, out: &mut dyn Write) -> Result<()> {
    let dir = directory(root, false)?;
    let count = match &dir {
        Some(dir) => record_count(dir)?,
        None => 0,
    };
    let path = root.join(DIRECTORY).join(LOG_FILE).display().to_string();
    let count = count.to_string();
    let until = dir
        .as_deref()
        .and_then(enabled_until)
        .filter(|until| now.timestamp() < *until as i64);
    let text = match until {
        Some(until) => tr_with(
            "cli.rejection_log.status_on",
            &[
                ("until", &date_text(until)),
                ("count", &count),
                ("path", &path),
            ],
        ),
        None => tr_with(
            "cli.rejection_log.status_off",
            &[("count", &count), ("path", &path)],
        ),
    };
    let _ = writeln!(out, "{text}");
    Ok(())
}

fn cmd_show(root: &Path, now: DateTime<Utc>, out: &mut dyn Write) -> Result<()> {
    let Some(dir) = directory(root, false)? else {
        return Ok(());
    };
    // 古いものの削除に失敗しても（排他が取れないなど）、読める分は出す。
    let _ = prune_in(root, now);
    for entry in parse_entries(&read_log(&dir)?) {
        let _ = writeln!(out, "{}", entry.line);
    }
    Ok(())
}

fn cmd_clear(root: &Path, out: &mut dyn Write) -> Result<()> {
    if let Some(dir) = directory(root, false)? {
        let _guard = lock(&dir)?;
        remove_temporaries(&dir);
        let path = dir.join(LOG_FILE);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !session::is_link(&meta) => {
                fs::remove_file(&path).map_err(|_| session::storage())?;
            }
            Ok(_) => return Err(session::storage()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(session::storage()),
        }
    }
    let _ = writeln!(out, "{}", tr("cli.rejection_log.cleared"));
    Ok(())
}

fn cli(
    root: Option<PathBuf>,
    args: &[String],
    now: DateTime<Utc>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let command = match parse_command(args) {
        Ok(command) => command,
        Err(Rejected::Days) => {
            let _ = writeln!(err, "{}", tr("cli.rejection_log.days_invalid"));
            return 2;
        }
        Err(Rejected::Usage) => {
            let _ = writeln!(err, "{}", tr("cli.usage"));
            return 2;
        }
    };
    let result = match root {
        None => Err(session::storage()),
        Some(root) => match command {
            Command::On(days) => cmd_on(&root, days, now, out),
            Command::Off => cmd_off(&root, now, out),
            Command::Status => cmd_status(&root, now, out),
            Command::Show => cmd_show(&root, now, out),
            Command::Clear => cmd_clear(&root, out),
        },
    };
    match result {
        Ok(()) => 0,
        Err(_) => {
            let _ = writeln!(err, "{}", tr("cli.rejection_log.unavailable"));
            1
        }
    }
}

/// `plumivox --rejection-log <on [--days N]|off|status|show|clear>`。終了コードは、成功 0、
/// 保存先の異常 1、引数の誤り 2。標準出力を使ってよい（stdio MCP ではない）。
pub fn run_cli(args: &[String]) -> i32 {
    cli(
        session::state_root(None),
        args,
        Utc::now(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_temporaries_deletes_only_regular_stale_temporaries() {
        let dir = std::env::temp_dir().join(format!(
            "plumivox-rejection-tmp-{:016x}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&dir).unwrap();
        let stale = dir.join(".rejection-0123456789abcdef.tmp");
        let keep = [
            dir.join(".rejection-0123456789abcdef.txt"),
            dir.join("other.tmp"),
            dir.join(LOG_FILE),
        ];
        fs::write(&stale, b"x").unwrap();
        for path in &keep {
            fs::write(path, b"x").unwrap();
        }
        let directory = dir.join(".rejection-folder.tmp");
        fs::create_dir(&directory).unwrap();
        remove_temporaries(&dir);
        let survived = keep.iter().all(|path| path.exists()) && directory.is_dir();
        let removed = !stale.exists();
        fs::remove_dir_all(&dir).unwrap();
        assert!(removed, "stale temporary file was not removed");
        assert!(survived, "unrelated entries must be kept");
    }
}
