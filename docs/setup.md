# 初めてのセットアップ

Windows PCと、自分で管理できるVPSを使って投稿できるようにします。公開投稿の読取り・検索だけならVPSは不要で、APIキーとXユーザー名だけを[設定](configuration.md)します。

```text
Plumivox → TwitterAPI.io → VPSの認証付きプロキシ
                              ↓ SSH逆トンネル
                         Windows PC → 自宅回線 → X
```

PCからVPSへSSH接続するため、自宅ルーターのポート開放は不要です。PCを終了している間は投稿・新規ログインを利用できません。

## 用意するもの

| 場所 | 必要なもの |
| --- | --- |
| Windows PC | Node.js 22以降、PowerShell 7、Codex、OpenSSHの`ssh`・`scp`・`ssh-keygen` |
| VPS | Ubuntu 22.04以降またはDebian 12以降、systemd、固定の公開IPv4、SSH管理者とsudo権限、有効なUFW |
| TwitterAPI.io | APIキー。API利用料とVPS料金は各サービスの契約に従います |
| X | 投稿先のユーザー名、ログイン用メールアドレスとパスワード。2FA利用時はBase32のTOTP秘密鍵 |

新規VPSを推奨します。既存のSquidや使用中のポートがあるサーバーでは、インストーラーが停止して既存サービスを保護します。VPSの公開IPv4または直接そのIPv4へ解決されるDNS名を使います。Cloudflareのオレンジ雲で代理するホスト名は使えません。

パスワードマネージャーで、プロキシ専用のランダムな32～72文字の半角パスワードを一つ用意します。同じ値をVPSとPCの対話入力で使います。APIキー・Xパスワード・TOTP秘密鍵・プロキシパスワードは会話やコマンド引数へ貼り付けません。

## 1. プラグインと専用SSH鍵を準備する（Windows）

PowerShell 7で実行します。取得元はGitHubの最新`main`です。`-ExecutionPolicy Bypass`はこの子プロセスの同梱スクリプトだけを実行可能にし、PCの永続的な実行ポリシーを変更しません。組織のポリシーで禁止されている場合は、その管理者に確認してください。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
$pluginRoot = (codex plugin add plumivox@plumivox-local --json | ConvertFrom-Json).installedPath
pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $pluginRoot 'setup/Setup-Plumivox.ps1') -PrepareKey
$publicKey = Join-Path $env:LOCALAPPDATA 'Kagayoi/Plumivox/keys/plumivox_ed25519.pub'
```

鍵は利用者だけが読める場所へ保存し、既存の鍵を上書きしません。鍵・設定のアクセス権は本人所有を確認してから制限し、所有者は変更しません。VPSへ送るのは`.pub`の公開鍵だけで、秘密鍵はPCへ残します。

`ssh`・`scp`が見つからない場合はWindowsの「オプション機能」で「OpenSSHクライアント」を追加し、ターミナルを開き直します。

## 2. 構築資材をVPSへ送る（Windows）

`203.0.113.10`を自分のVPSのIPv4に、`ubuntu`を現在使えるSSH管理者名に置き換えます。この管理者と、後で作る中継専用の`plumivox`利用者は別です。SSHポートを変更している場合は`22`も変更します。

```powershell
$vpsHost = '203.0.113.10'
$adminUser = 'ubuntu'
$sshPort = 22
scp -P $sshPort -r (Join-Path $pluginRoot 'setup/vps') "${adminUser}@${vpsHost}:/tmp/plumivox-setup"
scp -P $sshPort $publicKey "${adminUser}@${vpsHost}:/tmp/plumivox-setup/plumivox_ed25519.pub"
ssh -p $sshPort "${adminUser}@${vpsHost}"
```

初回SSH接続で表示されるホスト鍵指紋は、VPS事業者のコンソールで確認した指紋と照合します。`ssh-keyscan`の結果だけを根拠に承認しません。

## 3. VPSへインストールする（VPS）

VPS事業者のファイアウォールで、現在のSSHポートとTCP `3128`への接続を許可します。`43129`はVPS内のループバック専用なので公開しません。

新規VPSでUFWが未設定なら、事業者のコンソールから実際のSSHポートを許可してから有効にします。既存サーバーでは、管理者が既存サービスに必要なルールも確認します。

```bash
# UFWなどが未導入の新規VPSだけで実行
sudo apt-get update
sudo apt-get install -y ufw openssh-server iproute2

# SSHを22番で利用する新規VPSの例
sudo ufw allow 22/tcp
sudo ufw enable
```

まず変更しない事前確認を実行し、合格したらインストールします。

```bash
sudo bash /tmp/plumivox-setup/install.sh \
  --public-key-file /tmp/plumivox-setup/plumivox_ed25519.pub --check

sudo bash /tmp/plumivox-setup/install.sh \
  --public-key-file /tmp/plumivox-setup/plumivox_ed25519.pub
