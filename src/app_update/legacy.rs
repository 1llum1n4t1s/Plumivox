//! 旧経路（Claude Code・Codex の Plumivox プラグイン）が有効なままかを、読み取りだけで検知する。
//!
//! # なぜ必要か
//! Setup 版は `claude mcp add` / `codex mcp add` で名前 `plumivox` を登録する。しかしプラグインが持つ MCP
//! （プラグイン名は `plumivox@<マーケットプレイス名>`。通常は `plumivox@plumivox-local`）は別の登録なので、
//! プラグインを残したままだと Plumivox が2経路で起動し得る。`mcp get plumivox` はプラグイン由来の MCP を
//! 名前だけで返し、ユーザー登録と同名のときはユーザー登録が優先されて見えなくなる（下記の調査）ため、
//! CLI の出力からは旧プラグインの有効・無効を判別できない。そこで利用者の設定ファイルを直接読む。
//!
//! # Codex の挙動（openai/codex の main、リリース rust-v0.169.0 時点のソースで確認。実機では未確認）
//! - 無効化の公式キー: `[plugins."<plugin>@<marketplace>".mcp_servers.<server>] enabled = false`
//!   （`codex-rs/config/src/types.rs` の `PluginMcpServerConfig`、適用は `codex-rs/core-plugins/src/loader.rs`
//!   の `apply_plugin_mcp_server_policy`）。プラグイン自体の `[plugins."<plugin>@<marketplace>"] enabled = false`
//!   も有効。
//! - `codex mcp get/list` はプラグイン由来の MCP も返す（名前は `plumivox`。`plumivox@...` ではない）。
//!   `codex-rs/cli/src/mcp_cmd.rs` の `run_get` / `run_list` が `configured_servers` を使う。
//! - 同名なら優先順位 `Plugin < Config` でユーザー設定（`codex mcp add` の登録）が勝ち、プラグイン側は
//!   捨てられる（`codex-rs/codex-mcp/src/catalog.rs` の `RegistrationPrecedence`）。つまり Codex では Setup 版の
//!   登録がある間は二重起動にならないが、登録が外れた・失敗した途端に旧 `dist` が復活する。
//! - Claude Code のプラグイン MCP はユーザー登録とは別の名前で起動するため、2つ起動し得る
//!   （この点は Claude Code の公開仕様に基づく説明で、実機では未確認）。
//!
//! # 方針
//! 検知して案内するだけで、利用者の設定は書き換えない（`enabled = false` の自動書き込みは契約変更になる）。
//! 設定ファイルが無い・読めない・形式が想定外のときは「検知なし」として扱い、決して失敗しない。
//! 秘密値を含み得るファイルなので、中身は保持・記録せず、判定結果（ツール名とプラグイン名）だけを返す。
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// 旧プラグインの名前（`<name>@<マーケットプレイス>` の前半）。
const PLUGIN_NAME: &str = "plumivox";
/// プラグインが持つ MCP サーバーの名前（`mcp.json` の `mcpServers` のキー）。
const SERVER_NAME: &str = "plumivox";
/// 読み込む設定ファイルの上限。これを超えるものは想定外として読まない。
const FILE_LIMIT: u64 = 2 * 1024 * 1024;

/// 検知結果の1件。状態ファイルには `"<tool>:<plugin>"` の文字列で残す。
pub fn entry(tool: &str, plugin: &str) -> String {
    format!("{tool}:{plugin}")
}

/// 検知結果（`"codex:plumivox@plumivox-local"` 形式）から、人に見せる CLI 名の一覧を作る（重複なし・固定順）。
pub fn tool_labels(detected: &BTreeSet<String>) -> Vec<&'static str> {
    let mut labels = Vec::new();
    for (prefix, label) in [("claude:", "Claude Code"), ("codex:", "Codex")] {
        if detected.iter().any(|item| item.starts_with(prefix)) {
            labels.push(label);
        }
    }
    labels
}

/// 小さなファイルを読む（上限超過・通常ファイル以外・読めない場合は None）。リンクは辿らない。
fn read_small(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(FILE_LIMIT)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// Codex の設定ディレクトリ（`CODEX_HOME`、無ければホームの `.codex`）。
fn codex_home(home: Option<&Path>) -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME") {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => home.map(|home| home.join(".codex")),
    }
}

/// Claude Code の設定ディレクトリ（`CLAUDE_CONFIG_DIR`、無ければホームの `.claude`）。
fn claude_dir(home: Option<&Path>) -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => home.map(|home| home.join(".claude")),
    }
}

/// 今の利用者の設定から、有効なままの旧プラグインを検知する（読み取りだけ。2つの小さなファイルを読む）。
pub fn detect() -> BTreeSet<String> {
    let home = dirs::home_dir();
    detect_in(
        codex_home(home.as_deref()).as_deref(),
        claude_dir(home.as_deref()).as_deref(),
    )
}

