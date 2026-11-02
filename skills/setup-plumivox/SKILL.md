---
name: setup-plumivox
description: Plumivoxの初期設定・自分のVPSへの住宅回線中継の構築・設定確認を案内する。通常のX投稿や検索には使わない。
---

# Plumivoxをセットアップする

同梱の[導入手順](../../docs/setup.md)を読み、利用者の現在位置から進める。PCの操作はWindowsの`dist/plumivox.exe`またはmacOSの`dist/plumivox`の`--setup`を使う。Windowsは同梱`setup/Setup-Plumivox.ps1`へ転送し、macOSは本体で対話入力を処理する。VPS資材は`setup/vps/`を使う。別リポジトリ、開発クローン、特定の利用者の保存先へ依存しない。利用者へNode.jsやRustの導入を要求しない。

読取り・検索だけならAPIキーとXユーザー名の設定で足りる。投稿には認証付きの公開プロキシとセッションが必要。自宅回線を使う場合は、利用者のVPSの公開プロキシ → SSH逆トンネル → 起動中のWindows PCまたはMacという経路を構築する。

- `--setup --prepare-key` → 公開鍵と資材の転送 → VPSの`--check`とインストール → PCの`--setup` → ローカル`--setup --check`とMCPの`start_proxy`の順に進める。必要な利用者操作は場所・操作・返してほしい結果を簡潔に示す。
- VPSへの変更は利用者が構築を依頼した対象と範囲に限る。新規契約・購入は行わない。既存サービスの削除、ファイアウォールの一括変更、管理者SSH設定の緩和で衝突を回避しない。
- APIキー・パスワード・TOTP秘密鍵はウィザードの秘密入力で受け取る。会話・MCP引数・コマンド引数・ログに秘密値を求めない。VPSへ送る鍵は`.pub`だけ。秘密鍵はPCへ残す。
- SSH指紋は利用者がVPS事業者のコンソールなどの信頼できる経路で確認する。`ssh-keyscan`の結果だけを信頼せず、ホスト鍵検証を無効にしない。
- プラグインはGitHubの最新`main`から取得する。`codex plugin add ... --json`の`installedPath`を使い、キャッシュのバージョン名を推測しない。`config.toml`へ登録行を追加しない。
- WindowsではPowerShell 7を使い、ネイティブの`--setup`から取得元を確認した同梱スクリプトだけを子プロセスで実行する。永続ポリシーや組織の制限は変更しない。macOSでは端末の対話入力を使い、秘密値をCLI引数へ渡さない。
- Windowsの標準保存先は`%LOCALAPPDATA%\Kagayoi\Plumivox`、macOSは`~/Library/Application Support/Kagayoi/Plumivox`。`PLUMIVOX_SECRETS_FILE`と既存`~/dev/Secret/secrets.json`を含む選択順を維持する。macOSのセッション鍵は利用者のKeychainに保存し、利用不能時に平文へ切り替えない。
- `--setup --check`はローカル設定だけ、`start_proxy`はPC出口とSSH中継の確認。公開口への到達・実ログイン・投稿成功を確認済みと扱わない。セットアップだけの依頼ではログインや投稿を実行せず、追加の依頼があればその範囲で行う。

既存設定の保存先・アカウント追加は[設定](../../docs/configuration.md)、中継の起動・停止と共有動作は[管理プロキシ](../../docs/managed-proxy.md)で確認する。初回構築に必要な説明を毎回の投稿前チェックへ増やさない。
