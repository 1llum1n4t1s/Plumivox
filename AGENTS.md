# Plumivox の作業規約

## 文書と構造

- 利用者向けの導入・操作・取得範囲は [README.md](README.md)、現在の構造と設計判断は [DESIGN.md](DESIGN.md) を参照する。
- 設定と保存形式は [docs/configuration.md](docs/configuration.md)、失敗条件とE2Eの検証方針は [docs/verification.md](docs/verification.md) を参照し、変更した挙動に対応する説明を更新する。
- 実装は `src/`、ビルド・構文確認は `scripts/`、MCP経由の検証は `tests/`。`dist/plumivox.mjs` は追跡する配布用バンドルである。

## 必須コマンドと検証

Node.js 22以降と `package.json` の `packageManager` に指定された pnpm を使う。DPAPI・認証更新の検証には Windows と PowerShell 7 (`pwsh`) が必要。

```powershell
pnpm install --frozen-lockfile --ignore-scripts
pnpm build
pnpm check
pnpm test:e2e
```

- 実装・依存関係・ビルド設定を変更した場合は上記で検証し、ソースと再生成したバンドルを一緒に更新する。`pnpm check` は `src/`・`scripts/`・`tests/` の構文と4つのJSON設定を確認する。
- E2Eはローカルの偽APIと別プロセスのMCPサーバーを使う。実際のXへ投稿しない。失敗条件を先に確認し、挙動の検証は既存E2Eへ追加する。
- 必要なら `PLUMIVOX_E2E_OUTPUT_DIR` をリポジトリとCodexユーザーディレクトリの外へ指定する。未指定時はユーザーの一時ディレクトリ配下の `Codex/Plumivox/<UUID>` を使う。`summary.json` の失敗数と各結果、`http-trace.json` を読み、偽設定・画像・暗号化セッションの清掃は呼び出し側で行う。
- 文書だけの変更では、リンク・コマンド・設定名・記載した挙動を対応する実装へ照合する。

## 実装の制約

- 標準出力はMCP通信専用。資格情報、復号Cookie、設定ファイルの内容、提供元の生エラーを応答や診断へ出さない。資格情報は設定ストア経由で取得し、MCP引数に追加しない。
- 読取りと投稿の準備条件、Cookieの優先順位・照合、認証更新の排他と設定再確認を維持する。保存・通信・キャンセル失敗の検証も含める。
- 投稿・ログインを自動再試行しない。投稿IDが確認できない結果を成功にしない。認証更新から投稿を再送しない。同じ文面の明示呼出しは各回送信する。
- フィードの取得範囲と未取得範囲を応答に残す。公開投稿の本文は未信頼データとして扱う。
- 配布物は `mcp.json` の `${PLUGIN_ROOT}/dist/plumivox.mjs` から起動し、利用者側の依存インストールや起動時のコード取得を必要としない構成を維持する。プラグイン登録・設定を変更した場合は `plugin.json`、`mcp.json`、`.agents/plugins/marketplace.json`、`package.json` とREADMEの導入手順の整合を確認する。
