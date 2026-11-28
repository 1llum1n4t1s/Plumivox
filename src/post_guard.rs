//! X のユーザー名ごとの投稿の一時停止（冷却）。全プロセス共通の状態ファイルで管理する。
//!
//! 同じ X アカウントへ別々のセッションから短い間隔で投稿すると、X に自動化を疑われて拒否される。
//! 拒否の直後の再送はさらに悪化させる。そこで、冷却中の `post_tweet` は X へ何も送らず（提供元の課金も
//! 発生させず）断る。状態は `<状態ルート>/post-guard/<hash>.json`、排他は同じフォルダーの `<hash>.lock`。
//! `hash` は `post-guard\n` + 小文字のユーザー名（設定名ではなく X のユーザー名）の SHA-256。
//!
//! 状態には固定のコードと分類だけを入れる。本文・提供元の文面・ユーザー名・資格情報は入れない。
//! 投稿の自動再試行はしない。`finish` を呼ばずに終わると `inFlight` が残り、次の `acquire` が
//! `interrupted`（成否不明）として冷却をかける。保存先が使えないときは送信せず止める（fail-closed）。
//!
//! 画面・CLI へ出す文言はここでは作らない。一覧（[`list`]）と解除（[`clear`]）はデータだけを返す。
use crate::{
    api_rejection,
    error::{AppError, Result},
    session,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, TryLockError},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

const DIRECTORY: &str = "post-guard";
const SCHEMA: u32 = 1;
const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_HISTORY: usize = 32;
const MAX_STRIKES: u32 = 64;
/// 冷却の長さの上限（分）。
const MAX_COOLDOWN_MINUTES: u64 = 24 * 60;
/// strike を 0 に戻すまでの間隔（分）。
const STRIKE_RESET_MINUTES: u64 = 48 * 60;
/// 保存した時刻が今よりこれ以上未来なら、時計が戻ったとみなす。
const CLOCK_SKEW_SECS: u64 = 120;
const MIN_INTERVAL_MINUTES: u64 = 5;
const UNCERTAIN_MINUTES: u64 = 15;
/// 排他を取る試行の間隔と回数（合計約500ms）。
const LOCK_STEP: Duration = Duration::from_millis(20);
const LOCK_ATTEMPTS: u32 = 25;

/// 状態に保存する `cooldown.reason` の取りうる値（応答の `cooldownReason` と同じ）。
const REASONS: [&str; 10] = [
    "min_interval",
    "post_in_progress",
    "previous_uncertain",
    "interrupted",
    "guard_state_reset",
    "automation_suspected",
    "daily_limit",
    "rate_limit",
    "account_locked",
    "unclassified_rejection",
];

/// 制限系の分類と、strike を数える冷却の基本の長さ（分）。理由は応答の `cooldownReason`。
const STRIKE_RULES: [(&str, &str, u64); 5] = [
    ("rate_limit", "rate_limit", 15),
    ("automation_suspected", "automation_suspected", 180),
    ("daily_limit", "daily_limit", 360),
    ("account_locked", "account_locked", 24 * 60),
    ("account_suspended", "account_locked", 24 * 60),
];

/// 提供元だけで止まり、X まで届いていない拒否の分類。
const PROVIDER_ONLY: [&str; 3] = ["api_key", "credits", "proxy"];

/// 時間の規則。本番は1分=60000ms。試験用ビルドだけが上書きできる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rules {
    minute_ms: u64,
}

impl Rules {
    #[cfg(any(test, not(feature = "test-fixture")))]
    pub(crate) const PRODUCTION: Rules = Rules { minute_ms: 60_000 };

    /// `minutes` 分の秒数（切り上げ、最低1秒）。
    fn secs(self, minutes: u64) -> u64 {
        minutes.saturating_mul(self.minute_ms).div_ceil(1000).max(1)
    }
}

/// 有効な規則。None はガード無効（試験用ビルドで未設定・`off`・不正な値のとき）。
/// 本番ビルドは環境変数を読まず、常に有効。
fn active_rules() -> Option<Rules> {
    #[cfg(feature = "test-fixture")]
    {
        // 値は `off`（無効）か、1分の代わりにするミリ秒。未設定も無効（既存の E2E を壊さないため）。
        let value = std::env::var("PLUMIVOX_POST_GUARD_TEST").ok()?;
        value
            .parse::<u64>()
            .ok()
            .filter(|ms| *ms >= 1)
            .map(|minute_ms| Rules { minute_ms })
    }
    #[cfg(not(feature = "test-fixture"))]
    {
        Some(Rules::PRODUCTION)
    }
}

fn now_secs() -> u64 {
    u64::try_from(Utc::now().timestamp()).unwrap_or(0)
}

