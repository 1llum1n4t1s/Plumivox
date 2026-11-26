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
//! ただし `skipped = toolport` の CLI が `auto` で残っているあいだは、起動のたびに CLI の有無に加えて、そのクライアントの
//! 設定ファイルと Toolport の registry も読んで、ゲートウェイが消えていないか確かめる（読むだけで書かない）。
//!
//! - パスが変わった（初回・導入先の移動）: ログオン時の適用と、見つかった全ての CLI を登録する。
//! - 版だけが変わった（更新の後）: ログオン時の適用の定義だけを張り直す。MCP の登録はパスが同じなら
//!   有効なままなので CLI は呼ばない（使用中のセッションの登録を外して付け直すことを避けるため）。
//! - 同じパス・版で、前回見つからなかった・失敗した CLI が今は見つかる: その CLI だけを登録する。
//!
//! 複数のプロセスが同時に起動しても二重に登録しないよう、`setup-registration.lock` を try-lock する。
//!
//! # Toolport ゲートウェイ経由の提供
//! Toolport（[`crate::toolport`]）が既に Plumivox を提供している環境では、新しく直接登録を足さず、状態の
//! `skipped` に理由（`toolport` / `off`）を残す。`app-update.json` の `mcp_registration`（`auto` 既定 /
//! `direct` / `off`）で方針を変えられる。既に済んだ直接登録（`done`）は、どの方針でも外さない。
//! 省いたあとにクライアントから Toolport ゲートウェイが消えたら（`NotConnected`）、`auto` では直接登録に戻す。
use super::{McpRegistration, SetupContext, legacy, log_line, logon};
use crate::{
    connect,
    toolport::{self, ClientId, Detection, Env, Product},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// 判定する製品（Toolport 側のサーバーを `command` のファイル名と起動引数で見分ける）。
const PRODUCT: Product = Product {
    binary_stem: "plumivox",
    mode_arg: "--stdio",
};

/// 省いた理由。
const SKIP_TOOLPORT: &str = "toolport";
const SKIP_OFF: &str = "off";

/// CLI の名前から、Toolport の判定のクライアントを引く。
fn client_for(tool: &str) -> ClientId {
    if tool == "claude" {
        ClientId::ClaudeCode
    } else {
        ClientId::Codex
    }
}

/// 実際の環境で、その CLI のクライアントへ Toolport が Plumivox を提供しているか判定する。
fn detect_toolport(tool: &str) -> Detection {
    toolport::detect(&PRODUCT, client_for(tool), &Env::process())
}

/// 判定結果の診断ログ1行（種類と、Unknown の理由。秘密値は含まない）。
fn detection_line(tool: &str, detection: &Detection) -> String {
    match detection {
        Detection::Unknown(reason) => {
            format!("{tool}: toolport detection {} ({reason})", detection.kind())
        }
        _ => format!("{tool}: toolport detection {}", detection.kind()),
    }
}

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
    /// 直接登録を省いた CLI と理由（`toolport` / `off`）。`done`（直接登録済み）とは別で、古い状態ファイルでは空。
    pub skipped: BTreeMap<String, String>,
}

/// 何をするか。
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// 状態を作り直す（パスが変わった）。
    pub fresh: bool,
    /// ログオン時の適用を登録し直す。
    pub logon: bool,
    /// 直接登録する CLI。
    pub tools: Vec<&'static str>,
    /// 直接登録を省く CLI と理由（`skipped` へ記録する。CLI は呼ばない）。
    pub skip: Vec<(&'static str, &'static str)>,
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
        skipped: value["skipped"]
            .as_object()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|(tool, reason)| Some((tool.clone(), reason.as_str()?.to_owned())))
                    .filter(|(_, reason)| reason == SKIP_TOOLPORT || reason == SKIP_OFF)
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
        "skipped": state.skipped,
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

/// 1つの CLI についての判断。
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// 直接登録する。
    Register,
    /// 直接登録を省く（理由を `skipped` へ記録する）。
    Skip(&'static str),
    /// 何もしない。
    Nothing,
}

