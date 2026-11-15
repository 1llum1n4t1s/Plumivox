//! Setup 版の登録（Claude Code・Codex の MCP と、ログオン時の適用）を、必要なときだけ行う（全 OS 共通）。
//!
//! Windows では Velopack のフック（install では状態を忘れてから、updated ではそのまま）からも呼ぶ。フックは
//! CLI 1回を3秒で打ち切るため、終わらなかった CLI は `done` に入らず、ログオン時の `--update-apply`・引数なしの
//! 起動・stdio MCP の起動で（30秒の上限で）やり直す。MCP の登録が消えると Claude Code・Codex は Plumivox を
//! 起動しないので、ログオン時の `--update-apply` が確実な再試行の機会になる。
//!
//! # なぜ状態ファイルが要るか
//! Velopack の高速フック（install / updated / uninstall）は Windows でしか呼ばれない（velopack crate
//! 1.2.161 の `VelopackApp` で、フックの登録関数自体が Windows 限定）。macOS の .pkg は導入後にアプリを
//! 1回起動するだけで、更新はアプリを起動しない。そこで Plumivox 自身が起動したときに「登録が今の自分を
//! 指しているか」を確かめ、違うときだけ登録する。Windows でも、フックで終わらなかった登録の再試行に使う。
//!
//! # 軽くする工夫
//! データフォルダーの `app-update\setup-registration.json` に、登録した実行ファイルのパス・版・登録済みの
//! CLI を覚え、次の場合だけ処理する。普段の起動では、ファイル1つの読み込みと PATH の走査、旧プラグインの検知（[`super::legacy`]。設定ファイルを読むだけ）で終わる。
//! - パスが変わった（初回・導入先の移動）: ログオン時の適用と、見つかった全ての CLI を登録する。
//! - 版だけが変わった（更新の後）: ログオン時の適用の定義だけを張り直す。MCP の登録はパスが同じなら
//!   有効なままなので CLI は呼ばない（使用中のセッションの登録を外して付け直すことを避けるため）。
//! - 同じパス・版で、前回見つからなかった・失敗した CLI が今は見つかる: その CLI だけを登録する。
//!
//! 複数のプロセスが同時に起動しても二重に登録しないよう、`setup-registration.lock` を try-lock する。
use super::{SetupContext, legacy, log_line, logon};
use crate::connect;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

/// 登録の状態を覚えるファイル（状態フォルダー直下）。
pub const STATE_FILE: &str = "setup-registration.json";
/// 登録の排他に使うファイル（状態フォルダー直下）。
pub const LOCK_FILE: &str = "setup-registration.lock";

/// 前回の登録の状態。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    /// 登録した実行ファイルのパス。
    pub executable: Option<String>,
    /// 登録したときの版。
    pub version: Option<String>,
    /// 登録まで済んだ CLI（`claude` / `codex`）。
    pub done: BTreeSet<String>,
    /// ログオン時の適用を登録したか（別の導入の値が残っていて触れなかった場合は false）。
    pub logon: bool,
    /// 有効なままの旧プラグイン（`"codex:plumivox@plumivox-local"` 形式。[`legacy`]）。機械可読な結果として
    /// 残す。前回の検知と違うときだけ警告を診断ログへ出し直すための記録でもある。
    pub legacy: BTreeSet<String>,
}

/// 何をするか。
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// 状態を作り直す（パスが変わった）。
    pub fresh: bool,
    /// ログオン時の適用を登録し直す。
    pub logon: bool,
    /// 登録する CLI。
    pub tools: Vec<&'static str>,
}