fn rfc3339(secs: u64) -> String {
    DateTime::from_timestamp(i64::try_from(secs).unwrap_or(0), 0)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ---- 状態 ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct State {
    schema: u32,
    /// 設定名。X のユーザー名と一致するときは `<user>`。
    account: String,
    updated_at: u64,
    last_attempt_at: Option<u64>,
    in_flight: Option<InFlight>,
    cooldown: Option<Cooldown>,
    strikes: u32,
    last_strike_at: Option<u64>,
    history: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InFlight {
    started_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Cooldown {
    reason: String,
    from: u64,
    until: u64,
}

/// 履歴の1件。固定のコードと分類だけを持つ。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Event {
    pub(crate) at: u64,
    pub(crate) stage: String,
    pub(crate) code: String,
    pub(crate) reasons: Vec<String>,
    pub(crate) x_error_code: Option<u32>,
    pub(crate) x_http_status: Option<u16>,
}

impl State {
    fn new(label: &str, now: u64) -> Self {
        Self {
            schema: SCHEMA,
            account: label.to_owned(),
            updated_at: now,
            last_attempt_at: None,
            in_flight: None,
            cooldown: None,
            strikes: 0,
            last_strike_at: None,
            history: Vec::new(),
        }
    }

    /// 読み込んだ状態が、この版の規則で解釈できる形か。
    fn is_valid(&self) -> bool {
        let short = |text: &str| text.len() <= 64;
        self.schema == SCHEMA
            && self.account.len() <= 256
            && self.strikes <= MAX_STRIKES
            && self.history.len() <= MAX_HISTORY
            && self.cooldown.as_ref().is_none_or(|cooldown| {
                REASONS.contains(&cooldown.reason.as_str()) && cooldown.from <= cooldown.until
            })
            && self.history.iter().all(|event| {
                short(&event.stage)
                    && short(&event.code)
                    && event.reasons.len() <= 8
                    && event.reasons.iter().all(|reason| short(reason))
            })
    }

    fn push_event(&mut self, event: Event) {
        self.history.push(event);
        if self.history.len() > MAX_HISTORY {
            let extra = self.history.len() - MAX_HISTORY;
            self.history.drain(..extra);
        }
    }
}

fn guard_event(now: u64, code: &str) -> Event {
    Event {
        at: now,
        stage: "post_guard".to_owned(),
        code: code.to_owned(),
        reasons: Vec::new(),
        x_error_code: None,
        x_http_status: None,
    }
}

/// 新しい冷却を既存と合わせる。新しい期限が既存の期限より前（同じを含む）なら既存を残す。
fn merge_cooldown(state: &mut State, reason: &str, secs: u64, now: u64, rules: Rules) {
    let secs = secs.min(rules.secs(MAX_COOLDOWN_MINUTES));
    let until = now.saturating_add(secs);
    if state
        .cooldown
        .as_ref()
        .is_some_and(|cooldown| cooldown.until > now && cooldown.until >= until)
    {
        return;
    }
    state.cooldown = Some(Cooldown {
        reason: reason.to_owned(),
        from: now,
        until,
    });
}

/// 保存した時刻が今より120秒以上未来なら時計が戻ったとみなし、安全側に置き直す。
/// 冷却は `from` を今に、`until` を今＋元の長さ（上限24時間）にする。
/// 最後の投稿の試行が未来なら、最小間隔の冷却を今からかける。
fn fix_clock(state: &mut State, now: u64, rules: Rules) {
    let limit = now.saturating_add(CLOCK_SKEW_SECS);
    let cap = rules.secs(MAX_COOLDOWN_MINUTES);
    if let Some(cooldown) = &mut state.cooldown {
        if cooldown.from > limit {
            let length = cooldown.until.saturating_sub(cooldown.from).min(cap);
            cooldown.from = now;
            cooldown.until = now.saturating_add(length);
        }
        cooldown.until = cooldown.until.min(now.saturating_add(cap));
    }
    if state.updated_at > limit {
        state.updated_at = now;
    }
    if let Some(at) = &mut state.last_strike_at
        && *at > limit
    {
        *at = now;
    }
    if let Some(flight) = &mut state.in_flight
        && flight.started_at > limit
    {
        flight.started_at = now;
    }
    for event in &mut state.history {
        if event.at > limit {
            event.at = now;
        }
    }
    if state.last_attempt_at.is_some_and(|at| at > limit) {
        state.last_attempt_at = Some(now);
        merge_cooldown(
            state,
            "min_interval",
            rules.secs(MIN_INTERVAL_MINUTES),
            now,
            rules,
        );
    }
}

/// 最後の strike から48時間たっていたら strikes を0に戻す。
fn decay_strikes(state: &mut State, now: u64, rules: Rules) {
    let expired = state
        .last_strike_at
        .is_none_or(|at| now >= at.saturating_add(rules.secs(STRIKE_RESET_MINUTES)));
    if state.strikes > 0 && expired {
        state.strikes = 0;
        state.last_strike_at = None;
    }
}

// ---- 結果の判定（純粋な規則） ----

#[derive(Clone, Debug, PartialEq)]
struct Candidate {
    reason: &'static str,
    minutes: u64,
    strike: bool,
}

const MIN_INTERVAL: Candidate = Candidate {
    reason: "min_interval",
    minutes: MIN_INTERVAL_MINUTES,
    strike: false,
};

/// 投稿の結果から決まる、冷却の候補と履歴の1件。候補が空なら記録しない（X まで届いていない・取り消し）。
#[derive(Debug)]
struct Verdict {
    success: bool,
    candidates: Vec<Candidate>,
    event: Event,
}

/// 小文字と `_` だけの短い固定語だけを残す（提供元の文面が履歴に混ざらないようにする）。
fn fixed_words(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|word| {
            !word.is_empty()
                && word.len() <= 40
                && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        })
        .take(8)
        .map(str::to_owned)
        .collect()
}

fn fixed_code(code: &str) -> String {
    let valid = !code.is_empty()
        && code.len() <= 40
        && code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_');
    if valid { code } else { "UNKNOWN" }.to_owned()
}

/// 結果を冷却の規則に当てはめる。
fn judge(result: &Result<Value>) -> Verdict {
    let mut event = Event {
        at: 0,
        stage: "post_creation".to_owned(),
        code: "OK".to_owned(),
        reasons: Vec::new(),
        x_error_code: None,
        x_http_status: None,
    };
    let Err(error) = result else {
        return Verdict {
            success: true,
            candidates: vec![MIN_INTERVAL],
            event,
        };
    };
    let details = &error.details;
    let stage = match details["stage"].as_str() {
        Some("image_upload") => "image_upload",
        _ => "post_creation",
    };
    event.stage = stage.to_owned();
    event.code = fixed_code(&error.code);
    event.reasons = fixed_words(&details["rejectionReasons"]);
    event.x_error_code = details["xErrorCode"]
        .as_u64()
        .and_then(|code| u32::try_from(code).ok());
    event.x_http_status = details["xHttpStatus"]
        .as_u64()
        .and_then(|status| u16::try_from(status).ok());
    let has = |name: &str| event.reasons.iter().any(|reason| reason == name);
    let attempted = details["postAttempted"] == true || error.code == "POST_UNCERTAIN";
    let x_rejected = api_rejection::is_x_rejected(details);

    let mut candidates = Vec::new();
    if error.code == "POST_UNCERTAIN" || has("post_id_missing") {
        candidates.push(Candidate {
            reason: "previous_uncertain",
            minutes: UNCERTAIN_MINUTES,
            strike: false,
        });
    }
    for (name, reason, minutes) in STRIKE_RULES {
        if has(name) {
            // 提供元自身の流量制限かもしれない rate_limit（X 由来と分からないもの）は strike にしない。
            let strike = name != "rate_limit" || x_rejected;
            candidates.push(Candidate {
                reason,
                minutes,
                strike,
            });
        }
    }
    if attempted && candidates.is_empty() && x_rejected {
        candidates.push(Candidate {
            reason: "unclassified_rejection",
            minutes: UNCERTAIN_MINUTES,
            strike: false,
        });
    }
    // 提供元だけで止まった拒否（キー・残高・プロキシ、HTTP 401/402）は X まで届いていない。
    // ただし投稿作成の段階の proxy は、中継が X へ送った後に切れた可能性があるので除く。
    let status = details["httpStatus"].as_u64();
    let relay_may_have_sent = stage == "post_creation" && has("proxy");
    let provider_only = candidates.is_empty()
        && !x_rejected
        && (matches!(status, Some(401 | 402))
            || (!event.reasons.is_empty()
                && !relay_may_have_sent
                && event
                    .reasons
                    .iter()
                    .all(|reason| PROVIDER_ONLY.contains(&reason.as_str()))));
    if attempted && !provider_only {
        candidates.push(MIN_INTERVAL);
    }
    Verdict {
        success: false,
        candidates,
        event,
    }
}

/// 冷却の長さ（秒）と理由。strike を数える規則は基本の長さ × 2^(strikes-1)。複数なら最大を採る。
fn longest(candidates: &[Candidate], strikes: u32, rules: Rules) -> Option<(&'static str, u64)> {
    let mut best: Option<(&'static str, u64)> = None;
    for candidate in candidates {
        let factor = if candidate.strike {
            1u64 << strikes.saturating_sub(1).min(12)
        } else {
            1
        };
        let minutes = candidate
            .minutes
            .saturating_mul(factor)
            .min(MAX_COOLDOWN_MINUTES);
        let secs = rules.secs(minutes);
        if best.is_none_or(|(_, longest)| secs > longest) {
            best = Some((candidate.reason, secs));
        }
    }
    best
}

// ---- 応答 ----

/// 冷却中の理由と期限。応答の組み立てに使う。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pause {
    pub(crate) reason: String,
    pub(crate) until: u64,
    pub(crate) now: u64,
}

const NEXT_STEP_INTERVAL: &str = "Nothing was sent. This is not an X rejection; Plumivox is only keeping the normal gap between posts. If the user still wants this post, call post_tweet once with the same content after retryAfter. Leave at least 5 minutes between posts, send multiple posts one at a time, and do not call post_tweet for this account before retryAfter.";
const NEXT_STEP_PAUSED: &str = "Tell the user the retryAfter time and that Plumivox paused posting from this account to avoid X limits. Do not call post_tweet for this account again until that time, and do not try to work around the pause.";

impl Pause {
    fn retry_after_seconds(&self) -> u64 {
        self.until.saturating_sub(self.now).max(1)
    }

    /// 拒否された投稿のエラーへ、冷却の理由と再開の目安を足す。
    fn annotate(&self, error: &mut AppError) {
        if !error.details.is_object() {
            error.details = json!({});
        }
        error.details["cooldownReason"] = json!(self.reason);
        error.details["retryAfter"] = json!(rfc3339(self.now + self.retry_after_seconds()));
        error.details["retryAfterSeconds"] = json!(self.retry_after_seconds());
    }

    fn into_error(self) -> AppError {
        let next_step = if matches!(self.reason.as_str(), "min_interval" | "post_in_progress") {
            NEXT_STEP_INTERVAL
        } else {
            NEXT_STEP_PAUSED
        };
        let mut error = AppError::new(
            "POST_COOLDOWN",
            "Posting from this account is paused by Plumivox to avoid X limits. The post was not sent.",
        )
        .with_details(json!({
            "stage": "post_guard",
            "postAttempted": false,
            "uploadAttempts": 0,
            "nextStep": next_step,
        }));
        self.annotate(&mut error);
        error
    }
}

/// 送信前に止める理由。
#[derive(Debug)]
enum Refusal {
    Pause(Pause),
    Unavailable,
}

impl Refusal {
    fn into_error(self) -> AppError {
        match self {
            Refusal::Pause(pause) => pause.into_error(),
            Refusal::Unavailable => AppError::new(
                "POST_GUARD_UNAVAILABLE",
                "Plumivox could not read or update its posting pause state, so the post was not sent.",
            )
            .with_details(json!({
                "stage": "post_guard",
                "postAttempted": false,
                "uploadAttempts": 0,
                "nextStep": "Do not retry in a loop. Tell the user that the local storage Plumivox uses for its posting pause state is unavailable, and ask them to check the storage location and its permissions.",
            })),
        }
    }
}

// ---- 保存・排他 ----

fn unavailable() -> Refusal {
    Refusal::Unavailable
}

/// 検査済みの `post-guard\`。`create` が偽でフォルダーが無ければ None。
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

/// ユーザー名（小文字化）から決まる保存名。
fn key_hash(user_name: &str) -> String {
    session::hash(&format!("post-guard\n{}", user_name.to_lowercase()))
}

enum Lock {
    Held(File),
    /// 約500ms待っても取れなかった（別の投稿が送信中）。
    Busy,
}

/// 排他ロック（`<hash>.lock`）。リンクは開かない。約500msまで待って取れなければ Busy。
fn lock(dir: &Path, hash: &str) -> Result<Lock> {
    let mut options = session::read_options();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(dir.join(format!("{hash}.lock")))
        .map_err(|_| session::storage())?;
    let meta = file.metadata().map_err(|_| session::storage())?;
    if !meta.is_file() || session::is_link(&meta) {
        return Err(session::storage());
    }
    for _ in 0..LOCK_ATTEMPTS {
        match file.try_lock() {
            Ok(()) => return Ok(Lock::Held(file)),
            Err(TryLockError::WouldBlock) => std::thread::sleep(LOCK_STEP),
            Err(TryLockError::Error(_)) => return Err(session::storage()),
        }
    }
    Ok(Lock::Busy)
}

/// 既にあるロックファイルが他で保持されているか（作らない・待たない）。開けないときは偽。
fn is_locked(dir: &Path, hash: &str) -> bool {
    let mut options = session::read_options();
    options.write(true);
    let Ok(file) = options.open(dir.join(format!("{hash}.lock"))) else {
        return false;
    };
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

enum Loaded {
    Fresh,
    State(Box<State>),
    /// 形式の誤り・上限超過の大きさ・schema が0などこの版で解釈できない形。
    Corrupt,
    /// この版より新しい schema の状態。作り直さず、書き込まずに拒否だけ返す。
    Newer,
}

/// 書込みの途中で止まって残った、この状態の一時ファイル（`.post-guard-<hash>-*.tmp`）のうち、
/// リンクでない通常ファイルだけを消す。排他を取った状態で呼ぶ（別のユーザー名の書込み中のものを
/// 消さないため、名前にハッシュを含むものだけを対象にする）。
fn remove_temporaries(dir: &Path, hash: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let prefix = format!(".post-guard-{hash}-");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(&prefix) && name.ends_with(".tmp")) {
            continue;
        }
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file() && !session::is_link(&meta))
        {
            let _ = fs::remove_file(&path);
        }
    }
}

