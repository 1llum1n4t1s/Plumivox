# Plumivoxでプロキシを管理する

`managed` モードでは、PlumivoxがPC上のCONNECTプロキシとSSH逆転送を起動・停止し、接続が切れたときに中継だけを再接続します。Windowsの別タスク、GitのSSH実行ファイル、投稿前の外部プロキシ検査スクリプトは使いません。SSHライブラリは配布用バンドルに同梱します。

通信は `TwitterAPI.io → 公開認証付きプロキシ → SSH逆転送 → Plumivoxを実行するPC → X` です。PCの住宅回線側でポート開放する必要はありません。公開認証付きプロキシの初回準備は別途必要で、この機能がVPSを新規契約したり、共有中継サービスを作ったりすることはありません。

## 初回の準備

中継サーバーには、認証付きHTTPプロキシの公開口を用意します。既存構成ではSquidの公開口が3128番、上流が `127.0.0.1:43129` です。上流への転送先はX/Twitter/Twimgのドメインと443番に限定し、`never_direct allow all` で中継切断時のサーバー回線への迂回を禁止します。公開側の認証、接続制限、ファイアウォールも維持してください。

SSH利用者には、loopbackへのremote forwardingを許可します。公開bindや `GatewayPorts` の変更は不要です。SSHホスト鍵のSHA256指紋は、既に信頼している接続・管理コンソールから確認してください。未検証の鍵を自動で信用する処理はありません。

Plumivoxが使う設定ファイルへ共通中継設定を追加し、対象アカウントを `managed` にします。以下は形式の例です。指紋は実サーバーのものに置き換えてください。

```json
{
  "twitterapi_io": {
    "release_posting": {
      "api_key": "YOUR_API_KEY",
      "user_name": "YourAccount",
      "proxy": "http://user:password@relay.example.com:3128",
      "proxy_mode": "managed"
    }
  },
  "plumivox": {
    "managed_proxy": {
      "ssh_host": "relay.example.com",
      "ssh_port": 22,
      "ssh_user": "ubuntu",
      "private_key_file": "C:\\Users\\you\\.ssh\\id_ed25519",
      "host_key_sha256": "SHA256:REPLACE_WITH_VERIFIED_SERVER_FINGERPRINT",
      "local_port": 43128,
      "remote_port": 43129
    }
  }
}
```

SSH秘密鍵は暗号化されていない形式が必要です。既存の鍵にパスフレーズがある場合、それを解除するのではなく用途を限定した別の鍵を準備してください。秘密鍵・proxy資格情報は会話やMCP引数へ渡しません。公開proxy URLのホストは `ssh_host` と一致させます。複数アカウントで同じ中継を共有できます。

既存の公開proxy URLを維持すれば、Cookieのbindingも維持できます。URLを変更する場合は、設定更新後に `refresh_session` でログインCookieを取得し直してください。

## 起動・停止

- `start_proxy` にアカウント名を指定すると、ローカルbind、SSHホスト鍵検証、SSH認証、remote bindの成立後に `ready` を返します。Xへの投稿・ログインは行いません。
- `get_proxy_status` は、このMCPプロセスが所有する中継の状態・接続数・安全なエラーコードだけを返します。接続を開始せず、接続先や資格情報を返しません。
- `stop_proxy` は所有する中継を停止します。次の投稿・新規ログイン・セッション更新では必要な中継を再起動します。保存Cookieの再利用と公開投稿の読取りは中継を起動しません。

MCP終了時には住宅回線出口とSSH中継を閉じます。中継が停止している間は、この公開プロキシを使う投稿・認証更新も利用できません。1台・1中継ポートの所有者は一つのMCPプロセスに限定します。ポートが旧タスクや別プロセスに使われている場合は拒否し、他の所有者を自動終了しません。

一時的なSSH切断は、最大30秒までの待ち時間で連続6回まで再接続します。SSH認証拒否、ホスト鍵不一致、remote bind拒否は自動再試行しません。原因を修正した後に `stop_proxy`、`start_proxy` の順で再起動します。中継の復旧から投稿・ログインAPIを再送することはありません。

## 旧タスクから切り替える

1. GitHubの最新mainを参照してプラグインを更新し、Codexを再起動します。開発クローンをインストール元に変更しません。
2. 公開proxy URLを保持して中継設定を追加し、旧 `Kagayoi Twitter Home Egress` タスクを停止・無効化します。
3. `start_proxy` が `ready` になること、公開プロキシ経由のCONNECTが通ることを確認します。失敗時はPlumivoxを停止し、旧タスクを戻してから設定を確認します。
4. 成功を確認してから旧タスクと専用ファイルを撤去します。公開認証口・公開ポート・住宅回線出口への上流指定は引き続き必要です。

## Cloudflareとの関係

通常のCloudflare TunnelのTCP公開は接続元でも `cloudflared access tcp` を必要とし、TwitterAPI.io側にそのクライアントを追加できません。[公開プロトコル](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/routing-to-tunnel/protocols/)

WorkersのRequestはHTTP `CONNECT` をサポートしません。無料TunnelやWorkersを、この標準プロキシの公開口へ置き換える機能は実装していません。[Request仕様](https://developers.cloudflare.com/workers/runtime-apis/request/)

Spectrumの汎用TCPはEnterpriseの追加機能です。Tunnel側への接続方式と料金の確認が必要なため、自動設定や新規契約は行いません。[プラン表](https://developers.cloudflare.com/spectrum/protocols-per-plan/)
