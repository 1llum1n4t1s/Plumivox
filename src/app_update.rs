//! Setup（Velopack）導入版の更新・登録の処理。
//!
//! # 目的
//! 利用者がコマンドを打たずに Setup で導入し、常に最新版で使えるようにする。共通仕様は
//! `D:\Dev\APP-DISTRIBUTION.md`（多プロセス型）、参考実装は Remnifold の `native/src/app_update.rs`。
//!
//! # 多プロセス型としての設計
//! Plumivox は、Claude Code・Codex のセッションごとに stdio の MCP が常駐し、管理プロキシを使うと
//! 共有中継 daemon（`--proxy-daemon`。MCP の `current_exe()` から起動するので導入先配下の実行ファイル）も動く。
//! Velopack の適用（`Update.exe apply`）は導入先の自分以外の全プロセスを強制終了するため、
//! 日中に適用すると別セッションの MCP や中継を巻き込む。そこで次のように分ける。
//! - 起動時の自動適用は全サブコマンドで切る（[`startup`]）。
//! - 確認と取得は、長く動く stdio MCP の別スレッドで行う（[`spawn_background`]）。取得だけで適用しない。
//! - 適用はログオン時の `--update-apply`（[`update_apply`]）だけ。導入先に他のプロセス（daemon を含む）が
//!   無いことを確かめ、署名者を検証してから、再起動なしで適用する。
//! - Claude Code・Codex への MCP 登録とログオン時の適用の登録は、全 OS で [`registration`] が状態ファイル
//!   （`setup-registration.json`）とあわせて行う。Windows では Velopack のフック（install / updated /
//!   uninstall）からも呼び、いずれも必ず終了コード 0 で終える（フックの失敗でインストール・更新・
//!   アンインストールを止めない）。フックは CLI 1回を3秒で打ち切るため、登録が終わらなかった CLI は状態に
//!   「未完了」と残り、ログオン時の `--update-apply`・引数なしの起動・stdio 起動で（30秒の上限で）やり直す。
//!   macOS ではフックが呼ばれないため、これらの起動だけで登録し、解除は `--uninstall-setup --apply` で行う。
//!
//! # 保存先
//! 設定（`auto_update`）と確認・失敗時刻・診断ログ・排他用ロック・登録状態は、データフォルダー
//! （Windows `%LOCALAPPDATA%\Kagayoi\Plumivox`、macOS `~/Library/Application Support/Kagayoi/Plumivox`）の
//! 子 `app-update\` に置く。Velopack の導入先（`current\`）は更新で入れ替わるため使わない。
//! データフォルダー自体はセッション保存と同じ本人限定の手順（[`crate::session::private_data_root`]）で作る。
//!
//! # フィード
//! 更新元の生成は [`update_source`] の1か所に閉じる。
use crate::connect;
use crate::locale::{tr, tr_with};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use velopack::{
    ApplyWaitMode, UpdateCheck, UpdateManager, VelopackApp,
    locator::{self, LocationContext, VelopackLocator},
    sources::UpdateSource,
};

mod logon;
pub(crate) mod process;
mod processes;
// 旧プラグインが有効なままかの検知（読み取りだけ）。
mod legacy;
// 登録の要否の判定と状態の読み書き（全 OS 共通）。
mod registration;
mod trust;

/// このモジュールの失敗。利用者向けの短い説明だけを持つ（秘密値・提供元の生エラーを含めない）。
pub type Result<T> = std::result::Result<T, String>;

