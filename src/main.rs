//! Plumivox の実行ファイルの入口。
//!
//! Setup（Velopack）導入版のために、tokio のランタイムを作る前の同期の main で
//! [`plumivox::app_update::startup`] を全サブコマンドの最初に呼ぶ（フック引数 `--veloapp-*` の処理と、
//! 起動時の自動適用の停止。Velopack は環境変数を書き換えるため、スレッドを作る前に行う）。
//! 以前の `#[tokio::main]` では `--veloapp-*` が使い方の誤りとして終了コード 1 になっていた。
use std::io::{IsTerminal, Read};

/// 引数の解析結果。分岐を純関数にして単体テストで確かめる。
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// 引数なし。管理下ビルドの端末起動・初回起動なら案内、それ以外は stdio MCP。
    Default,
    /// `--stdio`。MCP クライアント（Claude Code・Codex）からの起動。
    Stdio,
    /// `--setup ...`。対話式セットアップ。
    Setup(Vec<String>),
    /// `--version`。
    Version,
    /// `--proxy-daemon`。MCP が起動する共有中継 daemon。
    ProxyDaemon,
    /// `--update-apply`。ログオン時に保留中の更新を、他のプロセスが無いときだけ適用する。
    UpdateApply,
    /// `--uninstall-setup [--apply]`。Setup 版の登録の解除。
    UninstallSetup(Vec<String>),
    /// `--rejection-log <on [--days N]|off|status|show|clear>`。拒否文面のローカル記録の操作。
    RejectionLog(Vec<String>),
    /// `--help` / `-h`。使い方の表示（利用者の言語）。
    Help,
    /// 不明な引数。
    Usage,
}

fn parse(args: &[String]) -> Mode {
    match args {
        [] => Mode::Default,
        [arg, rest @ ..] if arg == "--setup" => Mode::Setup(rest.to_vec()),
        [arg, rest @ ..] if arg == "--uninstall-setup" => Mode::UninstallSetup(rest.to_vec()),
        [arg, rest @ ..] if arg == "--rejection-log" => Mode::RejectionLog(rest.to_vec()),
        [arg] if arg == "--stdio" => Mode::Stdio,
        [arg] if arg == "--version" => Mode::Version,
        [arg] if arg == "--proxy-daemon" => Mode::ProxyDaemon,
        [arg] if arg == "--update-apply" => Mode::UpdateApply,
        [arg] if arg == "--help" || arg == "-h" => Mode::Help,
        _ => Mode::Usage,
    }
}

/// 引数なしの起動で案内を出すか。MCP クライアントは stdin をパイプで渡すため、端末から人が起動した
/// 場合（スタートメニュー・ターミナル）と Setup 直後の初回起動だけを案内にする。管理外のビルド
/// （開発ビルド・プラグインの dist）の素の起動は、従来どおり stdio MCP として動く。
fn wants_welcome(managed: bool, first_run: bool, stdin_is_terminal: bool) -> bool {
    managed && (first_run || stdin_is_terminal)
}

/// tokio のマルチスレッドランタイムで非同期処理を動かす（以前の `#[tokio::main]` と同じ構成）。
fn block_on<F: std::future::Future>(future: F) -> Option<F::Output> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .ok()
        .map(|runtime| runtime.block_on(future))
}

fn stdio() -> bool {
    // 拒否記録の古いものを背景で消す（`diagnostics\` があるときだけ。起動を待たせず、フォルダーも作らない）。
    plumivox::rejection_log::spawn_startup_prune();
    block_on(plumivox::run_stdio(plumivox::Options::default())).is_some_and(|result| result.is_ok())
}

/// 版の表示。`update-feed-test` のビルドに限り、ビルド時の `PLUMIVOX_TEST_PACK_VERSION` を表示して、
/// 隔離E2Eで「更新後に別の版の実行ファイルへ入れ替わったこと」を確かめられるようにする（本番は Cargo の版）。
fn version() -> &'static str {
    #[cfg(feature = "update-feed-test")]
    if let Some(version) = option_env!("PLUMIVOX_TEST_PACK_VERSION") {
        return version;
    }
    env!("CARGO_PKG_VERSION")
}

