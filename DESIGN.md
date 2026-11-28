# Plumivox の設計

詳細は [docs/design/](docs/design/) に分割している。操作は [README.md](README.md)、設定は [docs/configuration.md](docs/configuration.md)、作業規約は [AGENTS.md](AGENTS.md) を参照する。

## 目的と境界

PlumivoxはTwitterAPI.ioを経由してXへの投稿・認証更新・公開投稿の読取りを提供するローカルMCPサーバーである。Rust製のネイティブ実行ファイルをstdioで起動し（Windowsは Setup で導入した実行ファイル、macOSと従来の利用者はCodexプラグインの同梱実行ファイル）、ブラウザ操作は持たない。managedモードではloopback限定のCONNECT出口を起動し、公開プロキシへSSH逆転送を張る。詳細は [overview.md](docs/design/overview.md)。

## 主要コンポーネント

`server.rs`（MCP・ツール検証）、`config.rs`（設定の再読込み・準備状態）、`session.rs`（DPAPI／Keychainによる暗号化保存）、`client.rs`・`api_rejection.rs`（HTTP・投稿・フィード・拒否理由の固定分類）、`rejection_log.rs`（利用者が有効にしたときだけの拒否文面のローカル記録とCLI）、`post_guard.rs`・`post_guard_cli.rs`（Xのユーザー名ごとの投稿の冷却と、確認・解除のCLI）、`proxy*.rs`（中継・IPC・CONNECT出口）、`relay_log.rs`（中継の出来事の診断記録とCLI）、`setup*.rs`・`vps-installer/`（初期設定）、`app_update*`・`connect.rs`（Setup）、`xtask/`（ビルド・検査）。一覧は [architecture.md](docs/design/architecture.md)、検証は [testing.md](docs/design/testing.md)、配布CIと依存更新は [release-process.md](docs/design/release-process.md)。

## 初期設定と公開中継の構築

設定ファイルは環境変数、明示パス、既存の`secrets.json`、標準保存先の順に選び、起動後は別ファイルへ切り替えない。`--setup`は鍵・設定を本人限定の権限で保存し、既存値を上書きしない。VPSインストーラーは専用VPSへ認証付きのX宛先限定プロキシを作り、安全条件を満たさなければ変更前に拒否する。詳細は [configuration.md](docs/design/configuration.md)・[vps-installer.md](docs/design/vps-installer.md)。

## Setup（Velopack）

Windowsの既定配布。多プロセス型のため日中は適用せず、更新確認は背景、適用はログオン時の`--update-apply`だけが行い、他プロセスがあれば見送る。適用前に署名者を検証する。詳細は [setup-velopack.md](docs/design/setup-velopack.md)。

## ローカライズ

人が読む文言は9言語の辞書から引く。AIが読む文言（ツール説明・MCPエラー）は英語のまま辞書を通さない。詳細は [conventions.md](docs/design/conventions.md)。

## データフロー

要求をスキーマで検証し、アカウントを明示名・環境変数・既定の順で通信前に解決する。読取りはセッションに依存せず、投稿は文字数・画像を通信前に検証し、Xのユーザー名ごとの冷却ガードを通ってから投稿APIを1回だけ呼ぶ（冷却中は何も送らず`POST_COOLDOWN`で断る）。認証更新は排他し、暗号化したCookieを置換する。詳細は [workflow.md](docs/design/workflow.md)。

## 不変条件と採用した設計判断

- 資格情報と復号Cookieは外へ出さず、保存先はアカウント・APIキー・プロキシと結び付ける。通信先は固定し、応答は10MiBで打ち切る。提供元の生エラーは既定では保存せず、利用者がCLIで期限付きに有効にしたときだけ、伏字処理した拒否文面を本人限定のローカルファイルへ記録し、MCPへは出さない（[security.md](docs/design/security.md)、記録の仕様は [rejection-log.md](docs/design/rejection-log.md)）。
- 投稿・ログインは自動再送せず、成否不明は成功・未投稿と断定しない。画像だけ限定的に再試行する。本文の重複を理由には投稿を止めず、止めるのは時刻に基づく冷却ガードだけで、冷却はMCPからは解除できない。利用者本人がCLI（`--post-guard`）で確認・解除する（[workflow.md](docs/design/workflow.md)、[security.md](docs/design/security.md)）。フィードは網羅を保証せず、cursorは署名で改変を検知する（[api-reference.md](docs/design/api-reference.md)）。
- 既定アカウントは設定名を指すメタデータで、不正なら省略呼出しを拒否する（[configuration.md](docs/design/configuration.md)）。
- ネイティブ同梱によりNode.jsを不要にし、E2Eは実Xへ副作用を出さないため実サービスの成功は証明しない（[testing.md](docs/design/testing.md)）。

## 管理するプロキシの境界

`managed`のアカウントだけ事前検証後に中継を準備する。出口はX系ドメインの443番に限り、DNS全候補を検証する。IPCは相互HMACで認証し、SSH再接続は経路だけを復旧して投稿を再送しない。daemonと中継の状態の移り変わりは、固定コードと時刻だけを`<状態ルート>/relay/events.jsonl`へ常に記録し（ホスト・ポート・鍵・ユーザー名・IPは残さない）、閲覧は`--relay-log show`だけにする。書込みの失敗は中継へ影響させない。詳細は [security.md](docs/design/security.md)、運用は [docs/managed-proxy.md](docs/managed-proxy.md)。