/// 本番の packId。`Plumivox` は既存の `%LOCALAPPDATA%\Plumivox`（旧 JavaScript 版などの作業フォルダー）と
/// 衝突するため使わない。Velopack の Windows の既定の導入先は `%LOCALAPPDATA%\<packId>`。
pub const PACK_ID: &str = "PlumivoxApp";
/// 本番の更新元。`releases.<channel>.json` と `*-full.nupkg` をここから配信する（チャネルは `win`、将来 `osx`）。
pub const FEED_URL: &str = "https://plumivox-updates.kagayoi.com/velopack";
/// データフォルダーの中の、この機能の状態フォルダー名。
pub const STATE_DIR_NAME: &str = "app-update";
/// 設定・確認時刻を保存するファイル名（状態フォルダー直下）。
pub const SETTINGS_FILE: &str = "app-update.json";
/// プロセス間の排他に使うファイル名（状態フォルダー直下）。
pub const LOCK_FILE: &str = "app-update.lock";
/// 診断用の小さなログ（フックと MCP の背景スレッドは標準出力を使えないため、結果の確認はこのファイルで行う）。
pub const LOG_FILE: &str = "app-update.log";
/// 前回の確認（成功・失敗のどちらも）からこの時間が過ぎるまでは、ネットワークへ出ない。
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// 常駐中に「確認の時期か」を見直す間隔。壁時計で判定し直すので、スリープ明けや設定変更にも追随する。
pub const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// 自動更新の設定と、前回の確認・失敗時刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoUpdateSettings {
    /// 確認・取得・適用を自動で行うか。未設定は `true`（既定ON）。キー名は `auto_update`。
    pub auto_update: bool,
    /// 前回の確認に成功した時刻（UNIX秒）。
    pub last_checked_at: Option<u64>,
    /// 前回の確認に失敗した時刻（UNIX秒）。失敗しても24時間は再試行しない（起動の多い MCP が毎回通信しないため）。
    pub last_failed_at: Option<u64>,
}

impl Default for AutoUpdateSettings {
    fn default() -> Self {
        Self {
            auto_update: true,
            last_checked_at: None,
            last_failed_at: None,
        }
    }
}

/// JSON から設定を作る。壊れた JSON・型の違う項目は項目ごとに既定値へ戻し、決して失敗しない
/// （更新機能の不調で MCP やフックを止めないため）。
pub fn parse_settings(bytes: &[u8]) -> AutoUpdateSettings {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return AutoUpdateSettings::default();
    };
    AutoUpdateSettings {
        auto_update: value
            .get("auto_update")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        last_checked_at: value.get("last_checked_at").and_then(Value::as_u64),
        last_failed_at: value.get("last_failed_at").and_then(Value::as_u64),
    }
}

/// 状態フォルダーから設定を読む。無い・読めない・通常ファイルでない場合は既定値。
pub fn load_settings(state_dir: &Path) -> AutoUpdateSettings {
    let path = state_dir.join(SETTINGS_FILE);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => fs::read(&path)
            .map(|bytes| parse_settings(&bytes))
            .unwrap_or_default(),
        _ => AutoUpdateSettings::default(),
    }
}

/// 同じフォルダーの一時ファイルへ書いてから置き換える（読み手が書きかけの内容を見ないため）。
/// 書き込み側は排他ロックの保持者だけなので、一時ファイル名は乱数で衝突を避けるだけで足りる。
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| "The state file has no parent folder.".to_string())?;
    let temporary = parent.join(format!(".plumivox-{:016x}.tmp", rand::random::<u64>()));
    let written = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map_err(|_| "The state file could not be written.".to_string())
}

/// 時刻の1項目だけを書き換える。ファイルにある他のキー（利用者が足した項目を含む）は保つ。
/// 書き込み側は排他ロックの保持者だけなので、読み取り→変更→原子的置換で足りる。
fn record_time(state_dir: &Path, key: &str, now: u64) -> Result<()> {
    let path = state_dir.join(SETTINGS_FILE);
    let mut value = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({})),
        Ok(_) => return Err("The update settings path is not a regular file.".into()),
        Err(_) => json!({}),
    };
    value[key] = json!(now);
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| "JSON".to_string())?;
    bytes.push(b'\n');
    atomic_write(&path, &bytes)
}

/// 現在時刻（UNIX秒）。時計が1970年より前の異常値でも 0 として扱う。
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// 前回の時刻から24時間以上たったか。未記録と、未来の時刻（時計の巻き戻し・手編集）は「時期」とみなす
/// （永久に確認しなくなるのを避けるため）。
pub fn is_due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(last) if last > now => true,
        Some(last) => now - last >= CHECK_INTERVAL.as_secs(),
    }
}

/// 自動で確認・取得をするべきか。ON で、成功時刻と失敗時刻の両方が24時間より前のときだけ真。
pub fn should_check(settings: &AutoUpdateSettings, now: u64) -> bool {
    settings.auto_update
        && is_due(settings.last_checked_at, now)
        && is_due(settings.last_failed_at, now)
}