/// JSON から状態を作る。壊れていれば既定（未登録）として扱い、決して失敗しない。
pub fn parse(bytes: &[u8]) -> State {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return State::default();
    };
    State {
        executable: value["executable"].as_str().map(str::to_owned),
        version: value["version"].as_str().map(str::to_owned),
        done: value["done"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        logon: value["logon"].as_bool().unwrap_or(false),
        legacy: value["legacyPlugins"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

pub fn load(state_dir: &Path) -> State {
    let path = state_dir.join(STATE_FILE);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => fs::read(&path).map(|b| parse(&b)).unwrap_or_default(),
        _ => State::default(),
    }
}

fn save(state_dir: &Path, state: &State) -> super::Result<()> {
    let value = json!({
        "executable": state.executable,
        "version": state.version,
        "done": state.done.iter().collect::<Vec<_>>(),
        "logon": state.logon,
        "legacyPlugins": state.legacy.iter().collect::<Vec<_>>(),
    });
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| "JSON".to_string())?;
    bytes.push(b'\n');
    super::atomic_write(&state_dir.join(STATE_FILE), &bytes)
}

/// 状態ファイルを消す（インストール直後とアンインストール用。次に導入したときに登録し直すため）。
/// 無ければ何もしない。
pub fn forget(state_dir: &Path) -> std::io::Result<bool> {
    match fs::remove_file(state_dir.join(STATE_FILE)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// 前回の状態と今の導入から、するべきことを決める。何もしなくてよければ None。
/// `available` は CLI が見つかるかの判定（テストで差し替える）。
pub fn plan(
    state: &State,
    executable: &str,
    version: &str,
    available: impl Fn(&str) -> bool,
) -> Option<Plan> {
    if state.executable.as_deref() != Some(executable) {
        return Some(Plan {
            fresh: true,
            logon: true,
            tools: connect::SETUP_TOOLS.to_vec(),
        });
    }
    let updated = state.version.as_deref() != Some(version);
    let tools: Vec<&'static str> = connect::SETUP_TOOLS
        .into_iter()
        .filter(|tool| !state.done.contains(*tool) && available(tool))
        .collect();
    let logon = updated || !state.logon;
    if !logon && tools.is_empty() {
        return None;
    }
    Some(Plan {
        fresh: false,
        logon,
        tools,
    })
}

/// 必要なら登録する。`label` は診断ログの見出し。失敗は例外にせず、診断ログにだけ残す。
///
/// 登録の要否とは別に、旧プラグイン（[`legacy`]）が有効なままかも毎回確かめる（設定ファイルを2つ読むだけ）。
/// 前回の記録と違うときだけ状態ファイルへ書き、警告を診断ログへ出す。Codex・Claude Code の設定は読むだけで、
/// 書き換えない。
pub fn ensure(state_dir: &Path, context: &SetupContext, label: &str) {
    let executable = context.executable.to_string_lossy().into_owned();
    let detected = legacy::detect();
    if plan(
        &load(state_dir),
        &executable,
        &context.version,
        connect::cli_available,
    )
    .is_none()
        && load(state_dir).legacy == detected
    {
        return;
    }
    // 他のプロセスが登録中なら任せる（二重に CLI を呼ばない）。
    let Some(_lock) = super::try_lock_named(state_dir, LOCK_FILE) else {
        return;
    };
    // ロックを取る間に他のプロセスが終えていることがある。読み直して決め直す。
    let previous = load(state_dir);
    let plan = plan(
        &previous,
        &executable,
        &context.version,
        connect::cli_available,
    );
    if plan.is_none() && previous.legacy == detected {
        return;
    }
    // 登録が不要で旧プラグインの記録だけが違うときは、何もしない計画で進める。
    let plan = plan.unwrap_or(Plan {
        fresh: false,
        logon: false,
        tools: Vec::new(),
    });
    let mut state = if plan.fresh {
        State {
            executable: Some(executable.clone()),
            ..State::default()
        }
    } else {
        previous.clone()
    };
    state.version = Some(context.version.clone());
    // 旧プラグインの検知は、新しい導入（fresh）では前回の記録が無いので必ず出し、そうでなければ変化したときだけ出す。
    if !detected.is_empty() && (plan.fresh || detected != previous.legacy) {
        log_line(&format!(
            "{label}: legacy plugin still enabled in {}: {}; disable the plugin's MCP or remove the plugin to avoid starting Plumivox twice",
            legacy::tool_labels(&detected).join(", "),
            detected.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    state.legacy = detected;
    if plan.logon {
        match logon::register(&context.app_id, &context.executable) {
            Ok(result) => {
                state.logon = !matches!(result, logon::Registration::ForeignKept);
                log_line(&format!("{label}: logon apply {result:?}"));
            }
            Err(error) => {
                state.logon = false;
                log_line(&format!("{label}: logon apply failed: {error}"));
            }
        }
    }
    for outcome in connect::register_tools_for_setup(&context.executable, &plan.tools) {
        if outcome.done {
            state.done.insert(outcome.tool.to_string());
        } else {
            state.done.remove(outcome.tool);
        }
        log_line(&format!("{label}: {}", outcome.line));
    }
    if let Err(error) = save(state_dir, &state) {
        log_line(&format!("{label}: registration state not saved: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(state: &State, tools: &[&str]) -> State {
        State {
            done: tools.iter().map(|t| t.to_string()).collect(),
            ..state.clone()
        }
    }

    fn registered() -> State {
        State {
            executable: Some(r"C:\A\current\plumivox.exe".into()),
            version: Some("1.0.0".into()),
            done: ["claude".to_string(), "codex".to_string()].into(),
            logon: true,
            legacy: BTreeSet::new(),
        }
    }

    #[test]
    fn a_new_or_moved_install_registers_everything() {
        let everything = Plan {
            fresh: true,
            logon: true,
            tools: vec!["claude", "codex"],
        };
        // CLI が見つからなくても、新しい導入では全てを試す（見つからない CLI は done に入らないだけ）。
        assert_eq!(
            plan(
                &State::default(),
                r"C:\A\current\plumivox.exe",
                "1.0.0",
                |_| false
            ),
            Some(everything)
        );
        let moved = State {
            executable: Some(r"C:\Old\current\plumivox.exe".into()),
            ..registered()
        };
        assert!(
            plan(&moved, r"C:\A\current\plumivox.exe", "1.0.0", |_| true)
                .unwrap()
                .fresh
        );
    }

    #[test]
    fn nothing_happens_when_everything_is_current() {
        let state = registered();
        let exe = r"C:\A\current\plumivox.exe";
        assert_eq!(plan(&state, exe, "1.0.0", |_| true), None);
        // 見つからなかった CLI は、見つかるまでは何もしない（毎回の起動でプロセスを起動しない）。
        let partial = done(&state, &["claude"]);
        assert_eq!(plan(&partial, exe, "1.0.0", |_| false), None);
        // 見つかったら、その CLI だけを登録する。
        assert_eq!(
            plan(&partial, exe, "1.0.0", |tool| tool == "codex"),
            Some(Plan {
                fresh: false,
                logon: false,
                tools: vec!["codex"],
            })
        );
        // ログオン時の適用を登録できていなければ（別の導入の値が残っていた等）、それだけをやり直す。
        assert_eq!(
            plan(
                &State {
                    logon: false,
                    ..state
                },
                exe,
                "1.0.0",
                |_| true
            ),
            Some(Plan {
                fresh: false,
                logon: true,
                tools: vec![],
            })
        );
    }

    #[test]
    fn an_update_refreshes_only_the_logon_entry_without_cli_calls() {
        assert_eq!(
            plan(&registered(), r"C:\A\current\plumivox.exe", "1.0.1", |_| {
                true
            }),
            Some(Plan {
                fresh: false,
                logon: true,
                tools: vec![],
            })
        );
    }

    #[test]
    fn state_round_trips_and_broken_files_mean_unregistered() {
        let dir = super::super::tests::scratch("registration");
        assert_eq!(load(&dir), State::default());
        let state = State {
            executable: Some(r"C:\A\current\plumivox.exe".into()),
            version: Some("9.0.0".into()),
            done: ["codex".to_string()].into(),
            logon: true,
            legacy: BTreeSet::from([legacy::entry("codex", "plumivox@plumivox-local")]),
        };
        save(&dir, &state).unwrap();
        assert_eq!(load(&dir), state);
        // 機械可読な形（旧プラグインの検知）がそのまま残る。
        let json: Value = serde_json::from_slice(&fs::read(dir.join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(
            json["legacyPlugins"],
            json!(["codex:plumivox@plumivox-local"])
        );
        // 旧版の状態ファイル（legacyPlugins なし）は、検知なしとして読める。
        fs::write(
            dir.join(STATE_FILE),
            br#"{"executable":"x","version":"1","done":[],"logon":true}"#,
        )
        .unwrap();
        assert!(load(&dir).legacy.is_empty());
        fs::write(dir.join(STATE_FILE), b"{ broken").unwrap();
        assert_eq!(load(&dir), State::default());
        assert!(forget(&dir).unwrap());
        assert!(!forget(&dir).unwrap());
        let _ = fs::remove_dir_all(dir);
    }
}
