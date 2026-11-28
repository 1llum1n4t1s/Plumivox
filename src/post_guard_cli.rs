//! `plumivox --post-guard <status|clear [--account 設定名 | --user Xユーザー名]>`。投稿の冷却（`post_guard`）の状態の確認と解除。
//!
//! 冷却は MCP からは解除できない。利用者本人がこの CLI で確認・解除する。表示は状態ファイルにある
//! 固定のコード・分類・時刻だけで、資格情報・投稿本文・提供元の文面は出さない。人が読む文言は辞書から引く。
//! 終了コードは `--rejection-log` と同じく、成功 0、保存先の異常・送信中で解除できない 1、
//! 引数の誤り・端末でない・確認の不一致 2。
use crate::{
    locale::{tr, tr_with},
    post_guard::{self, Entry, Status, Target},
    session,
};
use chrono::{DateTime, SecondsFormat};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

/// 履歴の表示件数（新しい側から）。
const HISTORY_LINES: usize = 5;
/// 状態フォルダー名（`post_guard` の保存先と同じ。状態が1件も無いときの表示用）。
const DIRECTORY: &str = "post-guard";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Status,
    Clear(Scope),
}

/// 解除の対象。
#[derive(Debug, PartialEq, Eq)]
enum Scope {
    All,
    /// 設定名。
    Account(String),
    /// X のユーザー名。
    User(String),
}

impl Scope {
    /// 確認で入力させる文字列（すべては `all`）。
    fn label(&self) -> &str {
        match self {
            Scope::All => "all",
            Scope::Account(name) | Scope::User(name) => name,
        }
    }

    fn target(&self) -> Target<'_> {
        match self {
            Scope::All => Target::All,
            Scope::Account(name) => Target::Account(name),
            Scope::User(name) => Target::UserName(name),
        }
    }
}

fn parse_command(args: &[String]) -> Option<Command> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["status"] => Some(Command::Status),
        ["clear"] => Some(Command::Clear(Scope::All)),
        ["clear", "--account", name] if !name.trim().is_empty() => {
            Some(Command::Clear(Scope::Account(name.trim().to_owned())))
        }
        ["clear", "--user", name] if !name.trim().is_empty() => {
            Some(Command::Clear(Scope::User(name.trim().to_owned())))
        }
        _ => None,
    }
}

fn date_text(seconds: u64) -> String {
    DateTime::from_timestamp(seconds as i64, 0)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// 残り時間（`1h 05m 10s` の形。単位は言語によらない）。
fn duration_text(secs: u64) -> String {
    let (hours, minutes, seconds) = (secs / 3600, secs % 3600 / 60, secs % 60);
    match (hours, minutes) {
        (0, 0) => format!("{seconds}s"),
        (0, _) => format!("{minutes}m {seconds:02}s"),
        _ => format!("{hours}h {minutes:02}m {seconds:02}s"),
    }
}

fn account_name(account: &Option<String>) -> String {
    account
        .clone()
        .unwrap_or_else(|| tr("cli.post_guard.account_unknown").to_owned())
}

fn state_name(status: Status) -> &'static str {
    match status {
        Status::Clear => tr("cli.post_guard.state.clear"),
        Status::Paused => tr("cli.post_guard.state.paused"),
        Status::Sending => tr("cli.post_guard.state.sending"),
        Status::Interrupted => tr("cli.post_guard.state.interrupted"),
        Status::Unreadable => tr("cli.post_guard.state.unreadable"),
    }
}

fn or_dash<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn write_entry(entry: &Entry, out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "{}",
        tr_with(
            "cli.post_guard.entry",
            &[
                ("account", &account_name(&entry.account)),
                ("state", state_name(entry.status)),
            ],
        )
    );
    if let Some(reason) = &entry.reason {
        let _ = writeln!(
            out,
            "{}",
            tr_with("cli.post_guard.reason", &[("reason", reason)])
        );
    }
    if let Some(until) = entry.until {
        let _ = writeln!(
            out,
            "{}",
            tr_with(
                "cli.post_guard.until",
                &[
                    ("until", &date_text(until)),
                    ("remaining", &duration_text(entry.remaining_secs)),
                ],
            )
        );
    }
    if entry.status != Status::Unreadable {
        let _ = writeln!(
            out,
            "{}",
            tr_with(
                "cli.post_guard.strikes",
                &[("count", &entry.strikes.to_string())],
            )
        );
    }
    if let Some(at) = entry.last_attempt_at {
        let _ = writeln!(
            out,
            "{}",
            tr_with("cli.post_guard.last_attempt", &[("at", &date_text(at))])
        );
    }
    if let Some(at) = entry.updated_at {
        let _ = writeln!(
            out,
            "{}",
            tr_with("cli.post_guard.updated", &[("at", &date_text(at))])
        );
    }
    if entry.history.is_empty() {
        return;
    }
    let _ = writeln!(out, "{}", tr("cli.post_guard.history_head"));
    let skip = entry.history.len().saturating_sub(HISTORY_LINES);
    for event in entry.history.iter().skip(skip) {
        let class = if event.reasons.is_empty() {
            "-".to_owned()
        } else {
            event.reasons.join(",")
        };
        let _ = writeln!(
            out,
            "{}",
            tr_with(
                "cli.post_guard.history_line",
                &[
                    ("at", &date_text(event.at)),
                    ("stage", &event.stage),
                    ("code", &event.code),
                    ("class", &class),
                    ("x_error_code", &or_dash(event.x_error_code)),
                    ("x_http_status", &or_dash(event.x_http_status)),
                ],
            )
        );
    }
}