/// 更新元を作る。公開先を変えるときはこの関数だけを差し替える。
///
/// `update-feed-test` 機能付きのビルドに限り、環境変数 `PLUMIVOX_UPDATE_FEED_DIR` の
/// ローカルフォルダーを更新元にできる（隔離した E2E 検証用。本番ビルドにこの分岐は残らない）。
fn update_source() -> Box<dyn UpdateSource> {
    #[cfg(feature = "update-feed-test")]
    if let Some(directory) = std::env::var_os("PLUMIVOX_UPDATE_FEED_DIR").filter(|v| !v.is_empty())
    {
        return Box::new(velopack::sources::FileSource::new(PathBuf::from(directory)));
    }
    Box::new(velopack::sources::HttpSource::new(FEED_URL))
}

/// Velopack 管理下（Setup で導入された `current\` 配下）なら、その配置情報。
/// 開発ビルドやプラグイン（`dist/`）経路の起動では None（その場合、このモジュールは何もしない）。
pub(crate) fn locate() -> Option<VelopackLocator> {
    locator::auto_locate_app_manifest(LocationContext::FromCurrentExe).ok()
}

/// Setup（Velopack）で導入されたビルドか。直接起動の案内（[`welcome`]）を出してよいかの判定に使う。
pub fn is_managed() -> bool {
    locate().is_some()
}

fn manager() -> Result<UpdateManager> {
    UpdateManager::new_boxed(update_source(), None, None)
        .map_err(|_| "The Velopack update manager is unavailable.".to_string())
}

/// 状態フォルダー（データフォルダーの子 `app-update`）。作れなければ None（このとき状態を残せないので、
/// 登録・確認・適用はいずれも何もしない側へ倒す）。プロセス内で一度だけ決める（環境変数は起動時に固定）。
pub(crate) fn state_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = crate::session::private_data_root()?;
        let dir = root.join(STATE_DIR_NAME);
        create_private_dir(&dir).ok()?;
        Some(dir)
    })
    .clone()
}

/// 状態フォルダーを作る。Windows は親（本人限定 DACL）を継承し、Unix は 0700 で作る
/// （macOS のセットアップは他者が読める階層を `ACL_UNSAFE` として拒否するため）。
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => return Ok(()),
        Ok(_) => {
            return Err(std::io::Error::other(
                "the update state path is not a directory",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(dir)
    }
}

/// 診断ログへ1行足す。失敗は無視し（ログのために本処理を失敗させない）、肥大化したら作り直す。
/// 秘密値は扱わない（パス・版・状態の短い文言だけを書く）。
pub(crate) fn log_line(message: &str) {
    let Some(dir) = state_dir() else {
        return;
    };
    let path = dir.join(LOG_FILE);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > 64 * 1024) {
        let _ = fs::remove_file(&path);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {message}", now_unix());
    }
}

// ───────────────────────── 起動時の Velopack 処理 ─────────────────────────

/// 起動直後の結果。
pub struct Startup {
    /// Setup 直後の初回起動か（Velopack の firstrun）。
    pub first_run: bool,
}

/// フックの処理を実行し、成否にかかわらず戻る（panic も飲み込む）。Velopack は処理の後に必ず
/// 終了コード 0 でプロセスを終えるため、ここで失敗を外へ出さない。
fn guarded(label: &str, work: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_err() {
        log_line(&format!("{label}: unexpected failure"));
    }
}

/// 登録が使う導入情報（登録する実行ファイル・packId・版）。
pub(crate) struct SetupContext {
    /// Claude Code・Codex とログオン時の適用に登録する実行ファイル。Windows は `current\plumivox.exe`、
    /// macOS は `.app/Contents/MacOS/plumivox`。
    pub(crate) executable: PathBuf,
    pub(crate) app_id: String,
    pub(crate) version: String,
}

impl SetupContext {
    fn detect() -> Option<Self> {
        let locator = locate()?;
        Some(Self {
            executable: locator.get_main_exe_path(),
            app_id: locator.get_manifest_id(),
            version: locator.get_manifest_version_full_string(),
        })
    }
}