fn state_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(format!("{hash}.json"))
}

/// 状態の読み込み。無ければ Fresh。リンク・通常ファイル以外・読めないときは Err（fail-closed）。
fn load(dir: &Path, hash: &str) -> Result<Loaded> {
    let path = state_path(dir, hash);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Loaded::Fresh),
        Err(_) => return Err(session::storage()),
    };
    if !meta.is_file() || session::is_link(&meta) {
        return Err(session::storage());
    }
    if meta.len() > MAX_STATE_BYTES {
        return Ok(Loaded::Corrupt);
    }
    let bytes = session::bounded_read(&path)?;
    let newer = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value["schema"].as_u64())
        .is_some_and(|schema| schema > u64::from(SCHEMA));
    if newer {
        return Ok(Loaded::Newer);
    }
    Ok(match serde_json::from_slice::<State>(&bytes) {
        Ok(state) if state.is_valid() => Loaded::State(Box::new(state)),
        _ => Loaded::Corrupt,
    })
}

/// 同じフォルダーの一時ファイル（本人限定）へ書いてから置き換える。置換先が通常ファイル以外なら書かない。
fn write_state(dir: &Path, hash: &str, state: &State) -> Result<()> {
    let bytes = serde_json::to_vec(state).map_err(|_| session::storage())?;
    let destination = state_path(dir, hash);
    match fs::symlink_metadata(&destination) {
        Ok(meta) if meta.is_file() && !session::is_link(&meta) => {}
        Ok(_) => return Err(session::storage()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(session::storage()),
    }
    let temporary = dir.join(format!(
        ".post-guard-{hash}-{:016x}.tmp",
        rand::random::<u64>()
    ));
    let written = (|| -> Result<()> {
        let mut file = session::new_private_file(&temporary)?;
        file.write_all(&bytes).map_err(|_| session::storage())?;
        file.sync_all().map_err(|_| session::storage())?;
        drop(file);
        fs::rename(&temporary, &destination).map_err(|_| session::storage())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

// ---- 取得・完了 ----

/// 送信前に取得した、同じユーザー名の投稿の排他と状態。`finish`・`release` を呼ばずに破棄すると
/// `inFlight` が残り、次の取得が `interrupted` として冷却をかける。
pub(crate) struct Guard {
    _lock: File,
    dir: PathBuf,
    hash: String,
    state: State,
    rules: Rules,
}

/// 冷却の有無を見て、送ってよければ `inFlight` を書き込んで Guard を返す。
fn acquire_at(
    root: &Path,
    user_name: &str,
    label: &str,
    now: u64,
    rules: Rules,
) -> std::result::Result<Guard, Refusal> {
    if user_name.trim().is_empty() {
        return Err(unavailable());
    }
    let dir = directory(root, true)
        .ok()
        .flatten()
        .ok_or_else(unavailable)?;
    let hash = key_hash(user_name.trim());
    let file = match lock(&dir, &hash) {
        Ok(Lock::Held(file)) => file,
        Ok(Lock::Busy) => {
            return Err(Refusal::Pause(Pause {
                reason: "post_in_progress".to_owned(),
                until: now.saturating_add(rules.secs(1)),
                now,
            }));
        }
        Err(_) => return Err(unavailable()),
    };
    remove_temporaries(&dir, &hash);
    let corrupt;
    let mut state = match load(&dir, &hash) {
        Ok(Loaded::Newer) => {
            // 新しい版が書いた状態は読み解けないので、上書きせず、書き込まずに拒否だけ返す。
            return Err(Refusal::Pause(Pause {
                reason: "guard_state_reset".to_owned(),
                until: now.saturating_add(rules.secs(UNCERTAIN_MINUTES)),
                now,
            }));
        }
        Ok(Loaded::State(state)) => {
            corrupt = false;
            *state
        }
        Ok(Loaded::Fresh) => {
            corrupt = false;
            State::new(label, now)
        }
        Ok(Loaded::Corrupt) => {
            corrupt = true;
            State::new(label, now)
        }
        Err(_) => return Err(unavailable()),
    };
    label.clone_into(&mut state.account);
    fix_clock(&mut state, now, rules);
    decay_strikes(&mut state, now, rules);
    let uncertain = rules.secs(UNCERTAIN_MINUTES);
    if corrupt {
        merge_cooldown(&mut state, "guard_state_reset", uncertain, now, rules);
        state.push_event(guard_event(now, "guard_state_reset"));
    }
    if state.in_flight.take().is_some() {
        merge_cooldown(&mut state, "interrupted", uncertain, now, rules);
        state.push_event(guard_event(now, "interrupted"));
    }
    state.updated_at = now;
    let paused = state
        .cooldown
        .clone()
        .filter(|cooldown| cooldown.until > now);
    if paused.is_none() {
        state.cooldown = None;
        state.in_flight = Some(InFlight { started_at: now });
    }
    // 冷却中の書き込み失敗は、拒否が変わらないので無視する。送信前の書き込み失敗は止める。
    let written = write_state(&dir, &hash, &state);
    if let Some(cooldown) = paused {
        return Err(Refusal::Pause(Pause {
            reason: cooldown.reason,
            until: cooldown.until,
            now,
        }));
    }
    written.map_err(|_| unavailable())?;
    Ok(Guard {
        _lock: file,
        dir,
        hash,
        state,
        rules,
    })
}

impl Guard {
    fn save(&mut self, now: u64) {
        self.state.updated_at = now;
        // 書けなければ `inFlight` が残り、次の取得が interrupted として扱う。
        let _ = write_state(&self.dir, &self.hash, &self.state);
    }

    /// 結果を記録して `inFlight` を消す。候補が空（X まで届いていない・取り消し）なら記録しない。
    /// 冷却が始まった・延びたエラーのときは、その冷却を返す。
    fn finish_at(mut self, verdict: &Verdict, now: u64) -> Option<Pause> {
        self.state.in_flight = None;
        let rules = self.rules;
        let state = &mut self.state;
        decay_strikes(state, now, rules);
        let Some((reason, secs)) = ({
            let counted = verdict.candidates.iter().any(|candidate| candidate.strike);
            if counted {
                state.strikes = state.strikes.saturating_add(1).min(MAX_STRIKES);
                state.last_strike_at = Some(now);
            }
            longest(&verdict.candidates, state.strikes, rules)
        }) else {
            self.save(now);
            return None;
        };
        if verdict.success {
            state.strikes = 0;
            state.last_strike_at = None;
        }
        if verdict.candidates.contains(&MIN_INTERVAL) {
            state.last_attempt_at = Some(now);
        }
        merge_cooldown(state, reason, secs, now, rules);
        let mut event = verdict.event.clone();
        event.at = now;
        state.push_event(event);
        let pause = state.cooldown.as_ref().map(|cooldown| Pause {
            reason: cooldown.reason.clone(),
            until: cooldown.until,
            now,
        });
        self.save(now);
        pause.filter(|_| !verdict.success)
    }

    /// 記録せずに `inFlight` だけ消して放す（投稿前の失敗・取り消し）。
    fn release_at(mut self, now: u64) {
        self.state.in_flight = None;
        self.save(now);
    }
}

/// 送信前に呼ぶ。冷却中・送信中は `POST_COOLDOWN`、保存先が使えなければ `POST_GUARD_UNAVAILABLE`。
/// ガードが無効（試験用ビルドの既定）なら None。`root` は状態ルート、`user_name` は X のユーザー名、
/// `label` は状態に残す設定名（ユーザー名と一致するときは `<user>`）。
pub(crate) async fn acquire(
    root: Option<&Path>,
    user_name: &str,
    label: &str,
) -> Result<Option<Guard>> {
    let Some(rules) = active_rules() else {
        return Ok(None);
    };
    let Some(root) = root else {
        return Err(Refusal::Unavailable.into_error());
    };
    let (root, user_name, label) = (root.to_path_buf(), user_name.to_owned(), label.to_owned());
    let task = tokio::task::spawn_blocking(move || {
        acquire_at(&root, &user_name, &label, now_secs(), rules)
    });
    match task.await {
        Ok(Ok(guard)) => Ok(Some(guard)),
        Ok(Err(refusal)) => Err(refusal.into_error()),
        Err(_) => Err(Refusal::Unavailable.into_error()),
    }
}

/// 投稿の結果を記録して排他を放す。冷却が始まった・延びたエラーには、理由と再開の目安を足す。
pub(crate) async fn finish(guard: Option<Guard>, result: Result<Value>) -> Result<Value> {
    let Some(guard) = guard else {
        return result;
    };
    let verdict = judge(&result);
    let pause = tokio::task::spawn_blocking(move || guard.finish_at(&verdict, now_secs()))
        .await
        .ok()
        .flatten();
    match (result, pause) {
        (Err(mut error), Some(pause)) => {
            pause.annotate(&mut error);
            Err(error)
        }
        (result, _) => result,
    }
}

/// 記録せずに排他を放す（`ensure_proxy` の失敗など、投稿の作成まで進まなかったとき）。
pub(crate) async fn release(guard: Option<Guard>) {
    if let Some(guard) = guard {
        let _ = tokio::task::spawn_blocking(move || guard.release_at(now_secs())).await;
    }
}

// ---- 一覧・解除（U2b の CLI 向け。データだけを返す） ----

/// 一覧に出す状態の区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// 冷却中でない。
    Clear,
    /// 冷却中（`reason`・`until`・`remaining_secs`）。
    Paused,
    /// 別のプロセスが送信中（排他を保持している）。
    Sending,
    /// 前回が `finish` なしで終わった（次の投稿の取得で interrupted の冷却になる）。
    Interrupted,
    /// 読めない・形式が不正（次の投稿の取得で状態を作り直し15分の冷却になる）。
    Unreadable,
}

/// 1ユーザー名分の状態の一覧用データ。
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    /// 状態ファイルのパス。
    pub(crate) path: PathBuf,
    /// 状態に残した設定名（ユーザー名と一致するときは `<user>`）。読めないときは None。
    pub(crate) account: Option<String>,
    pub(crate) status: Status,
    pub(crate) reason: Option<String>,
    /// 冷却の期限（UTC のエポック秒）。
    pub(crate) until: Option<u64>,
    pub(crate) remaining_secs: u64,
    pub(crate) strikes: u32,
    pub(crate) last_attempt_at: Option<u64>,
    pub(crate) updated_at: Option<u64>,
    pub(crate) history: Vec<Event>,
}

fn hash_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".json")?;
    (stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit())).then(|| stem.to_owned())
}