```

用意したプロキシパスワードを秘密入力で渡します。インストーラーはSquid、認証情報、中継専用SSH利用者、systemdサービスを設定し、UFWには公開プロキシのポートだけを追加します。専用SSH利用者は中継だけに使い、管理者のSSH接続を維持します。既存設定と衝突した場合は停止し、上書きしません。

完了時に表示される`SHA256:…`のSSHホスト鍵指紋を控えます。事業者のコンソールでも次のコマンドで照合できます。

```bash
sudo ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256
sudo systemctl is-active plumivox-proxy.service
```

公開ポートなどを変える場合は`--help`を確認し、PC側にも同じ値を設定します。通常の再実行でパスワードを再生成せず、構築後に変更されたファイルの上書きも拒否します。

## 4. アカウントを設定する（Windows）

VPSのターミナルから抜け、PowerShell 7で実行します。

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $pluginRoot 'setup/Setup-Plumivox.ps1')
```

アカウント設定名の既定値は`main`です。変更するときは同じコマンドに`-Account blog`などを追加し、確認時にも同じ名前を指定します。

初回登録したアカウントは、MCPの`account`を省略したときの既定としても保存します。既存の有効な既定は保持し、既定なしで複数候補があるときは設定名を選びます。登録時に`-DefaultAccount blog_posting`のように明示することもできます。投稿・検索・ログインで別のアカウントを使う場合は、呼出しの`account`を指定すると優先されます。

既存利用者は次のコマンドで既定だけを変更できます。APIキー・Cookie・SSH鍵・中継設定は変更せず、秘密入力や接続確認も行いません。`blog_posting`は登録済みの設定名へ置き換えてください。

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $pluginRoot 'setup/Setup-Plumivox.ps1') -SetDefaultAccount blog_posting
```

別の設定ファイルを使っている場合は`-SettingsPath <絶対パス>`も指定します。`PLUMIVOX_DEFAULT_ACCOUNT`を環境変数へ設定している場合は、その値が保存済みの既定より優先されます。

| 設定する項目 | 入力する値 |
| --- | --- |
| アカウント名 | 通常は`main`。Xユーザー名とは別の設定名 |
| APIキー・Xログイン情報 | 自分のTwitterAPI.io APIキーと投稿先アカウントの情報 |
| VPSホスト・SSHポート | 手順2と同じホスト・ポート |
| SSH利用者 | インストーラーの既定値は`plumivox` |
| SSHホスト鍵指紋 | 手順3で確認した`SHA256:…` |
| プロキシ利用者・パスワード | 利用者の既定値は`plumivox`。手順3と同じパスワード |
| 公開・PC・VPS内ポート | 通常は`3128`・`43128`・`43129` |

秘密値は画面に表示しない対話入力で受け取り、利用者だけが読める設定ファイルへ保存します。新規利用者の保存先は`%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`です。既存の`~/dev/Secret/secrets.json`がある場合はそちらを使い、既存プロファイルを上書きしません。優先順位は[設定](configuration.md)を参照してください。

既存設定の変更や複数アカウントでの中継共有は[管理プロキシ](managed-proxy.md)に従います。ウィザードは`config.toml`を変更しません。

## 5. 投稿せずに確認する（Windows・Codex）

まずローカル設定を確認します。VPS接続・API通信・投稿・ログインは行いません。

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $pluginRoot 'setup/Setup-Plumivox.ps1') -Check
```

Codexを再起動し、新しいチャットで「Plumivoxのmainの設定状態を確認して、プロキシを起動して」と依頼します。`list_accounts`で設定が見え、`start_proxy`が`ready`を返せば、PC出口とSSH逆トンネルが起動しています。公開プロキシへの外部到達やTwitterAPI.ioのログイン成功は、この確認だけでは保証しません。

ログインするときは「Plumivoxのmainにログインして」と明示的に依頼します。これはTwitterAPI.ioを利用する操作です。成功するとWindows DPAPIで暗号化したセッションを保存します。投稿は投稿先と文面を指定して依頼します。ログイン確認のために投稿する必要はありません。

## 困ったとき

| 状況 | 確認する場所 |
| --- | --- |
| 事前確認で停止 | 表示された不足コマンド・UFW状態・既存サービス。既存設定を削除せず原因を確認する |
| SSH鍵が拒否される | PCの秘密鍵とVPSの公開鍵の組合せ、専用利用者名、VPSのSSH制限 |
| SSHホスト鍵が一致しない | 事業者のコンソールで指紋を再確認する。検証を無効にしない |
| ポート競合 | 古いトンネルや別の中継が同じポートを使っていないか |
| `ready`だがログインできない | 事業者のTCP 3128許可、VPSサービス、プロキシ資格情報、Xログイン情報と2FA |
| セッション更新が必要 | 設定確認後に`refresh_session`を明示的に呼ぶ。投稿を自動再送しない |

VPS側の詳細は[同梱インストーラー](../setup/vps/README.md)、PC側の運用は[管理プロキシ](managed-proxy.md)を参照してください。
