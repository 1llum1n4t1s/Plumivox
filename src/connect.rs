//! Setup（Velopack）導入版の Plumivox を、Claude Code・Codex の MCP として登録・解除する。
//!
//! - 登録は公式 CLI だけで行う（`claude mcp add --scope user plumivox -- <exe> --stdio`、
//!   `codex mcp add plumivox -- <exe> --stdio`）。設定ファイルを直接書き換えない。
//! - Claude Code のフックは使わない（Plumivox は MCP だけを提供する）。
//! - 登録前に `mcp get plumivox` で今の登録を確かめる。既にこの実行ファイルを指していれば触らない
//!   （外して付け直す間に打ち切られると登録が消えたままになり、使用中のセッションにも影響するため）。
//!   別の場所（旧プラグインの `dist` や別の導入先）を指す同名の登録だけを remove → add で置き換える。
//!   登録が無ければ add だけを行う。
//! - 解除は、この実行ファイルを指す登録だけを外す（別の場所を指す登録は保つ）。
//! - CLI は PATH 上の実行ファイルだけを探す（Windows は `.exe` だけ。スクリプトのラッパーは使わない）。
//! - どの呼び出しも時間制限付き（[`crate::app_update::process::run`]）。Velopack のフックからは1回3秒、
//!   フック以外（`--update-apply`・引数なしの起動・stdio の背景スレッド）では30秒。
//!   失敗は例外にせず、結果を文字列で返す（フックの失敗でインストールや更新を止めないため）。
use crate::app_update::process::{self, RunError};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// 登録する MCP サーバーの名前。
pub const SERVER_NAME: &str = "plumivox";
/// Setup 版が登録する CLI の名前（`claude` と `codex`）。
pub const SETUP_TOOLS: [&str; 2] = ["claude", "codex"];
/// Velopack のフックから呼ぶ CLI 1回あたりの上限（既定）。更新後のフックは15秒で打ち切られるため、
/// get・remove・add の3回でも収まりやすいよう1回を3秒に制限する。
pub const HOOK_CLI_TIMEOUT: Duration = Duration::from_secs(3);
/// フック以外で登録をやり直すときの上限。Velopack の打ち切りが無いので長めにし、フックで3秒に
/// 収まらなかった登録をここで完了させる。
pub const RETRY_CLI_TIMEOUT: Duration = Duration::from_secs(30);
/// CLI の標準出力・標準エラーを読む上限。
const OUTPUT_LIMIT: u64 = 64 * 1024;

/// このプロセスで CLI 呼び出しに使う上限（ミリ秒）。プロセスごとに役割が1つ（フック・ログオン時の適用・
/// MCP など）なので、フック以外の入口で一度だけ [`set_cli_timeout`] で引き上げる。
static CLI_TIMEOUT_MS: AtomicU64 = AtomicU64::new(HOOK_CLI_TIMEOUT.as_millis() as u64);

/// CLI 呼び出しの上限を変える（フック以外の入口から、登録をやり直す前に呼ぶ）。
pub fn set_cli_timeout(timeout: Duration) {
    CLI_TIMEOUT_MS.store(timeout.as_millis() as u64, Ordering::SeqCst);
}

fn cli_timeout() -> Duration {
    Duration::from_millis(CLI_TIMEOUT_MS.load(Ordering::SeqCst))
}

/// CLI を1回実行する。終了コード 0 なら標準出力を Some、非 0 なら None。起動できない・時間切れは Err。
fn run_cli<S: AsRef<OsStr>>(cli: &Path, args: &[S]) -> Result<Option<String>, RunError> {
    let mut command = Command::new(cli);
    command.args(args);
    // macOS の Setup 版は、インストーラーの `open` やログイン時の launchd から最小限の PATH で起動される
    // ことがある。npm で入れた CLI は `#!/usr/bin/env node` のスクリプトで、node は CLI と同じフォルダーに
    // あることが多いため、CLI のフォルダーを PATH の先頭に足す。
    #[cfg(target_os = "macos")]
    if let Some(dir) = cli.parent() {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            command.env("PATH", joined);
        }
    }
    match process::run(command, cli_timeout(), OUTPUT_LIMIT) {
        Ok(stdout) => Ok(Some(String::from_utf8_lossy(&stdout).into_owned())),
        Err(RunError::Failed) => Ok(None),
        Err(error) => Err(error),
    }
}

/// パスを比較用に正規化する（区切りを / に揃え、小文字にする。Windows のパスは大小を区別しない）。
fn normalize_for_match(text: &str) -> String {
    text.replace('\\', "/").to_ascii_lowercase()
}