/// 設定ディレクトリを指定して検知する（テスト用に分けてある）。
pub fn detect_in(codex_dir: Option<&Path>, claude_dir: Option<&Path>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if let Some(bytes) = codex_dir.and_then(|dir| read_small(&dir.join("config.toml")))
        && let Ok(text) = std::str::from_utf8(&bytes)
    {
        for plugin in codex_active_plugins(text) {
            found.insert(entry("codex", &plugin));
        }
    }
    if let Some(bytes) = claude_dir.and_then(|dir| read_small(&dir.join("settings.json"))) {
        for plugin in claude_enabled_plugins(&bytes) {
            found.insert(entry("claude", &plugin));
        }
    }
    found
}

/// Claude Code の `settings.json` の `enabledPlugins` から、有効な `plumivox@...` を返す。
/// 壊れた JSON・想定外の形は「検知なし」。
pub fn claude_enabled_plugins(bytes: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let Some(map) = value.get("enabledPlugins").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    map.iter()
        .filter(|(name, enabled)| is_plumivox_plugin(name) && enabled.as_bool() == Some(true))
        .map(|(name, _)| name.clone())
        .collect()
}

fn is_plumivox_plugin(name: &str) -> bool {
    name.strip_prefix(PLUGIN_NAME)
        .is_some_and(|rest| rest.starts_with('@'))
}

/// Codex の `config.toml` から、有効なままの `plumivox@...` プラグインを返す。
///
/// 次のどれかなら「無効」とみなして返さない:
/// - `[plugins."plumivox@..."]` の `enabled = false`（プラグイン自体の無効化）
/// - `[plugins."plumivox@...".mcp_servers.plumivox]` の `enabled = false`（プラグインの MCP だけの無効化）
///
/// 本物の TOML パーサーを使わず（依存を増やさないため）、テーブル見出しと `キー = 値` の行だけを読む。
/// 見出しは `[a."b".c]`、キーはドット区切り・引用符付きに対応する。インライン形式
/// （`plugins = { ... }`）や複数行の値は読まないので、そこに書かれた無効化は検知できない
/// （その場合は有効として扱い、警告が出る側に倒す）。
pub fn codex_active_plugins(text: &str) -> Vec<String> {
    #[derive(Default)]
    struct Seen {
        plugin_disabled: bool,
        mcp_disabled: bool,
    }
    let mut plugins: std::collections::BTreeMap<String, Seen> = Default::default();
    // 今いるテーブルの見出し（キーの並び）。
    let mut table: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            // `[[配列テーブル]]` は対象外。見出しとして読まず、以後のキーも無関係なテーブルとして扱う。
            if header.starts_with('[') {
                table = vec!["[[array]]".into()];
                continue;
            }
            // 見出しの後ろのコメントを除いてから、閉じ括弧までを読む。
            table = split_header(header).unwrap_or_else(|| vec!["[invalid]".into()]);
            note_seen(&table, &mut |plugin| {
                plugins.entry(plugin).or_default();
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(key_path) = parse_key_path(key.trim()) else {
            continue;
        };
        let mut full = table.clone();
        full.extend(key_path);
        note_seen(&full, &mut |plugin| {
            plugins.entry(plugin).or_default();
        });
        let is_false = strip_comment(value).trim() == "false";
        if is_false && let Some(plugin) = plugin_of(&full) {
            let entry = plugins.entry(plugin.to_string()).or_default();
            match &full[2..] {
                [leaf] if leaf == "enabled" => entry.plugin_disabled = true,
                [group, server, leaf]
                    if group == "mcp_servers" && server == SERVER_NAME && leaf == "enabled" =>
                {
                    entry.mcp_disabled = true
                }
                _ => {}
            }
        }
    }
    plugins
        .into_iter()
        .filter(|(_, seen)| !seen.plugin_disabled && !seen.mcp_disabled)
        .map(|(name, _)| name)
        .collect()
}

/// キーの並びが `plugins` / `<plumivox@...>` で始まるなら、そのプラグイン名。
fn plugin_of(path: &[String]) -> Option<&str> {
    match path {
        [first, plugin, ..] if first == "plugins" && is_plumivox_plugin(plugin) => Some(plugin),
        _ => None,
    }
}

/// キーの並びがプラグインの設定下にあれば、そのプラグインを「見つかった」として記録する。
fn note_seen(path: &[String], record: &mut dyn FnMut(String)) {
    if let Some(plugin) = plugin_of(path) {
        record(plugin.to_string());
    }
}

/// `]` までを読んで見出しのキーの並びにする（`header` は先頭の `[` を除いた残り）。
fn split_header(header: &str) -> Option<Vec<String>> {
    // 引用符の中の `]` を閉じ括弧と取り違えないよう、引用符の外の最初の `]` を探す。
    let mut quote: Option<char> = None;
    let mut end = None;
    for (index, ch) in header.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, ']') => {
                end = Some(index);
                break;
            }
            _ => {}
        }
    }
    parse_key_path(header[..end?].trim())
}

/// 行末のコメント（引用符の外の `#` 以降）を除く。
fn strip_comment(text: &str) -> &str {
    let mut quote: Option<char> = None;
    for (index, ch) in text.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '#') => return &text[..index],
            _ => {}
        }
    }
    text
}

