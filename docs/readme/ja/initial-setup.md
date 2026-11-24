# 初期設定

初めて使う場合は[セットアップ手順](../../setup.md)に進んでください。Windows・macOSのネイティブ設定ウィザードと、Linux x86_64・aarch64向けの静的Rust VPSインストーラーを同梱します。他CPUにはBash互換経路を残しています。Codexへ「Plumivoxをセットアップして」と依頼すると、同梱の`setup-plumivox`スキルでも案内できます。

設定ファイルはリポジトリ外へ置きます。新規利用者はWindowsでは`%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`、macOSでは`~/Library/Application Support/Kagayoi/Plumivox/settings.json`を使います。既存の`~/dev/Secret/secrets.json`がある利用者は、そのファイルの`twitterapi_io`を読みます。vava／blblの`release_posting`と`blog_posting`も維持します。別の場所を使う場合は`PLUMIVOX_SECRETS_FILE`を指定します。秘密値を会話やMCPの引数へ渡す必要はありません。

選択された設定ファイルを読み込めない場合は、別の設定へ切り替えずエラーになります。保存先を変更した場合はCodexを再起動してください。初回セットアップは既存の鍵・同名アカウント・共有中継設定を上書きしません。既存設定の変更方法は[セットアップ手順](../../setup.md)を参照してください。

[初めてのセットアップ](../../setup.md) · [アカウントとCookieの設定](../../configuration.md) · [Plumivoxでプロキシを管理する](../../managed-proxy.md)
