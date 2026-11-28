# 検証とテストの規約

Plumivox の検証コマンド、E2E、出力先、OS 別の確認、文書のみの変更の検証に関する規約。

開発には`rust-toolchain.toml`で固定したRust/Cargo 1.100.0を使う。利用者のMCP実行にはNode.js・Rustを要求しない。WindowsのDPAPI・ACL・ネイティブセットアップの検証にはWindows、VPSのRustインストーラーのOS境界検証にはLinux、Bash互換経路の構文・CLI確認にはBash（WindowsではGit for Windows）が必要。macOSのKeychain・Unix socket・ネイティブセットアップは実macOSで確認し、Windowsでの成功を代用しない。

```powershell
cargo test --locked --lib
cargo xtask build
cargo xtask check
cargo xtask test-e2e
```

- 実装・依存関係・ビルド設定を変更した場合は上記で検証し、Rustソースと再生成した対象OSの実行ファイルを一緒に更新する（プラグイン経路の`dist/`が残る間）。Setupの起動時処理・登録・更新・梱包を変更した場合は、Windowsで`cargo xtask test-setup-velopack-e2e`も実行する。macOSの正式配布物はARM64 Rustターゲットを導入して`cargo xtask build`で作り、arm64単独であることを確認する。`cargo xtask check`はCargoの形式・型検査、同梱Bashの構文、JSON設定を確認する。
- `cargo xtask test-e2e`はネイティブ本体を対象とするAPI・認証、管理プロキシ・共有daemon、PCセットアップ、VPSセットアップCLIの検証を行う。OS条件を理由に未実行の系統は明示し、全件成功へ数えない。全件検証ではAPI系のケース絞込み用`PLUMIVOX_E2E_CASE`を未設定にし、個別再実行の成功を全件成功として扱わない。ローカルの偽API・偽SSHと別プロセスのMCP・daemonを使い、実際のXへ投稿せず、実際のSSHホストへ接続しない。プロキシのRustテストも確認し、旧JavaScript実装だけの成功をネイティブ版の検証に代用しない。VPS CLIの確認だけで実Linuxへの導入成功を扱わない。失敗条件を先に確認し、挙動の検証は対応する既存E2Eへ追加する。
- 出力先を指定する場合は `PLUMIVOX_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_TLS_E2E_OUTPUT_DIR`、`PLUMIVOX_PROXY_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_E2E_OUTPUT_DIR`（PCセットアップ共通。全OSで使う）、`PLUMIVOX_NATIVE_SETUP_E2E_OUTPUT_DIR`（macOSで追加して使う）、`PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR`（Setup E2E。梱包物・導入先・隔離した環境を置く）をそれぞれ別の、リポジトリとCodex・Claude Codeのユーザー設定ディレクトリの外のフォルダーにする。未指定時は各系統がユーザーの一時ディレクトリ配下の `Codex/Plumivox/<UUID>` を使う。全件の`cargo xtask test-e2e`では`PLUMIVOX_SETUP_E2E_OUTPUT_DIR`を指定しない（`setup_e2e`の2テストが同じ出力先を並行して使い、空でない出力先は上書き防止で拒否されて失敗するため。未指定なら別々の一時フォルダーを使う）。各出力先の `summary.json` の失敗数と各結果、API系の`http-trace.json`、共有中継の`ssh-trace.json`、VPS CLIの`mutation-trace.json`を読み、偽設定・画像・暗号化セッション・SSH鍵の清掃は呼び出し側で行う。
- 冷却ガード（`post_guard`）の時間は、試験用ビルド（`test-fixture`）だけの環境変数`PLUMIVOX_POST_GUARD_TEST`で縮める（未設定・`off`は無効で既存のE2Eへ影響させない。整数は「1分の代わりにするms」）。本番ビルドは環境変数を読まない。MCPの子プロセスは既定でこの変数と`PLUMIVOX_STATE_DIR`を外して起動する（`tests/support/mcp.rs`）ので、必要なケースだけが環境で明示する。中継の診断記録は、試験用ビルドでは`PLUMIVOX_STATE_DIR`があるときだけ書く。追加したE2E（冷却ガード9ケース、数字コードの分類と画像の再試行、拒否記録の追加3ケース、中継の記録）と偽APIの場面（`rejection-`に続く`code226`・`code344`・`code344s`・`code88`・`codeunknown`・`xstatus`・`http400`・`http429`・`http400detail`・`http400big`、および`create502-stream`・`upload-hold`）は [検証の構成](../design/testing.md#冷却ガードと診断記録の検証) を参照する。
- 文書だけの変更では、リンク・コマンド・設定名・記載した挙動を対応する実装へ照合する。
- VPSインストーラー変更では`cargo test --locked --package plumivox-vps-installer`も実行し、Rust版とBash互換経路の検証を区別する。CLI・fixtureの成功を実システムへの導入成功と扱わない。
- R2関連の実装変更では`cargo test --locked --package xtask`も実行し、実R2通信とローカル検証を区別する。配布CIの検証は [release-process.md](release-process.md) を参照する。

## Setup（Velopack）の隔離検証

検証は本物の導入に触れずに行う: テスト用 packId`PlumivoxVelopackTest`（`PlumivoxApp`を使わない）、`--installto`は外部の作業フォルダー配下、子プロセスの`LOCALAPPDATA`・`APPDATA`・`USERPROFILE`・`HOME`・`CODEX_HOME`・`CLAUDE_CONFIG_DIR`・`TEMP`/`TMP`・`PLUMIVOX_SECRETS_FILE`をその配下へ、`PATH`はスタブ（`tests/cli-stub.rs`の`plumivox-cli-stub`）と System32 だけにする。`cargo xtask test-setup-velopack-e2e`がこれを行い、テスト用の Run 値・Uninstall の項目・導入先を本物のアンインストーラーで外して、本番の値と本物の`plumivox`登録の不変を確かめる。Run キーの単体テストは非公開の app hive（`RegLoadAppKeyW`）で行い、本物の Run キーを使わない。本物の Setup・`~/.claude`・`~/.codex`・Toolport の登録・HKCU Run には触れない。