/// `a."b.c".d` のようなドット区切りのキーを並びにする。裸のキー・基本文字列・リテラル文字列に対応し、
/// 基本文字列のエスケープは扱わない（プラグイン名・サーバー名には現れないため）。解釈できなければ None。
fn parse_key_path(text: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut part = String::new();
        match chars.peek().copied() {
            Some(q @ ('"' | '\'')) => {
                chars.next();
                loop {
                    match chars.next()? {
                        c if c == q => break,
                        c => part.push(c),
                    }
                }
            }
            Some(_) => {
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        part.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if part.is_empty() {
                    return None;
                }
            }
            None => return None,
        }
        parts.push(part);
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        match chars.next() {
            None => return Some(parts),
            Some('.') => continue,
            Some(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLUGIN: &str = "plumivox@plumivox-local";

    #[test]
    fn an_enabled_codex_plugin_is_detected() {
        let text = format!("model = \"x\"\n\n[plugins.\"{PLUGIN}\"]\nenabled = true\n");
        assert_eq!(codex_active_plugins(&text), [PLUGIN]);
        // `enabled` を書かない（既定は有効）場合も検知する。
        let text = format!("[plugins.\"{PLUGIN}\"]\n");
        assert_eq!(codex_active_plugins(&text), [PLUGIN]);
    }

    #[test]
    fn mcp_only_disable_and_plugin_disable_are_honored() {
        // README が案内する、プラグインの MCP だけを無効にする書き方。
        let text = format!(
            "[plugins.\"{PLUGIN}\"]\nenabled = true\n\n[plugins.\"{PLUGIN}\".mcp_servers.plumivox]\nenabled = false # 二重起動を避ける\n"
        );
        assert!(codex_active_plugins(&text).is_empty());
        // プラグイン自体の無効化。
        let text = format!("[plugins.\"{PLUGIN}\"]\nenabled = false\n");
        assert!(codex_active_plugins(&text).is_empty());
        // ドット区切りのキーでも同じ。
        let text = format!(
            "[plugins.\"{PLUGIN}\"]\nenabled = true\nmcp_servers.plumivox.enabled = false\n"
        );
        assert!(codex_active_plugins(&text).is_empty());
        // 別のサーバー名の無効化は旧 MCP を止めない。
        let text = format!("[plugins.\"{PLUGIN}\".mcp_servers.other]\nenabled = false\n");
        assert_eq!(codex_active_plugins(&text), [PLUGIN]);
    }

    #[test]
    fn unrelated_settings_are_not_detected() {
        let text = "[mcp_servers.plumivox]\ncommand = \"x\"\n\n[plugins.\"other@market\"]\nenabled = true\n[[hooks]]\nenabled = false\n";
        assert!(codex_active_plugins(text).is_empty());
        assert!(codex_active_plugins("").is_empty());
        assert!(codex_active_plugins("[[[ broken").is_empty());
    }

    #[test]
    fn claude_plugins_follow_enabled_plugins() {
        let json = format!(
            "{{\"enabledPlugins\":{{\"{PLUGIN}\":true,\"plumivox@other\":false,\"x@y\":true}}}}"
        );
        assert_eq!(claude_enabled_plugins(json.as_bytes()), [PLUGIN]);
        assert!(claude_enabled_plugins(b"{ broken").is_empty());
        assert!(claude_enabled_plugins(b"{}").is_empty());
    }

    #[test]
    fn the_warning_names_the_tools_in_every_language() {
        // 辞書の整合は locale のテストが見る。ここでは、検知した CLI 名が文言へ入ることだけを確かめる。
        let text =
            crate::locale::tr_with("welcome.legacy_plugin", &[("tools", "Codex, Claude Code")]);
        assert!(text.contains("Codex, Claude Code"));
        assert!(!text.contains("{tools}"));
    }

    #[test]
    fn detect_in_reads_both_config_files_and_ignores_missing_ones() {
        let dir = super::super::tests::scratch("legacy");
        let (codex, claude) = (dir.join("codex"), dir.join("claude"));
        fs::create_dir_all(&codex).unwrap();
        fs::create_dir_all(&claude).unwrap();
        // 何も無い。
        assert!(detect_in(Some(&codex), Some(&claude)).is_empty());
        assert!(detect_in(None, None).is_empty());
        fs::write(
            codex.join("config.toml"),
            format!("[plugins.\"{PLUGIN}\"]\nenabled = true\n"),
        )
        .unwrap();
        fs::write(
            claude.join("settings.json"),
            format!("{{\"enabledPlugins\":{{\"{PLUGIN}\":true}}}}"),
        )
        .unwrap();
        let found = detect_in(Some(&codex), Some(&claude));
        assert_eq!(
            found,
            BTreeSet::from([entry("codex", PLUGIN), entry("claude", PLUGIN)])
        );
        assert_eq!(tool_labels(&found), ["Claude Code", "Codex"]);
        let _ = fs::remove_dir_all(dir);
    }
}
