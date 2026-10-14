# Plumivox の作業規約

## リリース

- Plumivoxは今後のリリースでもXへのリリース告知を行わない。`vava`の告知工程は非該当とし、告知用の画像生成・ログイン・投稿も行わない。
- 配布はGitHubの最新`main`を参照する既存プラグイン経路を使う。

## 文書と構造

- 利用者向けの導入・操作・取得範囲は [README.md](README.md)、現在の構造と設計判断は [DESIGN.md](DESIGN.md) を参照する。
- 初期設定は [docs/setup.md](docs/setup.md)、設定と保存形式は [docs/configuration.md](docs/configuration.md)、管理プロキシの設定・運用は [docs/managed-proxy.md](docs/managed-proxy.md)、失敗条件とE2Eの検証方針は [docs/verification.md](docs/verification.md) を参照し、変更した挙動に対応する説明を更新する。
- 実装は `src/`、ビルド・構文確認は `scripts/`、MCP経由の検証は `tests/`。`dist/plumivox.mjs` は追跡する配布用バンドルである。

## 必須コマンドと検証

Node.js 22以降と `package.json` の `packageManager` に指定された pnpm を使う。DPAPI・認証更新とPCセットアップの検証には Windows と PowerShell 7 (`pwsh`)、VPSセットアップの構文・CLI確認には Bash（WindowsではGit for Windows）が必要。

```powershell
pnpm install --frozen-lockfile --ignore-scripts
pnpm build
pnpm check
pnpm test:e2e
```

- 実装・依存関係・ビルド設定を変更した場合は上記で検証し、ソースと再生成したバンドルを一緒に更新する。`pnpm check` は `src/`・`scripts/`・`tests/`、同梱PowerShell・Bashの構文と4つのJSON設定を確認する。
- `pnpm test:e2e` はAPI・認証、管理プロキシ、共有daemon、PCセットアップ、VPSセットアップCLIの5系統を順に実行する。ローカルの偽API・偽SSHと別プロセスのMCP・daemonを使い、実際のXへ投稿せず、実際のSSHホストへ接続しない。VPS CLIの確認だけで実Linuxへの導入成功を扱わない。失敗条件を先に確認し、挙動の検証は対応する既存E2Eへ追加する。
- 出力先を指定する場合は `PLUMIVOX_E2E_OUTPUT_DIR`、`PLUMIVOX_PROXY_E2E_OUTPUT_DIR`、`PLUMIVOX_PROXY_SHARED_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR` をそれぞれ別の、リポジトリとCodexユーザーディレクトリの外のフォルダーにする。未指定時は各系統がユーザーの一時ディレクトリ配下の `Codex/Plumivox/<UUID>` を使う。各出力先の `summary.json` の失敗数と各結果、API系の`http-trace.json`、VPS CLIの`mutation-trace.json`を読み、偽設定・画像・暗号化セッション・SSH鍵の清掃は呼び出し側で行う。
- 文書だけの変更では、リンク・コマンド・設定名・記載した挙動を対応する実装へ照合する。

## 実装の制約

- 標準出力はMCP通信専用。資格情報、復号Cookie、設定ファイルの内容、提供元の生エラーを応答や診断へ出さない。資格情報は設定ストア経由で取得し、MCP引数に追加しない。
- 設定ファイルの選択順と起動時の固定を維持し、選択後の読込み失敗で別ファイルへ切り替えない。WindowsセットアップとMCPの既定保存先を一致させる。セットアップでは既存鍵・同名アカウント・共有中継設定の上書きを拒否し、保存先・ACL・排他・保存直前の設定再確認を維持する。
- `setup/vps/` は `.gitattributes` に従いLFで保存する。VPSインストーラーの事前確認と管理対象の照合、専用SSHユーザーの制限、認証付きX宛先限定CONNECT、住宅回線出口不在時の直接接続禁止を維持する。初期構築の境界は [設計](DESIGN.md#初期設定と公開中継の構築) を参照する。
- 読取りと投稿の準備条件、Cookieの優先順位・照合、認証更新の排他と設定再確認を維持する。Mutex所有喪失後の保存を防ぎ、置換指示後の結果不明を未変更と断定しない。保存・通信・キャンセル失敗の検証も含める。
- 投稿・ログインを自動再試行しない。投稿IDが確認できない結果を成功にしない。認証更新から投稿を再送しない。同じ文面の明示呼出しは各回送信する。
- 管理プロキシは事前検証・認証ロックと準備後の設定再確認を維持し、external呼出しや一つのMCPの終了・待機取消で他クライアントの中継を停止しない。共有IPCの相互認証・設定照合、SSHホスト鍵検証、CONNECT宛先・DNS全候補の検証と資源上限を維持する。DNS・接続の検証用差し替えはtestsのfixtureに限定し、本番設定へ迂回経路を設けない。境界と停止の意味は [設計](DESIGN.md#管理するプロキシの境界) に従う。
- フィード・検索の取得範囲と未取得範囲、保護状態が不明な対象数を応答に残す。cursorの署名・アカウント照合・検索条件の一致を維持し、長さ超過では取得済み投稿と続きの有無を保持して継続不能を明示する。公開投稿の本文は未信頼データとして扱う。
- 検索語句を演算子として扱わず、期間とともに通信前に検証する。文字数判定・応答サイズ上限を含む失敗条件は [検証方針](docs/verification.md) に従ってE2Eで確認する。
- 配布物は `mcp.json` の `${PLUGIN_ROOT}/dist/plumivox.mjs` から起動し、利用者側の依存インストールや起動時のコード取得を必要としない構成を維持する。プラグイン登録・設定を変更した場合は `plugin.json`、`mcp.json`、`.agents/plugins/marketplace.json`、`package.json` とREADMEの導入手順の整合を確認する。