fn cmd_status(root: &Path, out: &mut dyn Write) -> crate::error::Result<()> {
    let entries = post_guard::list(root)?;
    let path = entries
        .first()
        .and_then(|entry| entry.path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| root.join(DIRECTORY));
    let _ = writeln!(
        out,
        "{}",
        tr_with(
            "cli.post_guard.status_path",
            &[("path", &path.display().to_string())],
        )
    );
    if entries.is_empty() {
        let _ = writeln!(out, "{}", tr("cli.post_guard.status_none"));
    }
    for entry in &entries {
        write_entry(entry, out);
    }
    Ok(())
}

/// 解除の結果を表示し、終了コードを返す。送信中の状態が残れば 1。
fn report_clear(report: &post_guard::ClearReport, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    for cleared in &report.cleared {
        let text = if cleared.was_paused {
            tr_with(
                "cli.post_guard.cleared_active",
                &[("account", &account_name(&cleared.account))],
            )
        } else {
            tr_with(
                "cli.post_guard.cleared_idle",
                &[("account", &account_name(&cleared.account))],
            )
        };
        let _ = writeln!(out, "{text}");
    }
    if !report.busy.is_empty() {
        let count = report.busy.len().to_string();
        let _ = writeln!(
            err,
            "{}",
            tr_with("cli.post_guard.clear_busy", &[("count", &count)])
        );
        return 1;
    }
    if report.cleared.is_empty() {
        let _ = writeln!(out, "{}", tr("cli.post_guard.clear_none"));
    }
    0
}

fn cmd_clear(
    root: &Path,
    scope: &Scope,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let label = scope.label();
    let _ = write!(
        out,
        "{}",
        tr_with("cli.post_guard.clear_prompt", &[("target", label)])
    );
    let _ = out.flush();
    let mut line = String::new();
    if input.read_line(&mut line).is_err() || !line.trim().eq_ignore_ascii_case(label) {
        let _ = writeln!(err, "{}", tr("cli.post_guard.clear_cancelled"));
        return 2;
    }
    match post_guard::clear(root, scope.target()) {
        Ok(report) => {
            let code = report_clear(&report, out, err);
            // 状態には設定名がユーザー名と関わるとき `<user>` で残るので、設定名では選べない。
            if code == 0 && report.cleared.is_empty() && matches!(scope, Scope::Account(_)) {
                let _ = writeln!(out, "{}", tr("cli.post_guard.clear_account_hint"));
            }
            code
        }
        Err(_) => {
            let _ = writeln!(err, "{}", tr("cli.post_guard.unavailable"));
            1
        }
    }
}

fn cli(
    root: Option<PathBuf>,
    args: &[String],
    stdin_is_terminal: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let Some(command) = parse_command(args) else {
        let _ = writeln!(err, "{}", tr("cli.usage"));
        return 2;
    };
    if matches!(command, Command::Clear(_)) && !stdin_is_terminal {
        let _ = writeln!(err, "{}", tr("cli.post_guard.clear_needs_terminal"));
        return 2;
    }
    let Some(root) = root else {
        let _ = writeln!(err, "{}", tr("cli.post_guard.unavailable"));
        return 1;
    };
    match command {
        Command::Status => match cmd_status(&root, out) {
            Ok(()) => 0,
            Err(_) => {
                let _ = writeln!(err, "{}", tr("cli.post_guard.unavailable"));
                1
            }
        },
        Command::Clear(scope) => cmd_clear(&root, &scope, input, out, err),
    }
}

/// `plumivox --post-guard <status|clear [--account 設定名 | --user Xユーザー名]>`。標準出力を使ってよい（stdio MCP ではない）。
pub fn run_cli(args: &[String]) -> i32 {
    use std::io::IsTerminal;
    cli(
        session::state_root(None),
        args,
        std::io::stdin().is_terminal(),
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_shows_state_and_only_the_latest_history() {
        let event = |at: u64, code: &str| post_guard::Event {
            at,
            stage: "post".to_owned(),
            code: code.to_owned(),
            reasons: vec!["daily_limit".to_owned()],
            x_error_code: Some(344),
            x_http_status: None,
        };
        let entry = Entry {
            path: PathBuf::from("x.json"),
            account: Some("main".to_owned()),
            status: Status::Paused,
            reason: Some("daily_limit".to_owned()),
            until: Some(1_800_003_600),
            remaining_secs: 3725,
            strikes: 2,
            last_attempt_at: Some(1_800_000_000),
            updated_at: Some(1_800_000_000),
            history: (0..7)
                .map(|i| event(1_800_000_000 + i, &format!("c{i}")))
                .collect(),
        };
        let mut out = Vec::new();
        write_entry(&entry, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("main") && text.contains("daily_limit"));
        assert!(text.contains("1h 02m 05s") && text.contains("xErrorCode=344"));
        assert!(text.contains("xHttpStatus=-"));
        assert!(text.contains("code=c6") && text.contains("code=c2"));
        assert!(!text.contains("code=c1"));
    }

    #[test]
    fn busy_states_make_the_clear_fail() {
        let report = post_guard::ClearReport {
            cleared: Vec::new(),
            busy: vec![PathBuf::from("x.json")],
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(report_clear(&report, &mut out, &mut err), 1);
        assert!(!err.is_empty());
    }
}