/// 状態ファイルの一覧（パス順）。フォルダーが無ければ空。
fn state_files(root: &Path) -> Result<(Option<PathBuf>, Vec<(String, PathBuf)>)> {
    let Some(dir) = directory(root, false)? else {
        return Ok((None, Vec::new()));
    };
    let mut files: Vec<(String, PathBuf)> = fs::read_dir(&dir)
        .map_err(|_| session::storage())?
        .filter_map(|item| {
            let path = item.ok()?.path();
            Some((hash_of(&path)?, path))
        })
        .collect();
    files.sort();
    Ok((Some(dir), files))
}

fn entry_of(path: PathBuf, loaded: Result<Loaded>, locked: bool, now: u64) -> Entry {
    let empty = |status| Entry {
        path: path.clone(),
        account: None,
        status,
        reason: None,
        until: None,
        remaining_secs: 0,
        strikes: 0,
        last_attempt_at: None,
        updated_at: None,
        history: Vec::new(),
    };
    let Ok(Loaded::State(state)) = loaded else {
        return empty(Status::Unreadable);
    };
    let paused = state
        .cooldown
        .as_ref()
        .filter(|cooldown| cooldown.until > now);
    let (status, reason) = match (locked, paused, state.in_flight.is_some()) {
        (true, _, _) => (Status::Sending, None),
        (false, _, true) => (Status::Interrupted, Some("interrupted".to_owned())),
        (false, Some(cooldown), false) => (Status::Paused, Some(cooldown.reason.clone())),
        (false, None, false) => (Status::Clear, None),
    };
    Entry {
        status,
        reason: paused.map(|c| c.reason.clone()).or(reason),
        until: paused.map(|cooldown| cooldown.until),
        remaining_secs: paused.map_or(0, |cooldown| cooldown.until - now),
        account: Some(state.account.clone()),
        strikes: state.strikes,
        last_attempt_at: state.last_attempt_at,
        updated_at: Some(state.updated_at),
        history: state.history.clone(),
        ..empty(status)
    }
}

