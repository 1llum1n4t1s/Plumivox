# Setup（Velopack）の規約

Windows の Setup（Velopack）の起動時処理・自動更新・登録・梱包・公開・検証に関する規約。DESIGN.md の [Setup（Velopack）](../../DESIGN.md#setupvelopack) も参照する。

packIdは`PlumivoxApp`（`Plumivox`は既存の`%LOCALAPPDATA%\Plumivox`と衝突する）、packTitleは`Plumivox`、mainExeは`plumivox.exe`、Velopack CLIと crate はともに1.3.39-g8d2844f（`velopack = "=1.3.39-g8d2844f"`）。フィードは`https://plumivox-updates.kagayoi.com/velopack/`の`releases.win.json`（将来`releases.osx.json`）。Plumivoxは**多プロセス型**（stdio MCP と共有中継 daemon）で、Velopack の適用は導入先の他プロセスを強制終了するため、日中に適用しない。

- `main()`は tokio のランタイムを作る前の同期処理で、全サブコマンドの最初に`VelopackApp`を自動適用**オフ**で動かす。install・updated・uninstall のフックは必ず終了コード0で終え、インストール・更新・アンインストールを止めない。フック内の CLI 呼び出しは1回3秒（`src/app_update/process.rs`の Job 付き時間制限）、フック以外（`--update-apply`・引数なしの起動・stdio の背景スレッド）は30秒。
- 登録は全OSで`setup-registration.json`（データフォルダー`%LOCALAPPDATA%\Kagayoi\Plumivox`の子`app-update\`）に記録する。フックで終わらなかった登録はログオン時の`--update-apply`・引数なしの起動・stdio 起動でやり直す。MCP の登録が消えると Plumivox が起動されないため、この再試行を外さない。
- Claude Code・Codex への MCP 登録、Run 値、旧プラグインの検知、Toolport 提供中の直接登録の省略は [mcp-registration.md](mcp-registration.md) に従う。
- 確認・取得は stdio MCP の背景スレッドだけが行い、適用しない（フック・`--proxy-daemon`・`--setup`では行わない）。標準出力へ何も書かず、診断は`app-update\app-update.log`へ。24時間の間引きと`app-update.lock`の排他で、複数の MCP が同時に起動しても取得は1回。管理外のビルド（開発・プラグインの`dist`）は何もしない。
- `--update-apply`（ログオン時）は保留が無ければすぐ終わり、導入先から他のプロセス（MCP・daemon）が1つでも動いていれば`busy`で適用しない（強制終了しない）。適用前に nupkg 内の全 PE の Authenticode 署名者 CN `Open Source Developer Yuichiro Shinozaki`（主実行ファイル`lib/app/plumivox.exe`と`Squirrel.exe`がちょうど1つずつ）を確かめ、再起動せずに適用する。
- `update-feed-test`機能（本番ビルド・署名梱包には入れない）だけが`PLUMIVOX_UPDATE_FEED_DIR`と`PLUMIVOX_UPDATE_ALLOW_UNSIGNED=1`を有効にする。`cargo xtask velopack-pack --sign`はテスト用の packId・版と併用できず、この機能付きのビルドを拒否する。
- セッション・設定・SSH鍵の保存先の禁止先に、Velopack の導入先（`agent_dirs::velopack_install_roots()`）を含める。プラグインルートの判定は変えない。

検証は隔離した環境とテスト用packIdで行い、本物の導入に触れない。手順は [testing.md](testing.md#setupvelopackの隔離検証) を参照する。

梱包・公開・CIの規約は [release-process.md](release-process.md#setupvelopackの梱包と公開) を参照する。公開・CI起動は明示の依頼があるときだけ行う。
