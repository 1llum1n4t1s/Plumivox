---
name: setup-plumivox
description: Plumivoxの初期設定・自分のVPSへの住宅回線中継の構築・設定確認を案内する。通常のX投稿や検索には使わない。
---

# Plumivoxをセットアップする

同梱の[導入手順](../../docs/setup.md)を読み、利用者の現在位置から進める。PCの操作はWindowsの`dist/plumivox.exe`またはmacOSの`dist/plumivox`の`--setup`を使い、本体で対話入力を処理する。VPS資材は`setup/vps/`を使う。別リポジトリ、開発クローン、特定の利用者の保存先へ依存しない。利用者へNode.jsやRustの導入を要求しない。

読取り・検索だけならAPIキーとXユーザー名の設定で足りる。投稿には認証付きの公開プロキシとセッションが必要。自宅回線を使う場合は、利用者のVPSの公開プロキシ → SSH逆トンネル → 起動中のWindows PCまたはMacという経路を構築する。

- `--setup --prepare-key` → 公開鍵と資材の転送 → VPSの`--check`とインストール → PCの`--setup` → ローカル`--setup --check`とMCPの`start_proxy`の順に進める。必要な利用者操作は場所・操作・返してほしい結果を簡潔に示す。
- VPSへの変更は利用者が構築を依頼した対象と範囲に限る。新規契約・購入は行わない。既存サービスの削除、ファイアウォールの一括変更、管理者SSH設定の緩和で衝突を回避しない。
- VPSのx86_64・aarch64では`setup/vps/plumivox-vps-setup-x86_64`・`plumivox-vps-setup-aarch64`を優先する。他CPUでは同梱`install.sh`をBash互換経路として使える。導入手順のscp経路とCLI引数を使い、Rust版の安全確認を回避するためにBash版へ切り替えない。
- 転送先は本人以外が書込みできずリンクでないリモートホーム配下に、`umask 077; mkdir -m 700`で新規作成する。既存時は停止し、削除・再利用せず未使用の別名を選ぶ。資材は明示した`assets/vps/`、公開鍵はその親の新規転送先直下へ送り、全パスを揃える。作成・転送失敗時は続行しない。sudo前にCPU・本人所有と書込み権限・リンクでないこと・想定資材の配置を確認し、予測可能な共有`/tmp`の資材を実行しない。
- 公開鍵・Bash用テンプレートとサービス定義はsudo呼出し本人またはroot所有、第三者非書込み、非リンクの通常ファイルとし、親階層も本人/root管理下に置く。固定した入力内容を使う検査を維持し、確認・導入中は編集・差替えしない。保存先のroot所有の祖先、安全な`/tmp`のsticky bit、`/run`と導入ロックの安全確認で停止した場合は、保護を弱めたりロックを削除したりせず本人が環境を確認する。
- Rust版・Bash版とも新規導入ではACL・拡張属性・SELinux・`CAP_SYS_ADMIN`などを安全に扱えない条件を変更前に拒否する。既存の同一導入の読取り確認とは区別する。`--check`は書込みprobeを行わず、成功後も同一ファイルシステム内の`RENAME_EXCHANGE`非対応などで導入が安全に停止する場合がある。
- Bash版の新規導入には`mv --exchange --no-copy -T`と`ln -T`対応GNU coreutils、`getfacl`・`getfattr`・`sync`・`readlink`・`lsattr`が必要。古いcoreutilsや必要なコマンドがない環境では変更前に停止し、本人が環境を確認するまで自動導入・更新しない。
- 新規導入は初期user namespace内の`CAP_SYS_ADMIN`、SSH設定と親のmount ID、保持不能なinode flagsがないことも確認する。別mount・確認不能・既存名との競合を拒否し、既存データを再利用・上書きして回避しない。清掃失敗時は通知されたstage・バックアップの保持先と交換状態を本人が確認するまで、一括削除・再実行しない。
- SSH設定の交換中は同時管理操作を避ける。Rust版・Bash版とも旧inodeをroot限定のバックアップディレクトリに保持し、旧ファイル記述子の利用終了、新旧内容とSSH管理接続の動作を本人が確認してから整理する。自動復元・自動清掃をしない。CLI検証だけで実システムへの導入成功と扱わない。
- APIキー・パスワード・TOTP秘密鍵はウィザードの秘密入力で受け取る。会話・MCP引数・コマンド引数・ログに秘密値を求めない。VPSへ送る鍵は`.pub`だけ。秘密鍵はPCへ残す。
- SSH指紋は利用者がVPS事業者のコンソールなどの信頼できる経路で確認する。`ssh-keyscan`の結果だけを信頼せず、ホスト鍵検証を無効にしない。
- プラグインはGitHubの最新`main`から取得する。`codex plugin add ... --json`の`installedPath`を使い、キャッシュのバージョン名を推測しない。`config.toml`へ登録行を追加しない。
- Windows・macOSともネイティブの`--setup`を端末で実行し、秘密値をCLI引数へ渡さない。WindowsのPowerShell 7や実行ポリシーの変更は不要。
- Windowsの標準保存先は`%LOCALAPPDATA%\Kagayoi\Plumivox`、macOSは`~/Library/Application Support/Kagayoi/Plumivox`。`PLUMIVOX_SECRETS_FILE`と既存`~/dev/Secret/secrets.json`を含む選択順を維持する。macOSのセッション鍵は利用者のKeychainに保存し、利用不能時に平文へ切り替えない。
- `--setup --check`はローカル設定だけ、`start_proxy`はPC出口とSSH中継の確認。公開口への到達・実ログイン・投稿成功を確認済みと扱わない。セットアップだけの依頼ではログインや投稿を実行せず、追加の依頼があればその範囲で行う。

既存設定の保存先・アカウント追加は[設定](../../docs/configuration.md)、中継の起動・停止と共有動作は[管理プロキシ](../../docs/managed-proxy.md)で確認する。初回構築に必要な説明を毎回の投稿前チェックへ増やさない。
