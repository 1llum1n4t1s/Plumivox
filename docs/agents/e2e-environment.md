# E2E の出力先と試験用環境変数

`cargo xtask test-e2e`の出力先と、試験用ビルドだけが読む環境変数の規約。

## 出力先

- 指定するときは次をそれぞれ別の、リポジトリとCodex・Claude Codeのユーザー設定ディレクトリの外のフォルダーにする。
  - `PLUMIVOX_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_TLS_E2E_OUTPUT_DIR`、`PLUMIVOX_PROXY_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_E2E_OUTPUT_DIR`
  - `PLUMIVOX_SETUP_E2E_OUTPUT_DIR`（PCセットアップ共通、全OS）、`PLUMIVOX_NATIVE_SETUP_E2E_OUTPUT_DIR`（macOSで追加）
  - `PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR`（梱包物・導入先・隔離環境を置く）
- 未指定なら各系統がユーザーの一時ディレクトリ配下の`Codex/Plumivox/<UUID>`を使う。
- 全件の`cargo xtask test-e2e`では`PLUMIVOX_SETUP_E2E_OUTPUT_DIR`を指定しない。`setup_e2e`の2テストが同じ出力先を並行して使い、空でない出力先は上書き防止で拒否されるため。
- 結果は各出力先の`summary.json`（失敗数と各結果）、API系の`http-trace.json`、共有中継の`ssh-trace.json`、VPS CLIの`mutation-trace.json`で確かめる。偽設定・画像・暗号化セッション・SSH鍵の清掃は呼び出し側で行う。

## 冷却ガードと診断記録

- 冷却ガード（`post_guard`）の時間は、試験用ビルド（`test-fixture`）だけが読む`PLUMIVOX_POST_GUARD_TEST`で縮める。整数は「1分の代わりにするms」、未設定・`off`は無効。本番ビルドは読まない。
- MCPの子プロセスは既定でこの変数と`PLUMIVOX_STATE_DIR`を外して起動する（`tests/support/mcp.rs`）。必要なケースだけが明示して渡す。
- 中継の診断記録は、試験用ビルドでは`PLUMIVOX_STATE_DIR`があるときだけ書く。
- 追加したE2Eのケースと偽APIの場面（`rejection-*`、`create502-stream`、`upload-hold`）の一覧は [検証の構成](../design/testing.md#冷却ガードと診断記録の検証) を参照する。
