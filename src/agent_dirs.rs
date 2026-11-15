//! AIエージェントのユーザー設定フォルダーとプラグインルートの判定。
//!
//! PlumivoxはCodex専用ではなく、Claude CodeなどのMCPホストからも起動される。
//! 秘密値を含む設定・セッション・SSH鍵を、ホスト側の設定フォルダーやプラグイン
//! キャッシュ（ホストの更新や同期で上書き・共有され得る場所）へ保存しないよう、
//! 各保存先の検査はここで定義した禁止先（ホストの設定フォルダー名・環境変数）を共通で使う。
//! ホストを追加するときは下の定数へ追記し、テスト側の同等リスト（tests/support/suite.rs など）も揃える。
//! プラグインルートの判定だけは用途で異なる：セットアップは実行ファイルの祖先から
//! いずれかのマニフェストを探す（executable_plugin_root）が、セッション保存先は
//! `dist/` の親に `mcp.json` と `plugin.json` が両方ある場合だけを対象にする
//! （session.rs の runtime_plugin_root）。拒否範囲を変えないため統一していない。
use std::path::{Path, PathBuf};

/// ホームディレクトリ直下にある各ホストの既定ユーザー設定フォルダー名。
/// session.rs ではパスの途中にこの名前の階層がある場合も拒否する。
pub(crate) const AGENT_DIR_NAMES: [&str; 2] = [".codex", ".claude"];

/// 各ホストのユーザー設定フォルダーを移動する環境変数。
/// Codex は `CODEX_HOME`、Claude Code は `CLAUDE_CONFIG_DIR`。
const AGENT_HOME_ENV: [&str; 2] = ["CODEX_HOME", "CLAUDE_CONFIG_DIR"];

/// プラグインルートを示すマニフェスト。各ホストのプラグイン形式と、
/// リポジトリ直下の共通 `plugin.json` を含める。
pub(crate) const PLUGIN_MANIFESTS: [&str; 3] = [
    ".codex-plugin/plugin.json",
    ".claude-plugin/plugin.json",
    "plugin.json",
];

/// 保存を禁止するホストのユーザー設定フォルダー一覧を返す。
/// 空の環境変数は未設定として扱う（空パスは全パスの接頭辞と判定されるため）。
pub(crate) fn agent_user_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = AGENT_DIR_NAMES.iter().map(|n| home.join(n)).collect();
    roots.extend(
        AGENT_HOME_ENV
            .iter()
            .filter_map(|name| std::env::var_os(name))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    );
    roots
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
/// 実行ファイルの親階層から、いずれかのマニフェストを持つ最初のフォルダーを返す。
pub(crate) fn executable_plugin_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .find(|ancestor| PLUGIN_MANIFESTS.iter().any(|m| ancestor.join(m).exists()))
        .map(Path::to_path_buf)
}

/// パスの途中にホストの既定設定フォルダー名を持つ階層があるか（大文字小文字は区別しない）。
pub(crate) fn has_agent_dir_component(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        AGENT_DIR_NAMES
            .iter()
            .any(|blocked| name.eq_ignore_ascii_case(blocked))
    })
}

/// Setup（Velopack）の導入先。更新で `current\` が入れ替わり、アンインストールで消えるため、
/// 秘密値を含む設定・セッション・SSH鍵の保存先として拒否する（プラグインルートと同じ扱い）。
///
/// - 実行中の実行ファイルが Velopack 管理下なら、その導入先（`get_root_dir()`。`--installto` で
///   既定以外の場所へ導入した場合も含む）。
/// - 本番の packId の既定の導入先（Windows `%LOCALAPPDATA%\PlumivoxApp`、macOS
///   `/Applications/Plumivox.app` と `~/Applications/Plumivox.app`）。プラグイン経路や開発ビルドから
///   セットアップした場合にも、後から Setup で導入される場所へ保存しないため、存在しなくても含める。
pub(crate) fn velopack_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(locator) = crate::app_update::locate() {
        roots.push(locator.get_root_dir());
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(local).join(crate::app_update::PACK_ID));
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/Applications/Plumivox.app"));
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join("Applications").join("Plumivox.app"));
        }
    }
    // 相対パスは全パスの接頭辞と誤判定され得るため除く。
    roots.retain(|root| root.is_absolute());
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velopack_install_roots_cover_the_default_setup_location() {
        let roots = velopack_install_roots();
        assert!(roots.iter().all(|root| root.is_absolute()));
        #[cfg(windows)]
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            assert!(roots.contains(&PathBuf::from(local).join("PlumivoxApp")));
            // 旧来の作業フォルダー（packId を変えた理由）は含めない。
            assert!(!roots.iter().any(|root| root.ends_with("Plumivox")));
        }
        #[cfg(target_os = "macos")]
        assert!(roots.contains(&PathBuf::from("/Applications/Plumivox.app")));
    }
}