/// インストール・更新の直後の登録（Windows のフック）。`fresh`（インストール）のときは前回の状態を忘れ、
/// Claude Code・Codex とログオン時の適用を全て確かめ直す。更新では [`registration::plan`] のとおり、
/// ログオン時の適用だけを張り直し、MCP の登録はパスが同じなら CLI を呼ばない（使用中のセッションの
/// 登録を外して付け直さない）。CLI は1回3秒で打ち切り（フックの時間制限のため）、終わらなかった CLI は状態に
/// 未完了と残して、ログオン時の `--update-apply` などでやり直す。
#[cfg_attr(not(windows), allow(dead_code))]
fn register_everything(label: &str, fresh: bool) {
    // 管理外のビルドでは何もしない（診断ログのためにデータフォルダーを作らない）。
    let Some(context) = SetupContext::detect() else {
        return;
    };
    let Some(dir) = state_dir() else {
        return;
    };
    if fresh && let Err(error) = registration::forget(&dir) {
        log_line(&format!(
            "{label}: previous registration state not reset: {error}"
        ));
    }
    registration::ensure(&dir, &context, label);
}

/// アンインストール前の解除。Claude Code・Codex の登録とログオン時の適用を外す。
#[cfg_attr(not(windows), allow(dead_code))]
fn unregister_everything() {
    let Some(context) = SetupContext::detect() else {
        return;
    };
    for line in unregister_context(&context) {
        log_line(&format!("uninstall: {line}"));
    }
}

/// 解除の本体。結果を人が読める行で返す（フックは診断ログへ、`--uninstall-setup` は標準出力へ出す）。
/// データフォルダー（設定・セッション・この機能の設定）は消さない。
fn unregister_context(context: &SetupContext) -> Vec<String> {
    let mut lines = Vec::new();
    // ログオン時の適用の解除（一瞬で終わる）を先に行い、時間がかかりうる CLI の解除を後ろに置く。
    lines.push(
        match logon::unregister(&context.app_id, &context.executable) {
            Ok(removed) => format!("logon apply removed={removed}"),
            Err(error) => format!("logon apply failed: {error}"),
        },
    );
    lines.extend(connect::unregister_for_setup(&context.executable));
    // 次に導入したときに登録し直すよう、登録の状態を忘れる。
    if let Some(dir) = state_dir()
        && let Err(error) = registration::forget(&dir)
    {
        lines.push(format!("registration state not removed: {error}"));
    }
    lines
}

/// Setup 版で、登録が今の自分を指していなければ（または前回終わらなかった CLI があれば）登録する
/// （[`registration`]）。引数なしの起動、stdio MCP の背景スレッド、ログオン時の `--update-apply` から呼ぶ。
/// Windows ではフックの3秒の打ち切りで終わらなかった登録をここで完了させる（フック以外なので CLI の上限を
/// 長めにする）。普段は状態ファイルを読むだけで終わる。Velopack 管理外のビルドでは何もしない。
/// panic しても呼び出し元（MCP・案内・適用）を止めない。
pub fn ensure_registered() {
    if let Some(context) = SetupContext::detect()
        && let Some(dir) = state_dir()
    {
        connect::set_cli_timeout(connect::RETRY_CLI_TIMEOUT);
        guarded("register", || {
            registration::ensure(&dir, &context, "register")
        });
    }
}

/// Setup 版の解除（`plumivox --uninstall-setup [--apply]`）。macOS には Velopack のアンインストーラーが
/// 無いため、利用者が .app をゴミ箱へ入れる前にこれを実行する。Windows でも、アンインストーラーと同じ解除を
/// 手動で行える。`--apply` が無ければ、何を外すかを示すだけで何も変えない。
pub fn uninstall_setup(args: &[String]) -> Result<Value> {
    let mut apply = false;
    for arg in args {
        match arg.as_str() {
            "--apply" => apply = true,
            "--help" | "-h" => {
                // 端末の利用者が読む説明は利用者の言語で出す（キー名と status は機械向けに固定）。
                return Ok(json!({
                    "usage": "plumivox --uninstall-setup [--apply]",
                    "description": tr("uninstall.help")
                }));
            }
            _ => {
                return Err(tr("uninstall.invalid_option").into());
            }
        }
    }
    let Some(context) = SetupContext::detect() else {
        return Ok(json!({
            "status": "unmanaged",
            "reason": tr("uninstall.unmanaged")
        }));
    };
    let mut result = json!({
        "executable": context.executable,
        "appId": context.app_id,
        "logonEntry": logon::describe(&context.app_id),
        "applied": apply,
    });
    if !apply {
        result["status"] = json!("planned");
        result["next"] = json!(tr("uninstall.next_planned"));
        return Ok(result);
    }
    let lines = unregister_context(&context);
    log_line(&format!("uninstall-setup: {}", lines.join("; ")));
    result["status"] = json!("removed");
    result["results"] = json!(lines);
    result["next"] = json!(tr("uninstall.next_removed"));
    Ok(result)
}