/// `mcp get plumivox` で確かめた、今の登録の状態。
#[derive(Debug, PartialEq, Eq)]
enum Current {
    /// 登録が無い（CLI が非 0 で終わった）。
    Absent,
    /// この実行ファイルを指している。
    Mine,
    /// 別の場所を指している（旧プラグイン・別の導入先）。
    Foreign,
}

/// `mcp get` の出力から状態を決める（出力の形式に依存しないよう、実行ファイルのパスを含むかだけを見る）。
fn classify(stdout: Option<&str>, executable: &Path) -> Current {
    match stdout {
        None => Current::Absent,
        Some(text)
            if normalize_for_match(text)
                .contains(&normalize_for_match(&executable.to_string_lossy())) =>
        {
            Current::Mine
        }
        Some(_) => Current::Foreign,
    }
}

fn current(cli: &Path, executable: &Path) -> Result<Current, RunError> {
    Ok(classify(
        run_cli(cli, &["mcp", "get", SERVER_NAME])?.as_deref(),
        executable,
    ))
}

/// 登録の引数。`claude` は利用者単位（`--scope user`）で、全プロジェクトから使えるようにする。
fn mcp_add_args(executable: &Path, claude: bool) -> Vec<String> {
    let mut args = vec!["mcp".to_string(), "add".to_string()];
    if claude {
        args.extend(["--scope".to_string(), "user".to_string()]);
    }
    args.extend([
        SERVER_NAME.to_string(),
        "--".to_string(),
        executable.to_string_lossy().into_owned(),
        "--stdio".to_string(),
    ]);
    args
}

fn mcp_remove_args(claude: bool) -> Vec<&'static str> {
    let mut args = vec!["mcp", "remove", SERVER_NAME];
    if claude {
        args.extend(["--scope", "user"]);
    }
    args
}

/// 1つの CLI へ登録する。結果は診断ログ用の短い状態名（`unchanged` / `registered` / `replaced`）。
fn register_one(cli: &Path, executable: &Path, claude: bool) -> Result<&'static str, String> {
    let describe = |error: RunError| format!("{error:?}");
    match current(cli, executable).map_err(describe)? {
        Current::Mine => Ok("unchanged"),
        Current::Absent => {
            run_cli(cli, &mcp_add_args(executable, claude))
                .map_err(describe)?
                .ok_or_else(|| "mcp add failed".to_string())?;
            Ok("registered")
        }
        Current::Foreign => {
            // 別の場所を指す同名の登録を外してから付け直す。remove の失敗は add の失敗で検出する。
            let _ = run_cli(cli, &mcp_remove_args(claude));
            run_cli(cli, &mcp_add_args(executable, claude))
                .map_err(describe)?
                .ok_or_else(|| "mcp add failed after removing the previous entry".to_string())?;
            Ok("replaced")
        }
    }
}

/// [`register_tools_for_setup`] の1つの CLI の結果。
pub struct SetupOutcome {
    /// CLI の名前（`claude` / `codex`）。
    pub tool: &'static str,
    /// 登録まで済んだか。CLI が見つからない・失敗した場合は false（次の機会に再試行してよい）。
    pub done: bool,
    /// 診断ログ用の1行。
    pub line: String,
}

/// 指定の CLI だけを登録する（未完了の CLI だけをやり直すために使う）。未知の名前は無視する。
/// どの失敗も例外にしない。
pub fn register_tools_for_setup(executable: &Path, tools: &[&str]) -> Vec<SetupOutcome> {
    let mut outcomes = Vec::new();
    for (tool, claude) in [("claude", true), ("codex", false)] {
        if !tools.contains(&tool) {
            continue;
        }
        let Some(cli) = find_cli(tool) else {
            outcomes.push(SetupOutcome {
                tool,
                done: false,
                line: format!("{tool}: CLI not found; skipped"),
            });
            continue;
        };
        outcomes.push(match register_one(&cli, executable, claude) {
            Ok(status) => SetupOutcome {
                tool,
                done: true,
                line: format!("{tool}: {status}"),
            },
            Err(error) => SetupOutcome {
                tool,
                done: false,
                line: format!("{tool}: failed: {error}"),
            },
        });
    }
    outcomes
}

/// 1つの CLI から、この実行ファイルを指す登録だけを外す。
fn unregister_one(cli: &Path, executable: &Path, claude: bool) -> Result<&'static str, String> {
    let describe = |error: RunError| format!("{error:?}");
    match current(cli, executable).map_err(describe)? {
        Current::Absent => Ok("absent"),
        Current::Foreign => Ok("kept (points at another executable)"),
        Current::Mine => {
            if run_cli(cli, &mcp_remove_args(claude))
                .map_err(describe)?
                .is_some()
            {
                return Ok("removed");
            }
            // 非 0 でも、確かめ直して登録が無ければ「既に無い」として扱う。
            match current(cli, executable).map_err(describe)? {
                Current::Absent => Ok("absent"),
                _ => Err("mcp remove failed".to_string()),
            }
        }
    }
}