fn main() {
    // Velopack の処理（フック引数・初回起動の判定）は、引数の解析より前に全サブコマンド共通で行う。
    // 起動時の自動適用は切ってある（適用は --update-apply だけ）。
    let startup = plumivox::app_update::startup();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 終了コードは、成功 0、保存先の異常 1、引数の誤り 2（他のサブコマンドは失敗を 1 にする）。
    if let Mode::RejectionLog(rest) = parse(&args) {
        std::process::exit(plumivox::rejection_log::run_cli(&rest));
    }
    let success = match parse(&args) {
        Mode::Default => {
            // macOS の Setup 版には Velopack のインストールフックが無い。.pkg は導入後に引数なしで1回起動する
            // ので、ここで Claude Code・Codex とログオン時の適用を登録する。Windows では、インストールフックの
            // 3秒の打ち切りで終わらなかった登録をここでもやり直す（登録済みなら状態ファイルを読むだけで終わる。
            // 管理外のビルドでは何もしない）。
            plumivox::app_update::ensure_registered();
            if wants_welcome(
                plumivox::app_update::is_managed(),
                startup.first_run,
                std::io::stdin().is_terminal(),
            ) {
                plumivox::app_update::welcome(startup.first_run);
                true
            } else {
                stdio()
            }
        }
        Mode::Stdio => stdio(),
        Mode::Setup(rest) => match block_on(plumivox::setup::run(&rest)) {
            Some(Ok(())) => true,
            Some(Err(error)) => {
                eprintln!("{}: {}", error.code, error.message);
                false
            }
            None => false,
        },
        Mode::Version => {
            println!("plumivox {}", version());
            true
        }
        Mode::ProxyDaemon => {
            // 専用の子プロセスだけで設定し、Unix socketを生成時から本人専用にする。
            #[cfg(unix)]
            unsafe {
                libc::umask(0o077);
            }
            let mut bytes = Vec::new();
            let input = std::io::stdin().take(8193).read_to_end(&mut bytes);
            if input.is_err() || bytes.len() > 8192 {
                false
            } else if let Ok(bootstrap) = serde_json::from_slice(&bytes) {
                block_on(plumivox::proxy::run_daemon(bootstrap))
                    .is_some_and(|result| result.is_ok())
            } else {
                false
            }
        }
        // ログオン時に起動される。適用しなかった理由（実行中のプロセスなど）は終了コード 0 の通常の結果として
        // 返し、署名者の検証に通らなかった場合（2）と実行の失敗（1）だけを非 0 にする。
        Mode::UpdateApply => match plumivox::app_update::update_apply() {
            Ok(outcome) => {
                println!("{}", outcome.to_json());
                if matches!(outcome, plumivox::app_update::ApplyOutcome::Untrusted(_)) {
                    std::process::exit(2);
                }
                true
            }
            Err(error) => {
                eprintln!("UPDATE_APPLY_FAILED: {error}");
                false
            }
        },
        Mode::UninstallSetup(rest) => match plumivox::app_update::uninstall_setup(&rest) {
            Ok(result) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                );
                true
            }
            Err(error) => {
                eprintln!("UNINSTALL_SETUP_FAILED: {error}");
                false
            }
        },
        // 使い方・ヘルプは人が読むので、利用者の言語の辞書から引く（stdio MCP の経路ではない）。
        Mode::Help => {
            println!(
                "{}",
                plumivox::locale::tr_with("cli.help", &[("version", version())])
            );
            true
        }
        // 上で処理して終了済み。
        Mode::RejectionLog(_) => true,
        Mode::Usage => {
            eprintln!("{}", plumivox::locale::tr("cli.usage"));
            false
        }
    };
    if !success {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn arguments_select_each_mode_and_reject_extras() {
        assert_eq!(parse(&[]), Mode::Default);
        assert_eq!(parse(&args(&["--stdio"])), Mode::Stdio);
        assert_eq!(parse(&args(&["--version"])), Mode::Version);
        assert_eq!(parse(&args(&["--proxy-daemon"])), Mode::ProxyDaemon);
        assert_eq!(parse(&args(&["--update-apply"])), Mode::UpdateApply);
        assert_eq!(parse(&args(&["--help"])), Mode::Help);
        assert_eq!(parse(&args(&["-h"])), Mode::Help);
        assert_eq!(
            parse(&args(&["--setup", "--check"])),
            Mode::Setup(args(&["--check"]))
        );
        assert_eq!(
            parse(&args(&["--uninstall-setup", "--apply"])),
            Mode::UninstallSetup(args(&["--apply"]))
        );
        assert_eq!(
            parse(&args(&["--uninstall-setup"])),
            Mode::UninstallSetup(vec![])
        );
        for bad in [
            &["--stdio", "x"][..],
            &["--update-apply", "--now"],
            &["--version", "--stdio"],
            &["--help", "--stdio"],
            &["mcp"],
            // Velopack のフック引数は startup() が先に処理して終了するため、ここまで来たら不明な引数。
            &["--veloapp-install", "1.0.0"],
        ] {
            assert_eq!(parse(&args(bad)), Mode::Usage, "{bad:?}");
        }
    }

    #[test]
    fn welcome_is_only_for_managed_builds_started_by_a_person() {
        // MCP クライアント（stdin がパイプ）からの起動は、管理下でも stdio MCP のまま。
        assert!(!wants_welcome(true, false, false));
        assert!(wants_welcome(true, false, true));
        assert!(wants_welcome(true, true, false));
        // 開発ビルド・プラグインの dist は、端末から起動しても従来どおり stdio MCP。
        assert!(!wants_welcome(false, false, true));
        assert!(!wants_welcome(false, true, true));
    }
}