/// `main()` の最初（引数の解析・tokio ランタイムの作成より前）に、全サブコマンド共通で呼ぶ。
///
/// - 起動時の自動適用を切る（多プロセス型のため。適用は `--update-apply` だけが行う）。
/// - Velopack のフック引数（`--veloapp-*`）は、ここで処理して終了コード 0 で終える。
/// - 管理外のビルドでは、Velopack 側が何もせずに戻る。
/// - Velopack は環境変数（初回起動の印）を消すため、スレッドを作る前の同期の main から呼ぶ。
pub fn startup() -> Startup {
    let first_run = AtomicBool::new(false);
    let mut app = VelopackApp::build().set_auto_apply_on_startup(false);
    // 高速フックは Windows だけが呼ぶ（crate 側でも登録関数が Windows 限定）。macOS の登録は
    // 引数なし・stdio の起動時に ensure_registered / spawn_background が行う。
    #[cfg(windows)]
    {
        app = app
            .on_after_install_fast_callback(|_| {
                guarded("install", || register_everything("install", true))
            })
            .on_after_update_fast_callback(|_| {
                guarded("updated", || register_everything("updated", false))
            })
            .on_before_uninstall_fast_callback(|_| guarded("uninstall", unregister_everything));
    }
    app = app.on_first_run(|_| first_run.store(true, Ordering::SeqCst));
    app.run();
    Startup {
        first_run: first_run.load(Ordering::SeqCst),
    }
}

// ───────────────────────── 背景の確認・取得 ─────────────────────────

/// 状態フォルダーの指定のロックファイルを try-lock する。保持はファイルを閉じるまで（プロセスの終了を含む）。
/// 他のプロセスが保持していれば None（その場合は何もしない）。
pub(crate) fn try_lock_named(state_dir: &Path, name: &str) -> Option<fs::File> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state_dir.join(name))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// 確認・取得を1回行う。新版があれば取得だけして、適用はしない。
fn check_and_download() -> Result<String> {
    let manager = manager()?;
    let check = manager
        .check_for_updates()
        .map_err(|_| "The update feed could not be checked.".to_string())?;
    Ok(match check {
        UpdateCheck::UpdateAvailable(info) => {
            // 取得済みなら即座に戻る。Velopack が .partial から原子的に確定し、フィードのハッシュを照合する。
            manager
                .download_updates(&info, None)
                .map_err(|_| "The update package could not be downloaded.".to_string())?;
            format!(
                "downloaded {}; it is applied at the next logon",
                info.TargetFullRelease.Version
            )
        }
        UpdateCheck::NoUpdateAvailable => "up to date".to_owned(),
        UpdateCheck::RemoteIsEmpty => "the update feed is empty".to_owned(),
    })
}

/// 時期なら、ロックを取って確認・取得する。結果は設定ファイルに記録する。
fn background_cycle(state_dir: &Path) {
    if !should_check(&load_settings(state_dir), now_unix()) {
        return;
    }
    // 他のセッションが確認・取得中なら何もしない（二重取得しない）。
    let Some(_lock) = try_lock_named(state_dir, LOCK_FILE) else {
        return;
    };
    // ロックを取る間に、別のプロセスが確認を終えていることがある。読み直して判定し直す。
    if !should_check(&load_settings(state_dir), now_unix()) {
        return;
    }
    match check_and_download() {
        Ok(message) => {
            let _ = record_time(state_dir, "last_checked_at", now_unix());
            log_line(&format!("auto-update: {message}"));
        }
        Err(error) => {
            // 失敗した時刻も記録し、24時間は再試行しない。現行版で動き続ける。
            let _ = record_time(state_dir, "last_failed_at", now_unix());
            log_line(&format!("auto-update failed: {error}"));
        }
    }
}

