# Plumivox の作業規約

常に適用する規則だけを置く。主題ごとの詳細は `docs/agents/` に分けてある。該当する作業の前に読む。

## 常に守る規則

- Plumivoxは今後のリリースでもXへのリリース告知を行わない。`vava`の告知工程は非該当とし、告知用の画像生成・ログイン・投稿も行わない。
- `v<版>`タグの push は公開操作なので、`vava`（版の更新・公開の依頼）でだけ行う。R2の実操作・Setupの公開・CI起動は依頼に含まれる場合だけ行う。
- 標準出力はMCP通信専用。資格情報、復号Cookie、設定ファイルの内容、提供元の生エラーを応答や診断へ出さず、既定では保存しない（例外は、利用者がCLIで期限付きに有効にしたときだけ記録する、伏字処理した本人限定のローカルの拒否文面。伏字の規則・保存先・閲覧経路を変えるときはreviewerの審査とE2Eが必須。`docs/agents/security.md`参照）。資格情報は設定ストア経由で取得し、MCP引数に追加しない。
- 投稿作成・ログインを自動再試行しない。投稿IDが確認できない結果を成功にしない。本文の重複では止めず、止めるのは時刻に基づく冷却ガードだけで、MCPからは解除できない（利用者本人がCLIの`--post-guard`で確認・解除する。`docs/agents/security.md`参照）。
- 本物の Setup・`~/.claude`・`~/.codex`・Toolport の登録・HKCU Run には触れない。検証は隔離した環境で行う。
- 開発・検証にもNode.jsやnpmパッケージを要求しない。`xtask/`と`tests/`が起動する子プロセスは、Windowsでコンソール窓を出さない（`CREATE_NO_WINDOW`）。
- 人が読む文言は9言語の辞書（`src/locales/`）を同時に更新する。AIが読む文言は英語のままにする。`README.md`を変えたら各言語版も直す。

## 必須コマンド

開発にはRust/Cargo 1.100.0（`rust-toolchain.toml`で固定）を使う。実装・依存関係・ビルド設定を変更した場合は次で検証し、再生成した実行ファイルも一緒に更新する。

```powershell
cargo test --locked --lib
cargo xtask build
cargo xtask check
cargo xtask test-e2e
```

OS条件を理由に未実行の系統は明示し、全件成功へ数えない。

## 主題別の詳細

- 文書の置き場所・ソース構成・配布物・プラグイン登録を変えるときは [architecture.md](docs/agents/architecture.md) を読む。設計は [DESIGN.md](DESIGN.md) を参照する。
- 検証・E2E・出力先・OS別の確認は [testing.md](docs/agents/testing.md) を読む。
- E2Eの出力先・試験用環境変数を変えるときは [e2e-environment.md](docs/agents/e2e-environment.md) を読む。
- リリース・配布・R2・CIは [release-process.md](docs/agents/release-process.md) を読む。
- Setup（Velopack）の起動時処理・更新・登録・梱包を変えるときは [setup-velopack.md](docs/agents/setup-velopack.md) を読む（[設計](DESIGN.md#setupvelopack)）。
- 資格情報・保存先・Cookie・投稿の再試行・冷却ガードを変えるときは [security.md](docs/agents/security.md) を読む（[データフロー](DESIGN.md#データフロー)、[不変条件](DESIGN.md#不変条件と採用した設計判断)）。
- アカウント選択を変えるときは [configuration.md](docs/agents/configuration.md) を読む。
- VPSインストーラー・`setup/vps/`を変えるときは [vps-installer-constraints.md](docs/agents/vps-installer-constraints.md) を読む（[設計](DESIGN.md#初期設定と公開中継の構築)）。
- 管理プロキシ・共有daemonを変えるときは [managed-proxy-constraints.md](docs/agents/managed-proxy-constraints.md) を読む（[設計](DESIGN.md#管理するプロキシの境界)）。
- MCPサーバーの登録を変えるときは [mcp-registration.md](docs/agents/mcp-registration.md) を読む。
- フィード・検索を変えるときは [api-reference.md](docs/agents/api-reference.md) を読む。
- ローカライズ・READMEの多言語同期は [conventions.md](docs/agents/conventions.md) を読む（[設計](DESIGN.md#ローカライズ)）。