/// 新しく決める場合（新規導入・`skipped = off` の見直し・前回登録できなかった CLI）の判断。
/// `auto` で Toolport が提供していれば省く。`needs_cli` のときは CLI が見つからなければ何もしない。
fn decide_new(
    tool: &'static str,
    mode: McpRegistration,
    needs_cli: bool,
    available: &impl Fn(&str) -> bool,
    served: &impl Fn(&str) -> Detection,
) -> Decision {
    if mode == McpRegistration::Off {
        return Decision::Skip(SKIP_OFF);
    }
    if needs_cli && !available(tool) {
        return Decision::Nothing;
    }
    if mode == McpRegistration::Auto && matches!(served(tool), Detection::Served { .. }) {
        return Decision::Skip(SKIP_TOOLPORT);
    }
    Decision::Register
}

/// 導入済み（状態のパスが今と同じ）の CLI の判断。`done` は、どの方針でも外さず触れない。
fn decide_existing(
    state: &State,
    tool: &'static str,
    mode: McpRegistration,
    available: &impl Fn(&str) -> bool,
    served: &impl Fn(&str) -> Detection,
) -> Decision {
    if state.done.contains(tool) {
        return Decision::Nothing;
    }
    if mode == McpRegistration::Off {
        // 未完了（done にも skipped にも無い）ものだけ off として記録する。
        return if state.skipped.contains_key(tool) {
            Decision::Nothing
        } else {
            Decision::Skip(SKIP_OFF)
        };
    }
    match state.skipped.get(tool).map(String::as_str) {
        // direct: 省いていたものを直接登録する。
        _ if mode == McpRegistration::Direct => {
            if available(tool) {
                Decision::Register
            } else {
                Decision::Nothing
            }
        }
        // auto: Toolport のゲートウェイが消えたときだけ直接登録に戻す。提供中・無効化・不明なら何もしない。
        Some(SKIP_TOOLPORT) => {
            if available(tool) && served(tool) == Detection::NotConnected {
                Decision::Register
            } else {
                Decision::Nothing
            }
        }
        // auto: off だったものは新規扱いで判定し直す。
        Some(_) => decide_new(tool, mode, true, available, served),
        // 前回見つからなかった・失敗した CLI。
        None => decide_new(tool, mode, true, available, served),
    }
}