/// アンインストール（フック・`--uninstall-setup --apply`）から呼ぶ解除。導入先の実行ファイルを指す登録だけを
/// 消し、別の場所（旧プラグインなど）を指す登録は保つ。失敗は例外にせず結果の行で返す。
pub fn unregister_for_setup(executable: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    for (tool, claude) in [("claude", true), ("codex", false)] {
        let Some(cli) = find_cli(tool) else {
            lines.push(format!("{tool}: CLI not found on PATH; skipped"));
            continue;
        };
        lines.push(match unregister_one(&cli, executable, claude) {
            Ok(status) => format!("{tool}: {status}"),
            Err(error) => format!("{tool}: failed: {error}"),
        });
    }
    lines
}

/// その CLI が見つかるか（プロセスを起動しない軽い確認。登録をやり直すべきかの判定に使う）。
pub fn cli_available(tool: &str) -> bool {
    find_cli(tool).is_some()
}

/// PATH から実行ファイルを探す（Windows は `.exe` だけを対象にし、スクリプトのラッパーは使わない）。
///
/// macOS では PATH に無ければ、CLI の代表的な導入先も探す。Setup 版の初回起動やログイン時の起動は
/// launchd の最小限の PATH で動くため。PATH を先に探すので、PATH に置いた CLI（検証用のスタブを含む）が
/// 常に優先される。
fn find_cli(name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    let dirs: Vec<PathBuf> = dirs
        .into_iter()
        .chain(macos_cli_fallback_dirs(
            &std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default(),
        ))
        .collect();
    dirs.into_iter()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
}

/// macOS で PATH の後に探す、CLI の代表的な導入先（探す順）。
#[cfg(target_os = "macos")]
fn macos_cli_fallback_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if home.is_absolute() {
        // Claude Code の公式インストーラー、旧来のローカル導入、npm の利用者単位の導入先。
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".claude/local"));
        dirs.push(home.join(".npm-global/bin"));
    }
    // Homebrew（Apple Silicon と旧来の場所）。
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_arguments_point_at_the_installed_executable_with_stdio() {
        let exe = Path::new(r"C:\Users\利用者\AppData\Local\PlumivoxApp\current\plumivox.exe");
        assert_eq!(
            mcp_add_args(exe, true),
            [
                "mcp",
                "add",
                "--scope",
                "user",
                "plumivox",
                "--",
                exe.to_str().unwrap(),
                "--stdio"
            ]
        );
        assert_eq!(
            mcp_add_args(exe, false),
            [
                "mcp",
                "add",
                "plumivox",
                "--",
                exe.to_str().unwrap(),
                "--stdio"
            ]
        );
        assert_eq!(
            mcp_remove_args(true),
            ["mcp", "remove", "plumivox", "--scope", "user"]
        );
        assert_eq!(mcp_remove_args(false), ["mcp", "remove", "plumivox"]);
    }

    #[test]
    fn get_output_is_classified_by_the_executable_path_only() {
        let exe = Path::new(r"C:\Users\a\AppData\Local\PlumivoxApp\current\plumivox.exe");
        assert_eq!(classify(None, exe), Current::Absent);
        // 大小・区切りの違いは同じ実行ファイルとみなす。
        assert_eq!(
            classify(
                Some(
                    "plumivox:\n  Command: c:/users/A/appdata/local/plumivoxapp/current/plumivox.exe\n  Args: --stdio\n"
                ),
                exe
            ),
            Current::Mine
        );
        // 旧プラグインの dist を指す登録は別物。
        assert_eq!(
            classify(
                Some(r"Command: C:\Users\a\.claude\plugins\plumivox\dist\plumivox.exe"),
                exe
            ),
            Current::Foreign
        );
    }

    #[test]
    fn cli_timeout_defaults_to_the_hook_bound_and_can_be_raised() {
        // 既定はフック用の3秒。フック以外の入口が長めの上限へ切り替える。
        assert_eq!(cli_timeout(), HOOK_CLI_TIMEOUT);
        set_cli_timeout(RETRY_CLI_TIMEOUT);
        assert_eq!(cli_timeout(), RETRY_CLI_TIMEOUT);
        set_cli_timeout(HOOK_CLI_TIMEOUT);
    }
}