/// stdio MCP の起動後に、確認・取得の専用スレッドを始める。Velopack 管理外では何もしない。
///
/// 標準出力は MCP のプロトコル専用のため、このスレッドは何も出力しない（結果は診断ログだけ）。
/// プロセスの終了で一緒に止まるので join しない。呼び出しは stdio の起動ごとに1回だけにする。
pub fn spawn_background() {
    if locate().is_none() {
        return;
    }
    let Some(state_dir) = state_dir() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("plumivox-auto-update".into())
        .spawn(move || {
            // まず登録が今の自分を指しているか（Windows ではフックで終わらなかった CLI が無いか）を確かめる
            // （普段は状態ファイルを読むだけで終わる）。MCP の応答を遅らせないよう、このスレッドで行う。
            ensure_registered();
            loop {
                // panic しても MCP 本体へは波及させない（このスレッドだけが止まる）。
                if std::panic::catch_unwind(|| background_cycle(&state_dir)).is_err() {
                    log_line("auto-update: unexpected failure");
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        });
    if spawned.is_err() {
        log_line("auto-update thread could not start");
    }
}

// ───────────────────────── ログオン時の適用 ─────────────────────────

/// `--update-apply` の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Velopack 管理外（開発ビルド・プラグイン経路）。
    Unmanaged,
    /// 設定で自動更新が切られている。
    Disabled,
    /// 保留中の更新が無い。
    NothingPending,
    /// 導入先から他のプロセス（MCP・中継 daemon など）が動いているため、適用しなかった（次のログオンに回す）。
    Busy(Vec<u32>),
    /// 署名者などの検証に通らず、適用しなかった。
    Untrusted(String),
    /// 適用を Update.exe に依頼した（このプロセスの終了を待って入れ替わる。再起動しない）。
    Applied(String),
}

impl ApplyOutcome {
    /// 標準出力へ1行で出す JSON。
    pub fn to_json(&self) -> Value {
        match self {
            Self::Unmanaged => json!({"status":"unmanaged"}),
            Self::Disabled => json!({"status":"disabled"}),
            Self::NothingPending => json!({"status":"nothingPending"}),
            Self::Busy(pids) => json!({"status":"busy","processes":pids.len()}),
            Self::Untrusted(reason) => json!({"status":"untrusted","reason":reason}),
            Self::Applied(version) => json!({"status":"applied","version":version}),
        }
    }
}

/// 保留中の更新を、安全な場合だけ適用する。強制終了も再起動もしない。
pub fn update_apply() -> Result<ApplyOutcome> {
    let Some(locator) = locate() else {
        return Ok(ApplyOutcome::Unmanaged);
    };
    // ログオンのたびに、登録が残っているか（フックで終わらなかった CLI が無いか）を確かめて直す。
    // MCP の登録が消えていると Claude Code・Codex が MCP を起動しないため、ここが確実な再試行の機会になる。
    // 自動更新の設定とは関係なく行う。
    ensure_registered();
    let state_dir =
        state_dir().ok_or_else(|| "The update state folder is unavailable.".to_string())?;
    if !load_settings(&state_dir).auto_update {
        return Ok(ApplyOutcome::Disabled);
    }
    let manager = manager()?;
    let Some(pending) = manager.get_update_pending_restart() else {
        return Ok(ApplyOutcome::NothingPending);
    };
    // 導入先から動いている自分以外のプロセスが1つでもあれば、適用しない。適用は全て強制終了してしまう。
    // 中継 daemon も MCP の current_exe（導入先配下）から起動するので、ここで数えられる。
    let root = locator.get_root_dir();
    let others = processes::others_in(&root, std::process::id())
        .map_err(|_| "The running processes could not be listed.".to_string())?;
    if !others.is_empty() {
        log_line(&format!(
            "update-apply: skipped, {} other process(es) are running from the install folder",
            others.len()
        ));
        return Ok(ApplyOutcome::Busy(others));
    }
    let package = locator.get_packages_dir().join(&pending.FileName);
    if !package.is_file() {
        return Err("The pending update package is missing.".into());
    }
    if let Err(error) = trust::verify_package(&package) {
        log_line(&format!("update-apply: refused: {error}"));
        return Ok(ApplyOutcome::Untrusted(error));
    }
    // このプロセスの終了を待って適用し、再起動しない。Update.exe は別プロセスとして続行する。
    manager
        .unsafe_apply_updates(
            &pending,
            true,
            ApplyWaitMode::WaitPid(std::process::id()),
            false,
            Vec::<String>::new(),
        )
        .map_err(|_| "The update could not be handed to the Velopack updater.".to_string())?;
    log_line(&format!("update-apply: applying {}", pending.Version));
    Ok(ApplyOutcome::Applied(pending.Version.clone()))
}

// ───────────────────────── 案内（初回起動・引数なしの対話起動） ─────────────────────────

/// 利用者が直接起動したとき（Setup 直後の初回起動、スタートメニュー・端末からの引数なし起動）の案内。
/// MCP として stdin を待たず、導入済み・登録済みであることと次の手順を示す。
/// 秘密値を扱う `--setup` は自動では始めず、実行するコマンドを示すだけにする（端末からの明示の実行に任せる）。
pub fn welcome(first_run: bool) {
    use std::io::{BufRead, IsTerminal};
    let executable = SetupContext::detect()
        .map(|context| context.executable)
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("plumivox"));
    // 案内は人が読むので、利用者の言語の辞書から引く（MCP の経路ではないので言語判定を走らせてよい）。
    println!("{}", tr("welcome.installed"));
    // 旧プラグイン（Codex・Claude Code）が有効なままなら、二重起動を避ける案内を出す（設定は書き換えない）。
    let legacy_tools = legacy::tool_labels(&legacy::detect());
    if !legacy_tools.is_empty() {
        println!(
            "{}",
            tr_with(
                "welcome.legacy_plugin",
                &[("tools", &legacy_tools.join(", "))]
            )
        );
    }
    let settings_hint = state_dir()
        .map(|dir| dir.join(SETTINGS_FILE).display().to_string())
        .unwrap_or_else(|| SETTINGS_FILE.to_string());
    println!(
        "{}",
        tr_with("welcome.updates", &[("settings", &settings_hint)])
    );
    // 設定ファイルの選択順は MCP と同じ（resolve_secrets_path）。存在の確認だけで、中身は読まない。
    let configured = crate::config::resolve_secrets_path(None).is_file();
    if configured {
        println!("{}", tr("welcome.configured"));
    } else {
        println!(
            "{}",
            tr_with(
                "welcome.setup_next",
                &[("executable", &executable.display().to_string())]
            )
        );
    }
    if (first_run || console_closes_on_exit()) && std::io::stdin().is_terminal() {
        // 初回起動やスタートメニューからの起動のコンソールは、閉じると結果が読めなくなる。確認してから閉じる。
        print!("{}", tr("welcome.press_enter"));
        let _ = std::io::stdout().flush();
        let mut ignored = String::new();
        let _ = std::io::stdin().lock().read_line(&mut ignored);
    }
}

