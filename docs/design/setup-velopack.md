# Setup（Velopack）

[DESIGN.md](../../DESIGN.md#setupvelopack) の詳細。実装は [`src/app_update.rs`](../../src/app_update.rs) と`src/app_update/`、梱包・公開は`xtask/src/velopack.rs`。

本人の決定（2026-11-15）により、Windowsの既定の配布をSetup（Velopack）へ移した。共通仕様は`D:\Dev\APP-DISTRIBUTION.md`の多プロセス型、参考実装はRemnifold。packIdは`PlumivoxApp`、導入先は既定で`%LOCALAPPDATA%\PlumivoxApp`（`current\plumivox.exe`）、ユーザー単位で管理者権限を要求しない。

- **起動時処理**：`main`は全サブコマンドの最初に、tokioのスレッドを作る前の同期処理で`VelopackApp`を自動適用オフで実行する（Velopackが環境変数を書き換えるため）。フック引数はここで処理して終了コード0で終える。
- **多プロセス型の適用**：Velopackの適用は導入先の他プロセスを強制終了し、Plumivoxはstdio MCPと共有中継daemonが導入先から動くため、日中には適用しない。確認・取得はstdio MCPの背景スレッドだけが行い（24時間の間引き、失敗も24時間は再試行しない、`app-update.lock`で重複取得を防ぐ）、適用はログオン時の`--update-apply`だけが行う。他のプロセスが動いていれば`busy`として次のログオンへ回す。
- **信頼**：取得はHTTPSとフィードのハッシュ照合で守り、適用前にnupkg内の全PEのAuthenticode署名と署名者CN（`Open Source Developer Yuichiro Shinozaki`）を確かめる。主実行ファイルと`Squirrel.exe`はちょうど1つずつ。失効確認はしない（オフラインのログオン時でも適用するため）。
- **登録**：インストールフックとフック以外の起動で、Claude Code・Codexへ公式CLIで登録する。フックはCLI 1回を3秒で打ち切り（Velopackはフックを15～30秒で打ち切る）、未完了は`setup-registration.json`に残して30秒の上限でやり直す。更新後はパスが同じならRun値だけを張り直す。Toolportが自分と同じ実行ファイルで既に提供中なら直接登録を省く。別の実行ファイルで提供中でも直接登録は省き、`toolport-foreign`を記録して警告する（起動のたびに判定し直す）。判定はOSによらず（Windows・macOS・Linux）、Setupの配布は今はWindowsだけ。規約と判定仕様は [mcp-registration.md](../agents/mcp-registration.md)。
- **状態の置き場**：設定・時刻・ロック・ログ・登録状態はデータフォルダーの子`app-update\`に、本人限定の権限で置く。導入先の`current\`は更新で入れ替わるため使わない。
- **保存先の保護**：セッション・設定・SSH鍵の禁止先にVelopackの導入先（`get_root_dir()`と既定の導入先）を加えた。
- **解除**：アンインストールフックと`--uninstall-setup --apply`は自分を指す登録とRun値だけを外し、データフォルダーを残す。macOSはフックが無いため起動時に登録し、`--uninstall-setup --apply`で解除する（`osx`チャネルは未配信）。
- **案内**：管理下のビルドを端末から引数なしで起動した場合と初回起動は、登録済みであることと未設定なら`--setup`を案内する。`--setup`は自動で始めない。MCPクライアントからはstdioで動く。
- **配信**：`distribution.yml`の`velopack` jobが`windows` jobの後に梱包・署名・検証し、公開R2の`velopack/`へ置く（`releases.win.json`は最後）。詳細は [release-process.md](release-process.md)。
- **範囲外**：Linux、プラグインの縮小、Toolport設定の書き換え。