fn list_at(root: &Path, now: u64) -> Result<Vec<Entry>> {
    let (dir, files) = state_files(root)?;
    let Some(dir) = dir else {
        return Ok(Vec::new());
    };
    Ok(files
        .into_iter()
        .map(|(hash, path)| {
            let locked = is_locked(&dir, &hash);
            let loaded = load(&dir, &hash);
            entry_of(path, loaded, locked, now)
        })
        .collect())
}

/// 全ユーザー名の状態の一覧（ロックは取らず、送信中かどうかだけ調べる）。
pub(crate) fn list(root: &Path) -> Result<Vec<Entry>> {
    list_at(root, now_secs())
}

/// 解除の対象。
#[derive(Clone, Copy, Debug)]
pub(crate) enum Target<'a> {
    /// すべて（読めない状態も作り直す）。
    All,
    /// 状態に残した設定名（大文字小文字を無視）。`<user>` で残ったものは設定名では選べない。
    Account(&'a str),
    /// X のユーザー名（大文字小文字を無視）。
    UserName(&'a str),
}

/// 解除の結果。
#[derive(Clone, Debug, Default)]
pub(crate) struct ClearReport {
    /// 解除した状態（解除前に冷却中・interrupted・読めない状態だったかを `was_paused` で示す）。
    pub(crate) cleared: Vec<Cleared>,
    /// 送信中（ロックが取れない）で解除できなかった状態ファイル。
    pub(crate) busy: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct Cleared {
    pub(crate) account: Option<String>,
    pub(crate) was_paused: bool,
}

enum ClearOne {
    Cleared(Cleared),
    /// 対象外（設定名が合わない・状態が無い）。
    Skipped,
    /// 送信中でロックが取れない。
    Busy,
}

fn clear_one(dir: &Path, hash: &str, target: Target, now: u64) -> Result<ClearOne> {
    let Lock::Held(_held) = lock(dir, hash)? else {
        return Ok(ClearOne::Busy);
    };
    remove_temporaries(dir, hash);
    let (mut state, was_paused, account) = match load(dir, hash)? {
        Loaded::State(state) => {
            let paused =
                state.cooldown.as_ref().is_some_and(|c| c.until > now) || state.in_flight.is_some();
            let account = Some(state.account.clone());
            (*state, paused, account)
        }
        Loaded::Corrupt | Loaded::Newer => (State::new("<unknown>", now), true, None),
        Loaded::Fresh => return Ok(ClearOne::Skipped),
    };
    if let Target::Account(name) = target
        && !state.account.eq_ignore_ascii_case(name)
    {
        return Ok(ClearOne::Skipped);
    }
    state.cooldown = None;
    state.in_flight = None;
    state.strikes = 0;
    state.last_strike_at = None;
    state.updated_at = now;
    state.push_event(guard_event(now, "cleared"));
    write_state(dir, hash, &state)?;
    Ok(ClearOne::Cleared(Cleared {
        account,
        was_paused,
    }))
}

fn clear_at(root: &Path, target: Target, now: u64) -> Result<ClearReport> {
    let (dir, files) = state_files(root)?;
    let mut report = ClearReport::default();
    let Some(dir) = dir else {
        return Ok(report);
    };
    let wanted = match target {
        Target::UserName(name) => Some(key_hash(name.trim())),
        _ => None,
    };
    for (hash, path) in files {
        if wanted.as_ref().is_some_and(|wanted| *wanted != hash) {
            continue;
        }
        match clear_one(&dir, &hash, target, now)? {
            ClearOne::Cleared(cleared) => report.cleared.push(cleared),
            ClearOne::Busy => report.busy.push(path),
            ClearOne::Skipped => {}
        }
    }
    Ok(report)
}

/// 冷却・strikes・`inFlight` を解除する（履歴と最後の試行は残す）。送信中でロックが取れない状態は
/// `busy` に入れて解除しない。保存先が使えないときは Err。
pub(crate) fn clear(root: &Path, target: Target) -> Result<ClearReport> {
    clear_at(root, target, now_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rules = Rules::PRODUCTION;
    const NOW: u64 = 1_800_000_000;
    const MIN: u64 = 60;
    const HOUR: u64 = 3600;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "plumivox-post-guard-{:016x}",
                rand::random::<u64>()
            ));
            fs::create_dir_all(&path).unwrap();
            Temp(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn api_error(code: &str, details: Value) -> Result<Value> {
        Err(AppError::new(code, "x").with_details(details))
    }

    fn post_error(code: &str, reasons: &[&str], extra: Value) -> Result<Value> {
        let mut details = json!({"stage":"post_creation","postAttempted":true,"uploadAttempts":0,
            "rejectionReasons":reasons});
        for (key, value) in extra.as_object().into_iter().flatten() {
            details[key] = value.clone();
        }
        api_error(code, details)
    }

    fn outcome(result: &Result<Value>, strikes: u32) -> Option<(&'static str, u64)> {
        let verdict = judge(result);
        let strikes = strikes + u32::from(verdict.candidates.iter().any(|c| c.strike));
        longest(&verdict.candidates, strikes, R)
    }

    #[test]
    fn rules_table_picks_the_expected_cooldown() {
        let cases: Vec<(Result<Value>, Option<(&str, u64)>)> = vec![
            (Ok(json!({})), Some(("min_interval", 5 * MIN))),
            (
                post_error("POST_UNCERTAIN", &[], json!({})),
                Some(("previous_uncertain", 15 * MIN)),
            ),
            (
                post_error("API_REJECTED", &["post_id_missing"], json!({})),
                Some(("previous_uncertain", 15 * MIN)),
            ),
            (
                post_error("API_REJECTED", &["rate_limit"], json!({})),
                Some(("rate_limit", 15 * MIN)),
            ),
            (
                post_error(
                    "API_REJECTED",
                    &["automation_suspected"],
                    json!({"xErrorCode":226}),
                ),
                Some(("automation_suspected", 3 * HOUR)),
            ),
            (
                post_error("API_REJECTED", &["daily_limit"], json!({})),
                Some(("daily_limit", 6 * HOUR)),
            ),
            (
                post_error("API_REJECTED", &["account_locked"], json!({})),
                Some(("account_locked", 24 * HOUR)),
            ),
            (
                post_error("API_REJECTED", &["account_suspended"], json!({})),
                Some(("account_locked", 24 * HOUR)),
            ),
            (
                post_error("API_REJECTED", &["duplicate"], json!({"xRejected":true})),
                Some(("unclassified_rejection", 15 * MIN)),
            ),
            (
                post_error("API_REJECTED", &["unknown"], json!({})),
                Some(("min_interval", 5 * MIN)),
            ),
            (post_error("API_REJECTED", &["api_key"], json!({})), None),
            (post_error("API_REJECTED", &["credits"], json!({})), None),
            // 投稿作成の段階の proxy だけの拒否は、中継が X へ送った後に切れたかもしれないので最小間隔。
            (
                post_error("API_REJECTED", &["proxy"], json!({})),
                Some(("min_interval", 5 * MIN)),
            ),
            (
                post_error("API_REJECTED", &["proxy"], json!({"postAttempted":false})),
                None,
            ),
            (
                api_error(
                    "API_REJECTED",
                    json!({"stage":"image_upload","postAttempted":false,
                        "rejectionReasons":["proxy"]}),
                ),
                None,
            ),
            (
                post_error(
                    "API_HTTP_ERROR",
                    &["unauthorized"],
                    json!({"httpStatus":401}),
                ),
                None,
            ),
            (
                post_error("API_HTTP_ERROR", &["credits"], json!({"httpStatus":402})),
                None,
            ),
            (
                post_error("API_HTTP_ERROR", &["rate_limit"], json!({"httpStatus":429})),
                Some(("rate_limit", 15 * MIN)),
            ),
            // 複数の理由は最大を採る。
            (
                post_error("API_REJECTED", &["rate_limit", "daily_limit"], json!({})),
                Some(("daily_limit", 6 * HOUR)),
            ),
            // 画像アップロードの段階: 制限系だけ冷却に入れ、最小間隔は付けない。
            (
                api_error(
                    "API_REJECTED",
                    json!({"stage":"image_upload","postAttempted":false,
                        "rejectionReasons":["automation_suspected"]}),
                ),
                Some(("automation_suspected", 3 * HOUR)),
            ),
            (
                api_error(
                    "API_REJECTED",
                    json!({"stage":"image_upload","postAttempted":false,
                        "rejectionReasons":["media"],"xRejected":true}),
                ),
                None,
            ),
            (
                api_error(
                    "CANCELLED_BEFORE_POST",
                    json!({"stage":"image_upload","postAttempted":false}),
                ),
                None,
            ),
        ];
        for (index, (result, expected)) in cases.iter().enumerate() {
            assert_eq!(outcome(result, 0), *expected, "case {index}");
        }
    }

    #[test]
    fn strikes_double_the_cooldown_up_to_the_cap() {
        let result = post_error("API_REJECTED", &["automation_suspected"], json!({}));
        assert_eq!(outcome(&result, 0).unwrap().1, 3 * HOUR);
        assert_eq!(outcome(&result, 1).unwrap().1, 6 * HOUR);
        assert_eq!(outcome(&result, 2).unwrap().1, 12 * HOUR);
        assert_eq!(outcome(&result, 3).unwrap().1, 24 * HOUR);
        assert_eq!(outcome(&result, 40).unwrap().1, 24 * HOUR);
        let limit = post_error("API_REJECTED", &["rate_limit"], json!({"xRejected":true}));
        assert_eq!(outcome(&limit, 2).unwrap().1, 60 * MIN);
    }

    #[test]
    fn rate_limit_without_x_origin_is_fixed_and_not_a_strike() {
        let provider = post_error("API_HTTP_ERROR", &["rate_limit"], json!({"httpStatus":429}));
        let verdict = judge(&provider);
        assert!(verdict.candidates.iter().all(|c| !c.strike));
        assert_eq!(outcome(&provider, 5), Some(("rate_limit", 15 * MIN)));
        let from_x = post_error(
            "API_REJECTED",
            &["rate_limit"],
            json!({"xRejected":true,"xHttpStatus":429}),
        );
        assert!(judge(&from_x).candidates.iter().any(|c| c.strike));
    }

    #[test]
    fn history_keeps_only_fixed_words() {
        let result = post_error(
            "API_REJECTED",
            &["rate_limit", "Contains Spaces", "secret@user", "x"],
            json!({"xErrorCode":88,"xHttpStatus":429}),
        );
        let event = judge(&result).event;
        assert_eq!(event.reasons, vec!["rate_limit", "x"]);
        assert_eq!(event.x_error_code, Some(88));
        assert_eq!(event.x_http_status, Some(429));
        let odd = api_error("not a code!", json!({}));
        assert_eq!(judge(&odd).event.code, "UNKNOWN");
    }

    #[test]
    fn merge_keeps_the_later_deadline() {
        let mut state = State::new("a", NOW);
        merge_cooldown(&mut state, "daily_limit", 6 * HOUR, NOW, R);
        merge_cooldown(&mut state, "min_interval", 5 * MIN, NOW + MIN, R);
        let cooldown = state.cooldown.clone().unwrap();
        assert_eq!(cooldown.reason, "daily_limit");
        assert_eq!(cooldown.until, NOW + 6 * HOUR);
        // 期限が切れた後は新しい冷却に置き換える。
        merge_cooldown(&mut state, "min_interval", 5 * MIN, NOW + 7 * HOUR, R);
        assert_eq!(state.cooldown.unwrap().reason, "min_interval");
    }

    #[test]
    fn clock_going_back_resets_the_cooldown_from_now() {
        let mut state = State::new("a", NOW + 10 * HOUR);
        state.cooldown = Some(Cooldown {
            reason: "daily_limit".into(),
            from: NOW + 10 * HOUR,
            until: NOW + 16 * HOUR,
        });
        state.last_attempt_at = Some(NOW + 10 * HOUR);
        fix_clock(&mut state, NOW, R);
        let cooldown = state.cooldown.clone().unwrap();
        assert_eq!((cooldown.from, cooldown.until), (NOW, NOW + 6 * HOUR));
        assert_eq!(state.last_attempt_at, Some(NOW));
        // 未来が120秒未満なら時計の誤差として扱う。
        let mut near = State::new("a", NOW + 100);
        near.cooldown = Some(Cooldown {
            reason: "min_interval".into(),
            from: NOW + 100,
            until: NOW + 400,
        });
        fix_clock(&mut near, NOW, R);
        assert_eq!(near.cooldown.unwrap().from, NOW + 100);
        // 長すぎる冷却は上限に収める。
        let mut long = State::new("a", NOW);
        long.cooldown = Some(Cooldown {
            reason: "daily_limit".into(),
            from: NOW,
            until: NOW + 100 * HOUR,
        });
        fix_clock(&mut long, NOW, R);
        assert_eq!(long.cooldown.unwrap().until, NOW + 24 * HOUR);
    }

    #[test]
    fn strikes_decay_after_48_hours() {
        let mut state = State::new("a", NOW);
        state.strikes = 3;
        state.last_strike_at = Some(NOW);
        decay_strikes(&mut state, NOW + 47 * HOUR, R);
        assert_eq!(state.strikes, 3);
        decay_strikes(&mut state, NOW + 48 * HOUR, R);
        assert_eq!((state.strikes, state.last_strike_at), (0, None));
    }

    #[test]
    fn test_rules_scale_minutes_to_milliseconds() {
        let fast = Rules { minute_ms: 200 };
        assert_eq!(fast.secs(5), 1);
        assert_eq!(fast.secs(15), 3);
        assert_eq!(fast.secs(0), 1);
    }

    fn acquire_user(root: &Path, now: u64) -> std::result::Result<Guard, Refusal> {
        acquire_at(root, "Some_User", "main", now, R)
    }

    fn pause_of(result: std::result::Result<Guard, Refusal>) -> Pause {
        match result {
            Err(Refusal::Pause(pause)) => pause,
            Err(Refusal::Unavailable) => panic!("unexpected unavailable"),
            Ok(_) => panic!("unexpected acquire"),
        }
    }

    fn read_state(root: &Path) -> State {
        let dir = root.join(DIRECTORY);
        match load(&dir, &key_hash("some_user")).unwrap() {
            Loaded::State(state) => *state,
            _ => panic!("no state"),
        }
    }

    #[test]
    fn success_starts_the_minimum_interval_and_expires() {
        let temp = Temp::new();
        let guard = acquire_user(&temp.0, NOW).ok().unwrap();
        assert!(read_state(&temp.0).in_flight.is_some());
        let verdict = judge(&Ok(json!({})));
        assert!(guard.finish_at(&verdict, NOW + 2).is_none());
        let state = read_state(&temp.0);
        assert!(state.in_flight.is_none());
        assert_eq!(state.last_attempt_at, Some(NOW + 2));
        assert_eq!(state.history.len(), 1);
        let pause = pause_of(acquire_user(&temp.0, NOW + 3 * MIN));
        assert_eq!(pause.reason, "min_interval");
        assert_eq!(pause.retry_after_seconds(), 5 * MIN - (3 * MIN - 2));
        assert!(acquire_user(&temp.0, NOW + 2 + 5 * MIN).is_ok());
    }

    #[test]
    fn same_user_name_in_any_case_shares_one_state() {
        let temp = Temp::new();
        let _held = acquire_at(&temp.0, "SOME_user", "a", NOW, R).ok().unwrap();
        let pause = pause_of(acquire_at(&temp.0, "some_USER", "b", NOW, R));
        assert_eq!(pause.reason, "post_in_progress");
        assert!(pause.retry_after_seconds() >= 1);
    }

    #[test]
    fn two_handles_contend_for_the_lock_and_release_it() {
        let temp = Temp::new();
        let first = acquire_user(&temp.0, NOW).ok().unwrap();
        assert_eq!(
            pause_of(acquire_user(&temp.0, NOW)).reason,
            "post_in_progress"
        );
        first.release_at(NOW + 1);
        let state = read_state(&temp.0);
        assert!(state.in_flight.is_none() && state.history.is_empty());
        assert!(acquire_user(&temp.0, NOW + 2).is_ok());
    }

    #[test]
    fn leftover_in_flight_becomes_interrupted() {
        let temp = Temp::new();
        drop(acquire_user(&temp.0, NOW).ok().unwrap());
        let pause = pause_of(acquire_user(&temp.0, NOW + 10 * MIN));
        assert_eq!(pause.reason, "interrupted");
        assert_eq!(pause.until, NOW + 10 * MIN + 15 * MIN);
        let state = read_state(&temp.0);
        assert!(state.in_flight.is_none());
        assert_eq!(state.history.last().unwrap().code, "interrupted");
        assert!(acquire_user(&temp.0, NOW + 25 * MIN).is_ok());
    }

    #[test]
    fn rejections_count_strikes_and_double() {
        let temp = Temp::new();
        let rejection = post_error("API_REJECTED", &["automation_suspected"], json!({}));
        let first = acquire_user(&temp.0, NOW).ok().unwrap();
        let pause = first.finish_at(&judge(&rejection), NOW).unwrap();
        assert_eq!(
            (pause.reason.as_str(), pause.until),
            ("automation_suspected", NOW + 3 * HOUR)
        );
        let second_time = NOW + 3 * HOUR;
        let second = acquire_user(&temp.0, second_time).ok().unwrap();
        let pause = second.finish_at(&judge(&rejection), second_time).unwrap();
        assert_eq!(pause.until, second_time + 6 * HOUR);
        assert_eq!(read_state(&temp.0).strikes, 2);
        // 成功すると strikes が戻る。
        let third_time = second_time + 6 * HOUR;
        let third = acquire_user(&temp.0, third_time).ok().unwrap();
        assert!(
            third
                .finish_at(&judge(&Ok(json!({}))), third_time)
                .is_none()
        );
        assert_eq!(read_state(&temp.0).strikes, 0);
    }

    #[test]
    fn provider_only_rejection_records_nothing() {
        let temp = Temp::new();
        let guard = acquire_user(&temp.0, NOW).ok().unwrap();
        let rejection = post_error("API_REJECTED", &["credits"], json!({}));
        assert!(guard.finish_at(&judge(&rejection), NOW + 1).is_none());
        let state = read_state(&temp.0);
        assert!(state.cooldown.is_none() && state.last_attempt_at.is_none());
        assert!(acquire_user(&temp.0, NOW + 2).is_ok());
    }

    #[test]
    fn corrupt_oversized_or_unknown_state_is_reset_with_a_cooldown() {
        let bodies: Vec<Vec<u8>> = vec![
            b"not json".to_vec(),
            serde_json::to_vec(&json!({"schema":0})).unwrap(),
            vec![b' '; MAX_STATE_BYTES as usize + 1],
        ];
        for body in bodies {
            let temp = Temp::new();
            drop(acquire_user(&temp.0, NOW).ok().unwrap());
            let dir = temp.0.join(DIRECTORY);
            fs::write(state_path(&dir, &key_hash("some_user")), &body).unwrap();
            let pause = pause_of(acquire_user(&temp.0, NOW + 1));
            assert_eq!(pause.reason, "guard_state_reset");
            assert_eq!(pause.until, NOW + 1 + 15 * MIN);
            assert!(read_state(&temp.0).in_flight.is_none());
        }
    }

    #[test]
    fn newer_schema_state_is_refused_without_being_overwritten() {
        let temp = Temp::new();
        drop(acquire_user(&temp.0, NOW).ok().unwrap());
        let dir = temp.0.join(DIRECTORY);
        let path = state_path(&dir, &key_hash("some_user"));
        let body = serde_json::to_vec(&json!({"schema":SCHEMA + 1,"future":true})).unwrap();
        fs::write(&path, &body).unwrap();
        let pause = pause_of(acquire_user(&temp.0, NOW + 1));
        assert_eq!(pause.reason, "guard_state_reset");
        assert_eq!(pause.until, NOW + 1 + 15 * MIN);
        assert_eq!(fs::read(&path).unwrap(), body);
    }

    #[test]
    fn leftover_temporaries_of_this_state_only_are_removed() {
        let temp = Temp::new();
        drop(acquire_user(&temp.0, NOW).ok().unwrap());
        let dir = temp.0.join(DIRECTORY);
        let hash = key_hash("some_user");
        let mine = dir.join(format!(".post-guard-{hash}-0000000000000001.tmp"));
        let other = dir.join(".post-guard-0000-0000000000000001.tmp");
        fs::write(&mine, b"x").unwrap();
        fs::write(&other, b"x").unwrap();
        let _ = acquire_user(&temp.0, NOW + 20 * MIN);
        assert!(!mine.exists() && other.exists());
    }

    #[test]
    fn unusable_storage_fails_closed() {
        let temp = Temp::new();
        let blocked = temp.0.join(DIRECTORY);
        fs::write(&blocked, b"a file where the folder should be").unwrap();
        assert!(matches!(
            acquire_user(&temp.0, NOW),
            Err(Refusal::Unavailable)
        ));
        assert!(matches!(
            acquire_at(&temp.0, " ", "x", NOW, R),
            Err(Refusal::Unavailable)
        ));
    }

    #[test]
    fn list_and_clear_report_states() {
        let temp = Temp::new();
        let guard = acquire_user(&temp.0, NOW).ok().unwrap();
        let sending = list_at(&temp.0, NOW + 1).unwrap();
        assert_eq!(sending.len(), 1);
        assert_eq!(sending[0].status, Status::Sending);
        let report = clear_at(&temp.0, Target::All, NOW + 1).unwrap();
        assert_eq!((report.cleared.len(), report.busy.len()), (0, 1));
        guard.finish_at(&judge(&Ok(json!({}))), NOW + 2);
        let paused = list_at(&temp.0, NOW + 3).unwrap();
        assert_eq!(paused[0].status, Status::Paused);
        assert_eq!(paused[0].reason.as_deref(), Some("min_interval"));
        assert_eq!(paused[0].remaining_secs, 5 * MIN - 1);
        assert_eq!(paused[0].account.as_deref(), Some("main"));
        let none = clear_at(&temp.0, Target::Account("other"), NOW + 3).unwrap();
        assert!(none.cleared.is_empty());
        let by_name = clear_at(&temp.0, Target::Account("MAIN"), NOW + 3).unwrap();
        assert_eq!(by_name.cleared.len(), 1);
        assert!(by_name.cleared[0].was_paused);
        assert_eq!(list_at(&temp.0, NOW + 4).unwrap()[0].status, Status::Clear);
        assert!(acquire_user(&temp.0, NOW + 4).is_ok());
        let wrong = clear_at(&temp.0, Target::UserName("someone_else"), NOW + 5).unwrap();
        assert!(wrong.cleared.is_empty() && wrong.busy.is_empty());
    }
}