/// このプロセスの終了とともにコンソールが閉じるか（コンソールに付いているのが自分だけか）。
/// スタートメニューから起動すると専用のコンソールが作られ、案内を出した直後に閉じてしまうため、
/// そのときも Enter を待つ。ターミナルから起動した場合はシェルも同じコンソールに付いているので待たない。
#[cfg(windows)]
fn console_closes_on_exit() -> bool {
    let mut ids = [0u32; 4];
    // SAFETY: ids は ids.len() 個の u32 を書き込める領域。戻り値は付いているプロセスの数（失敗は 0）。
    let count = unsafe {
        windows_sys::Win32::System::Console::GetConsoleProcessList(
            ids.as_mut_ptr(),
            ids.len() as u32,
        )
    };
    count == 1
}

#[cfg(not(windows))]
fn console_closes_on_exit() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn scratch(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "plumivox-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn defaults_are_on_and_unchecked() {
        let settings = AutoUpdateSettings::default();
        assert!(settings.auto_update);
        assert_eq!(settings.last_checked_at, None);
        assert_eq!(settings.last_failed_at, None);
        let dir = scratch("settings-default");
        assert_eq!(load_settings(&dir), settings);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_or_mistyped_json_falls_back_per_field() {
        let default = AutoUpdateSettings::default();
        for broken in [&b""[..], b"{", b"not json", b"[1,2]", b"null", b"\xff\xfe"] {
            assert_eq!(parse_settings(broken), default);
        }
        let parsed =
            parse_settings(br#"{"auto_update":"no","last_checked_at":123,"last_failed_at":"x"}"#);
        assert!(parsed.auto_update);
        assert_eq!(parsed.last_checked_at, Some(123));
        assert_eq!(parsed.last_failed_at, None);
        assert!(!parse_settings(br#"{"auto_update":false}"#).auto_update);
    }

    #[test]
    fn recording_keeps_other_keys_and_each_time_separately() {
        let dir = scratch("settings-record");
        fs::write(
            dir.join(SETTINGS_FILE),
            br#"{"auto_update":false,"note":"mine"}"#,
        )
        .unwrap();
        record_time(&dir, "last_checked_at", 1_000).unwrap();
        record_time(&dir, "last_failed_at", 2_000).unwrap();
        let settings = load_settings(&dir);
        assert!(!settings.auto_update);
        assert_eq!(settings.last_checked_at, Some(1_000));
        assert_eq!(settings.last_failed_at, Some(2_000));
        let value: Value =
            serde_json::from_slice(&fs::read(dir.join(SETTINGS_FILE)).unwrap()).unwrap();
        assert_eq!(value["note"], "mine");
        // 一時ファイルを残さない。
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        // 壊れたファイルは作り直す。
        fs::write(dir.join(SETTINGS_FILE), b"{ broken").unwrap();
        record_time(&dir, "last_checked_at", 5).unwrap();
        assert_eq!(load_settings(&dir).last_checked_at, Some(5));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn throttle_counts_both_success_and_failure_times() {
        let day = CHECK_INTERVAL.as_secs();
        assert!(is_due(None, 10));
        assert!(!is_due(Some(1_000), 1_000 + day - 1));
        assert!(is_due(Some(1_000), 1_000 + day));
        // 時計の巻き戻しで時刻が未来になっても、永久に確認しない状態にならない。
        assert!(is_due(Some(5_000), 1_000));
        let now = 1_000_000;
        let due = AutoUpdateSettings {
            auto_update: true,
            last_checked_at: Some(now - day),
            last_failed_at: Some(now - day),
        };
        assert!(should_check(&due, now));
        // 失敗が新しければ、成功が古くても確認しない。逆も同じ。
        assert!(!should_check(
            &AutoUpdateSettings {
                last_failed_at: Some(now - 60),
                ..due
            },
            now
        ));
        assert!(!should_check(
            &AutoUpdateSettings {
                last_checked_at: Some(now - 60),
                ..due
            },
            now
        ));
        assert!(!should_check(
            &AutoUpdateSettings {
                auto_update: false,
                ..due
            },
            now
        ));
    }

    #[test]
    fn lock_is_exclusive_between_holders() {
        let dir = scratch("lock");
        let first = try_lock_named(&dir, LOCK_FILE).expect("first holder");
        assert!(
            try_lock_named(&dir, LOCK_FILE).is_none(),
            "second holder must not run"
        );
        drop(first);
        assert!(try_lock_named(&dir, LOCK_FILE).is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unmanaged_builds_do_nothing() {
        // テストの実行ファイルは Velopack の導入先ではない。状態フォルダーにも触れずに戻る。
        assert!(locate().is_none());
        assert!(!is_managed());
        assert_eq!(update_apply().unwrap(), ApplyOutcome::Unmanaged);
        assert_eq!(
            uninstall_setup(&["--apply".into()]).unwrap()["status"],
            "unmanaged"
        );
        assert!(uninstall_setup(&["--force".into()]).is_err());
        spawn_background();
    }

    #[test]
    fn outcomes_report_stable_status_codes() {
        assert_eq!(
            ApplyOutcome::NothingPending.to_json()["status"],
            "nothingPending"
        );
        assert_eq!(ApplyOutcome::Busy(vec![1, 2]).to_json()["processes"], 2);
        assert_eq!(
            ApplyOutcome::Applied("9.0.1".into()).to_json()["version"],
            "9.0.1"
        );
        assert_eq!(
            ApplyOutcome::Untrusted("x".into()).to_json()["status"],
            "untrusted"
        );
    }

    #[cfg(unix)]
    #[test]
    fn state_folder_is_created_private() {
        use std::os::unix::fs::MetadataExt;
        let dir = scratch("state-private").join(STATE_DIR_NAME);
        create_private_dir(&dir).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        // 既存のフォルダーはそのまま使う。
        create_private_dir(&dir).unwrap();
    }
}