/// 前回の状態と今の導入から、するべきことを決める。何もしなくてよければ None。
/// `available` は CLI が見つかるかの判定、`served` は Toolport が提供しているかの判定（どちらもテストで差し替える）。
pub fn plan(
    state: &State,
    executable: &str,
    version: &str,
    mode: McpRegistration,
    available: impl Fn(&str) -> bool,
    served: impl Fn(&str) -> Detection,
) -> Option<Plan> {
    let fresh = state.executable.as_deref() != Some(executable);
    let mut tools = Vec::new();
    let mut skip = Vec::new();
    for tool in connect::SETUP_TOOLS {
        let decision = if fresh {
            // CLI が見つからなくても、新しい導入では全てを試す（見つからない CLI は done に入らないだけ）。
            decide_new(tool, mode, false, &available, &served)
        } else {
            decide_existing(state, tool, mode, &available, &served)
        };
        match decision {
            Decision::Register => tools.push(tool),
            Decision::Skip(reason)
                if state.skipped.get(tool).map(String::as_str) != Some(reason) || fresh =>
            {
                skip.push((tool, reason))
            }
            Decision::Skip(_) | Decision::Nothing => {}
        }
    }
    if fresh {
        return Some(Plan {
            fresh: true,
            logon: true,
            tools,
            skip,
        });
    }
    let updated = state.version.as_deref() != Some(version);
    let logon = updated || !state.logon;
    if !logon && tools.is_empty() && skip.is_empty() {
        return None;
    }
    Some(Plan {
        fresh: false,
        logon,
        tools,
        skip,
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
    let settings = super::load_settings(state_dir);
    let mode = settings.mcp_registration;
    // 不正値は、早期 return より前に毎回記録する。
    if settings.mcp_registration_invalid {
        log_line(&format!(
            "{label}: unknown mcp_registration value in {}; treated as auto",
            super::SETTINGS_FILE
        ));
    }
    let early = load(state_dir);
    if plan(
        &early,
        &executable,
        &context.version,
        mode,
        connect::cli_available,
        detect_toolport,
    )
    .is_none()
        && early.legacy == detected
    {
        return;
    }
    // 他のプロセスが登録中なら任せる（二重に CLI を呼ばない）。
    let Some(_lock) = super::try_lock_named(state_dir, LOCK_FILE) else {
        return;
    };
    // ロックを取る間に他のプロセスが終えていることがある。読み直して決め直す。
    let previous = load(state_dir);
    // 判定したクライアントの結果を覚え、登録処理が動いた回に記録する。
    let detections = RefCell::new(BTreeMap::new());
    let plan = plan(
        &previous,
        &executable,
        &context.version,
        mode,
        connect::cli_available,
        |tool| {
            let detection = detect_toolport(tool);
            detections
                .borrow_mut()
                .insert(tool.to_string(), detection.clone());
            detection
        },
    );
    if plan.is_none() && previous.legacy == detected {
        return;
    }
    for (tool, detection) in detections.into_inner() {
        log_line(&format!("{label}: {}", detection_line(&tool, &detection)));
    }
    // 登録が不要で旧プラグインの記録だけが違うときは、何もしない計画で進める。
    let plan = plan.unwrap_or(Plan {
        fresh: false,
        logon: false,
        tools: Vec::new(),
        skip: Vec::new(),
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
    for (tool, reason) in &plan.skip {
        state.skipped.insert(tool.to_string(), reason.to_string());
        log_line(&format!(
            "{label}: {tool}: direct registration skipped ({reason})"
        ));
    }
    for outcome in connect::register_tools_for_setup(&context.executable, &plan.tools) {
        if outcome.done {
            state.done.insert(outcome.tool.to_string());
            state.skipped.remove(outcome.tool);
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
            skipped: BTreeMap::new(),
        }
    }

    const EXE: &str = r"C:\A\current\plumivox.exe";

    fn served() -> Detection {
        Detection::Served {
            server_id: "plumivox".into(),
            command: r"D:\Toolport\runtimes\plumivox\plumivox.exe".into(),
        }
    }

    fn skipped(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(tool, reason)| (tool.to_string(), reason.to_string()))
            .collect()
    }

    /// 判定を注入して計画を作る。`status` は両クライアント共通の Toolport の判定。
    fn plan_with(
        state: &State,
        version: &str,
        mode: McpRegistration,
        available: impl Fn(&str) -> bool,
        status: Detection,
    ) -> Option<Plan> {
        plan(state, EXE, version, mode, available, |_| status.clone())
    }

    fn plan_of(
        tools: &[&'static str],
        skip: &[(&'static str, &'static str)],
        fresh: bool,
        logon: bool,
    ) -> Plan {
        Plan {
            fresh,
            logon,
            tools: tools.to_vec(),
            skip: skip.to_vec(),
        }
    }

    #[test]
    fn a_new_or_moved_install_registers_everything() {
        // CLI が見つからなくても、新しい導入では全てを試す（見つからない CLI は done に入らないだけ）。
        assert_eq!(
            plan_with(
                &State::default(),
                "1.0.0",
                McpRegistration::Auto,
                |_| false,
                Detection::NotConnected
            ),
            Some(plan_of(&["claude", "codex"], &[], true, true))
        );
        let moved = State {
            executable: Some(r"C:\Old\current\plumivox.exe".into()),
            ..registered()
        };
        assert!(
            plan_with(
                &moved,
                "1.0.0",
                McpRegistration::Auto,
                |_| true,
                Detection::NotConnected
            )
            .unwrap()
            .fresh
        );
    }

    #[test]
    fn nothing_happens_when_everything_is_current() {
        let state = registered();
        let auto = McpRegistration::Auto;
        let none = Detection::NotConnected;
        assert_eq!(
            plan_with(&state, "1.0.0", auto, |_| true, none.clone()),
            None
        );
        // 見つからなかった CLI は、見つかるまでは何もしない（毎回の起動でプロセスを起動しない）。
        let partial = done(&state, &["claude"]);
        assert_eq!(
            plan_with(&partial, "1.0.0", auto, |_| false, none.clone()),
            None
        );
        // 見つかったら、その CLI だけを登録する。
        assert_eq!(
            plan_with(
                &partial,
                "1.0.0",
                auto,
                |tool| tool == "codex",
                none.clone()
            ),
            Some(plan_of(&["codex"], &[], false, false))
        );
        // ログオン時の適用を登録できていなければ（別の導入の値が残っていた等）、それだけをやり直す。
        assert_eq!(
            plan_with(
                &State {
                    logon: false,
                    ..state
                },
                "1.0.0",
                auto,
                |_| true,
                none
            ),
            Some(plan_of(&[], &[], false, true))
        );
    }

    #[test]
    fn an_update_refreshes_only_the_logon_entry_without_cli_calls() {
        for mode in [
            McpRegistration::Auto,
            McpRegistration::Direct,
            McpRegistration::Off,
        ] {
            assert_eq!(
                plan_with(&registered(), "1.0.1", mode, |_| true, served()),
                Some(plan_of(&[], &[], false, true))
            );
        }
    }

    #[test]
    fn a_fresh_install_skips_direct_registration_only_when_toolport_serves_it() {
        let fresh = State::default();
        let go = |mode, status| plan_with(&fresh, "1.0.0", mode, |_| true, status);
        // auto: 提供中なら CLI を呼ばず省く。他の判定は従来どおり直接登録。
        assert_eq!(
            go(McpRegistration::Auto, served()),
            Some(plan_of(
                &[],
                &[("claude", "toolport"), ("codex", "toolport")],
                true,
                true
            ))
        );
        for status in [
            Detection::NotConnected,
            Detection::NotServed,
            Detection::Unknown("x".into()),
        ] {
            assert_eq!(
                go(McpRegistration::Auto, status),
                Some(plan_of(&["claude", "codex"], &[], true, true))
            );
        }
        // direct は判定に関わらず直接登録、off は何も登録せず理由を残す。
        assert_eq!(
            go(McpRegistration::Direct, served()),
            Some(plan_of(&["claude", "codex"], &[], true, true))
        );
        assert_eq!(
            go(McpRegistration::Off, Detection::NotConnected),
            Some(plan_of(
                &[],
                &[("claude", "off"), ("codex", "off")],
                true,
                true
            ))
        );
        // クライアントごとに判定する（Claude Code だけが提供されている場合）。
        let only_claude = plan(
            &fresh,
            EXE,
            "1.0.0",
            McpRegistration::Auto,
            |_| true,
            |tool| {
                if tool == "claude" {
                    served()
                } else {
                    Detection::NotConnected
                }
            },
        );
        assert_eq!(
            only_claude,
            Some(plan_of(&["codex"], &[("claude", "toolport")], true, true))
        );
    }

    #[test]
    fn a_toolport_skip_is_re_checked_and_falls_back_to_direct_only_when_disconnected() {
        let state = State {
            done: BTreeSet::new(),
            skipped: skipped(&[("claude", "toolport"), ("codex", "toolport")]),
            ..registered()
        };
        let auto = McpRegistration::Auto;
        for status in [
            served(),
            Detection::NotServed,
            Detection::Unknown("x".into()),
        ] {
            assert_eq!(plan_with(&state, "1.0.0", auto, |_| true, status), None);
        }
        assert_eq!(
            plan_with(&state, "1.0.0", auto, |_| true, Detection::NotConnected),
            Some(plan_of(&["claude", "codex"], &[], false, false))
        );
        // CLI が無ければ何もしない。
        assert_eq!(
            plan_with(&state, "1.0.0", auto, |_| false, Detection::NotConnected),
            None
        );
        // direct にすると、省いていたものを直接登録する。off は何もしない。
        assert_eq!(
            plan_with(&state, "1.0.0", McpRegistration::Direct, |_| true, served()),
            Some(plan_of(&["claude", "codex"], &[], false, false))
        );
        assert_eq!(
            plan_with(
                &state,
                "1.0.0",
                McpRegistration::Off,
                |_| true,
                Detection::NotConnected
            ),
            None
        );
    }

    #[test]
    fn mode_changes_never_remove_done_and_recheck_off_skips() {
        // 登録済み（done）は、どの方針・判定でも触れない。
        for mode in [
            McpRegistration::Auto,
            McpRegistration::Direct,
            McpRegistration::Off,
        ] {
            for status in [served(), Detection::NotConnected, Detection::NotServed] {
                assert_eq!(
                    plan_with(&registered(), "1.0.0", mode, |_| true, status),
                    None
                );
            }
        }
        let off_skipped = State {
            done: BTreeSet::new(),
            skipped: skipped(&[("claude", "off"), ("codex", "off")]),
            ..registered()
        };
        // auto に戻すと、新規扱いで判定する（提供中なら理由だけを toolport に書き換える）。
        assert_eq!(
            plan_with(
                &off_skipped,
                "1.0.0",
                McpRegistration::Auto,
                |_| true,
                served()
            ),
            Some(plan_of(
                &[],
                &[("claude", "toolport"), ("codex", "toolport")],
                false,
                false
            ))
        );
        assert_eq!(
            plan_with(
                &off_skipped,
                "1.0.0",
                McpRegistration::Auto,
                |_| true,
                Detection::NotServed
            ),
            Some(plan_of(&["claude", "codex"], &[], false, false))
        );
        assert_eq!(
            plan_with(
                &off_skipped,
                "1.0.0",
                McpRegistration::Auto,
                |_| false,
                Detection::NotServed
            ),
            None
        );
        // off のままなら何もしない。
        assert_eq!(
            plan_with(
                &off_skipped,
                "1.0.0",
                McpRegistration::Off,
                |_| true,
                served()
            ),
            None
        );
        // auto に戻したとき、CLI が無ければ提供中でも判定せず何もしない。
        assert_eq!(
            plan_with(
                &off_skipped,
                "1.0.0",
                McpRegistration::Auto,
                |_| false,
                served()
            ),
            None
        );
        // 前回登録できなかった CLI は、off では足さず、理由だけを off として残す（done は触れない）。
        let partial = done(&registered(), &["claude"]);
        assert_eq!(
            plan_with(
                &partial,
                "1.0.0",
                McpRegistration::Off,
                |_| true,
                Detection::NotConnected
            ),
            Some(plan_of(&[], &[("codex", "off")], false, false))
        );
        // 記録済みなら繰り返し記録しない。
        let recorded = State {
            skipped: skipped(&[("codex", "off")]),
            ..partial
        };
        assert_eq!(
            plan_with(
                &recorded,
                "1.0.0",
                McpRegistration::Off,
                |_| true,
                Detection::NotConnected
            ),
            None
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
            skipped: skipped(&[("claude", "toolport")]),
        };
        save(&dir, &state).unwrap();
        assert_eq!(load(&dir), state);
        let written: Value =
            serde_json::from_slice(&fs::read(dir.join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(written["skipped"], json!({"claude": "toolport"}));
        // 旧版の状態ファイル（skipped なし）は空として読め、未知の理由は捨てる。
        fs::write(
            dir.join(STATE_FILE),
            br#"{"executable":"x","version":"1","done":["claude"],"logon":true}"#,
        )
        .unwrap();
        assert!(load(&dir).skipped.is_empty());
        fs::write(
            dir.join(STATE_FILE),
            br#"{"executable":"x","skipped":{"claude":"toolport","codex":"weird"}}"#,
        )
        .unwrap();
        assert_eq!(load(&dir).skipped, skipped(&[("claude", "toolport")]));
        save(&dir, &state).unwrap();
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
