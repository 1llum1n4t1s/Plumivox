# 初めてのセットアップ

Windows PCまたはMacと、自分で管理できるVPSを使って投稿できるようにします。公開投稿の読取り・検索だけならVPSは不要で、APIキーとXユーザー名だけを[設定](configuration.md)します。

```text
Plumivox → TwitterAPI.io → VPSの認証付きプロキシ
                              ↓ SSH逆トンネル
                         PC / Mac → 自宅回線 → X
```

PCからVPSへSSH接続するため、自宅ルーターのポート開放は不要です。PCを終了している間は投稿・新規ログインを利用できません。

## 用意するもの

| 場所 | 必要なもの |
| --- | --- |
| Windows PC | Windows x64、Codex、OpenSSHの`ssh`・`scp`・`ssh-keygen` |
| Mac | Apple Silicon Mac（macOS ARM64）、Codex、`ssh`・`scp`・`ssh-keygen`。ログインする利用者のKeychainを利用可能なこと |
| VPS | Ubuntu 22.04以降またはDebian 12以降、systemd、固定の公開IPv4、SSH管理者とsudo権限、有効なUFW |
| TwitterAPI.io | APIキー。API利用料とVPS料金は各サービスの契約に従います |
| X | 投稿先のユーザー名、ログイン用メールアドレスとパスワード。2FA利用時はBase32のTOTP秘密鍵 |

新規VPSを推奨します。既存のSquidや使用中のポートがあるサーバーでは、インストーラーが停止して既存サービスを保護します。VPSの公開IPv4または直接そのIPv4へ解決されるDNS名を使います。Cloudflareのオレンジ雲で代理するホスト名は使えません。

パスワードマネージャーで、プロキシ専用のランダムな32～72文字の半角パスワードを一つ用意します。同じ値をVPSとPCの対話入力で使います。APIキー・Xパスワード・TOTP秘密鍵・プロキシパスワードは会話やコマンド引数へ貼り付けません。

## 1. プラグインと専用SSH鍵を準備する

取得元はGitHubの最新`main`です。Windows・macOSとも、同梱の実行ファイルの`--setup`が端末の対話入力を処理します。WindowsのPowerShell 7は不要です。次のWindows向けコマンド例はPowerShellで実行します。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
$pluginRoot = (codex plugin add plumivox@plumivox-local --json | ConvertFrom-Json).installedPath
& (Join-Path $pluginRoot 'dist/plumivox.exe') --setup --prepare-key
$publicKey = Join-Path $env:LOCALAPPDATA 'Kagayoi/Plumivox/keys/plumivox_ed25519.pub'
```

Apple Silicon Macではターミナルで実行します。同梱のmacOS実行ファイルはARM64専用です。鍵の作成にはmacOSの`ssh-keygen`を使います。

```bash
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local --json
# 上の結果のinstalledPathをそのまま指定する
pluginRoot='/取得結果のinstalledPath'
"$pluginRoot/dist/plumivox" --setup --prepare-key
publicKey="$HOME/Library/Application Support/Kagayoi/Plumivox/keys/plumivox_ed25519.pub"
```

鍵は利用者だけが読める場所へ保存し、既存の鍵を上書きしません。鍵・設定のアクセス権は本人所有を確認してから制限し、所有者は変更しません。VPSへ送るのは`.pub`の公開鍵だけで、秘密鍵はPCへ残します。

別の鍵の保存先を使う場合は`--private-key-path <絶対パス>`を指定し、後の設定・確認にも同じパスを渡します。秘密鍵の内容やパスフレーズを引数へ渡しません。

`ssh`・`scp`が見つからない場合はWindowsの「オプション機能」で「OpenSSHクライアント」を追加し、ターミナルを開き直します。

## 2. 構築資材をVPSへ送る

`203.0.113.10`を自分のVPSのIPv4に、`ubuntu`を現在使えるSSH管理者名に置き換えます。この管理者と、後で作る中継専用の`plumivox`利用者は別です。SSHポートを変更している場合は`22`も変更します。

転送先は、管理者本人のリモート`$HOME`配下に新規作成します。ホームとその親階層は本人以外に書込みを許さず、リンク経由でないことを先に確認してください。以下の`plumivox-setup-1`が既に存在する場合は停止します。既存のものを削除・再利用せず、未使用の別名を選んで以下の全箇所を揃えてください。新規作成・転送のいずれかが失敗したら、次の操作へ進みません。

公開鍵と、Bash版で使う`squid.conf.in`・`plumivox-proxy.service`は、sudoを呼び出す本人またはrootが所有し、第三者が書込みできない、リンクでない通常ファイルにします。親階層も本人またはrootの管理下に置きます。別の利用者が用意した共有資材をそのまま使わず、今回の転送先に本人が転送してください。インストーラーは入力の参照先・読取り中の変更を確認して内容を固定するため、確認・導入中は資材と公開鍵を編集・差替えしません。

```powershell
$vpsHost = '203.0.113.10'
$adminUser = 'ubuntu'
$sshPort = 22
ssh -p $sshPort "${adminUser}@${vpsHost}" 'umask 077; mkdir -m 700 -- "$HOME/plumivox-setup-1" && mkdir -m 700 -- "$HOME/plumivox-setup-1/assets"'
if ($LASTEXITCODE -ne 0) { throw '新規転送先を作成できません。既存先は再利用せず、別名を選んでください。' }
scp -P $sshPort -r (Join-Path $pluginRoot 'setup/vps') "${adminUser}@${vpsHost}:plumivox-setup-1/assets/"
if ($LASTEXITCODE -ne 0) { throw '資材の転送に失敗しました。導入へ進まないでください。' }
scp -P $sshPort $publicKey "${adminUser}@${vpsHost}:plumivox-setup-1/plumivox_ed25519.pub"
if ($LASTEXITCODE -ne 0) { throw '公開鍵の転送に失敗しました。導入へ進まないでください。' }
ssh -p $sshPort "${adminUser}@${vpsHost}"
```

macOSでは同じ値を使い、次のように転送します。

```bash
vpsHost='203.0.113.10'
adminUser='ubuntu'
sshPort=22
if ssh -p "$sshPort" "$adminUser@$vpsHost" 'umask 077; mkdir -m 700 -- "$HOME/plumivox-setup-1" && mkdir -m 700 -- "$HOME/plumivox-setup-1/assets"'; then
  scp -P "$sshPort" -r "$pluginRoot/setup/vps" "$adminUser@$vpsHost:plumivox-setup-1/assets/" &&
  scp -P "$sshPort" "$publicKey" "$adminUser@$vpsHost:plumivox-setup-1/plumivox_ed25519.pub" &&
  ssh -p "$sshPort" "$adminUser@$vpsHost"
else
  printf '%s\n' '新規転送先を作成できません。既存先は再利用せず、別名を選んでください。'
fi
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

sudoの前に、転送先が本人所有で本人以外に書込みを許さないこと、リンクでないこと、資材が`assets/vps/`直下、公開鍵がその親の新規転送先直下にあることを確認します。想定外の古いファイルやディレクトリ構造なら実行せず停止してください。

```bash
setupRoot="$HOME/plumivox-setup-1"
uname -m
ls -ld "$HOME" "$setupRoot" "$setupRoot/assets" "$setupRoot/assets/vps"
ls -l "$setupRoot/assets/vps/" "$setupRoot/plumivox_ed25519.pub"
```

確認したVPSのCPUに合う静的ネイティブ実行ファイルを選び、変更しない事前確認を実行します。x86_64・aarch64ではRust版を使います。実行ファイルを使うため、VPSへのRust導入は不要です。

```bash
case "$(uname -m)" in
  x86_64) installer=("$setupRoot/assets/vps/plumivox-vps-setup-x86_64") ;;
  aarch64) installer=("$setupRoot/assets/vps/plumivox-vps-setup-aarch64") ;;
  *) installer=(bash "$setupRoot/assets/vps/install.sh") ;;
esac
if [ "${installer[0]}" = bash ] || chmod u+x "${installer[0]}"; then
  if sudo "${installer[@]}" \
    --public-key-file "$setupRoot/plumivox_ed25519.pub" --check; then
    sudo "${installer[@]}" \
      --public-key-file "$setupRoot/plumivox_ed25519.pub"
  else
    printf '%s\n' '事前確認に失敗しました。導入せず、原因を確認してください。'
  fi
else
  printf '%s\n' '実行権限を設定できません。導入せず、資材を確認してください。'
fi
```

他のCPUでは、上の選択で同梱の`install.sh`をBash互換経路として使います。どちらも同じ引数で事前確認と導入を行います。Rust版の安全確認を回避する目的でBash版へ切り替えないでください。

用意したプロキシパスワードを秘密入力で渡します。インストーラーはSquid、認証情報、中継専用SSH利用者、systemdサービスを設定し、UFWには公開プロキシのポートだけを追加します。専用SSH利用者は中継だけに使い、管理者のSSH接続を維持します。既存設定と衝突した場合は停止し、上書きしません。

Rust版・Bash版とも新規導入では、SSH設定のACL・拡張属性・SELinuxや、全属性の確認に必要な`CAP_SYS_ADMIN`などを安全に扱えない環境を変更前に拒否します。既存の同一導入の読取り確認はこの新規導入条件とは別です。`--check`は書込みをしないため、合格しても導入時の同一ファイルシステム内の交換機能が利用できず、安全に停止する場合があります。

新規導入には初期user namespace内の`CAP_SYS_ADMIN`が必要です。コンテナ内のrootや追加のuser namespaceだけでは条件を満たしません。SSH設定と親ディレクトリのmount IDを照合し、別mount（対象ファイルだけのbind mountを含む）や確認不能な状態、保持できないinode flags（immutable・nodumpなど）も拒否します。既存のファイル・ディレクトリ名と競合する場合は、再利用や上書きで進めません。

保存先の祖先ディレクトリもroot所有・安全な権限である必要があります。他者が書込みできる`/tmp`にはsticky bitが必要です。`/run`や既存の導入ロックが安全に確認できない場合も停止します。検査を通すために共有領域の保護を弱めたり、既存ロックを削除したりせず、管理者が環境と実行中の導入を確認してください。

Bash版の新規導入には、`mv --exchange --no-copy -T`と`ln -T`に対応するGNU coreutils、`getfacl`・`getfattr`・`sync`・`readlink`・`lsattr`も必要です。標準の古いcoreutilsや必要なコマンドが未導入の環境では、変更前に停止します。インストーラーはこれらを自動導入・更新しません。管理者が環境を確認し、必要な準備を行ってから再実行してください。

導入中は別の管理操作でSSH設定を変更しないでください。Rust版・Bash版ともSSH設定の交換時に旧inodeをroot限定のバックアップディレクトリへ保持します。旧ファイル記述子を使う処理が終了し、新旧内容とSSH管理接続の動作を確認してから、管理者本人が整理します。自動復元・自動清掃は行いません。詳細は[同梱インストーラーの説明](../setup/vps/README.md)を参照してください。

清掃に失敗した場合は、案内された導入用一時ディレクトリ（stage）やSSHバックアップの保持先を確認してください。今回のものか、SSH設定の交換がどこまで進んだかを確認する前に、一括削除や導入の再実行をしないでください。

完了時に表示される`SHA256:…`のSSHホスト鍵指紋を控えます。事業者のコンソールでも次のコマンドで照合できます。

```bash
sudo ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256
sudo systemctl is-active plumivox-proxy.service
```

公開ポートなどを変える場合は`--help`を確認し、PC側にも同じ値を設定します。通常の再実行でパスワードを再生成せず、構築後に変更されたファイルの上書きも拒否します。

## 4. アカウントを設定する

VPSのターミナルから抜け、Windowsの端末で実行します。次の例はPowerShell用です。

```powershell
& (Join-Path $pluginRoot 'dist/plumivox.exe') --setup
```

macOSでは`"$pluginRoot/dist/plumivox" --setup`を実行します。アカウント設定名の既定値は`main`です。変更するときは同じコマンドに`--account blog`などを追加し、確認時にも同じ名前を指定します。

初回登録したアカウントは、MCPの`account`を省略したときの既定としても保存します。既存の有効な既定は保持し、既定なしで複数候補があるときは設定名を選びます。登録時に`--default-account blog_posting`のように明示することもできます。投稿・検索・ログインで別のアカウントを使う場合は、呼出しの`account`を指定すると優先されます。

既存利用者は次のコマンドで既定だけを変更できます。APIキー・Cookie・SSH鍵・中継設定は変更せず、秘密入力や接続確認も行いません。`blog_posting`は登録済みの設定名へ置き換えてください。

```powershell
& (Join-Path $pluginRoot 'dist/plumivox.exe') --setup --set-default-account blog_posting
```

macOSでは`"$pluginRoot/dist/plumivox" --setup --set-default-account blog_posting`を使います。別の設定ファイルを使っている場合は`--settings-path <絶対パス>`も指定します。`PLUMIVOX_DEFAULT_ACCOUNT`を環境変数へ設定している場合は、その値が保存済みの既定より優先されます。

| 設定する項目 | 入力する値 |
| --- | --- |
| アカウント名 | 通常は`main`。Xユーザー名とは別の設定名 |
| APIキー・Xログイン情報 | 自分のTwitterAPI.io APIキーと投稿先アカウントの情報 |
| VPSホスト・SSHポート | 手順2と同じホスト・ポート |
| SSH利用者 | インストーラーの既定値は`plumivox` |
| SSHホスト鍵指紋 | 手順3で確認した`SHA256:…` |
| プロキシ利用者・パスワード | 利用者の既定値は`plumivox`。手順3と同じパスワード |
| 公開・PC・VPS内ポート | 通常は`3128`・`43128`・`43129` |

秘密値は画面に表示しない対話入力で受け取り、利用者だけが読める設定ファイルへ保存します。新規利用者の保存先はWindowsでは`%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`、macOSでは`~/Library/Application Support/Kagayoi/Plumivox/settings.json`です。macOSではディレクトリを700、ファイルを600にし、本人所有とリンクを検証します。既存の`~/dev/Secret/secrets.json`がある場合はそちらを使い、既存プロファイルを上書きしません。優先順位は[設定](configuration.md)を参照してください。

既存設定の変更や複数アカウントでの中継共有は[管理プロキシ](managed-proxy.md)に従います。ウィザードは`config.toml`を変更しません。

## 5. 投稿せずに確認する

まずローカル設定を確認します。VPS接続・API通信・投稿・ログインは行いません。

```powershell
& (Join-Path $pluginRoot 'dist/plumivox.exe') --setup --check
```

macOSでは`"$pluginRoot/dist/plumivox" --setup --check`を使います。

Codexを再起動し、新しいチャットで「Plumivoxのmainの設定状態を確認して、プロキシを起動して」と依頼します。`list_accounts`で設定が見え、`start_proxy`が`ready`を返せば、PC出口とSSH逆トンネルが起動しています。公開プロキシへの外部到達やTwitterAPI.ioのログイン成功は、この確認だけでは保証しません。

ログインするときは「Plumivoxのmainにログインして」と明示的に依頼します。これはTwitterAPI.ioを利用する操作です。成功するとWindowsはDPAPI、macOSはKeychain内の鍵を使うAES-GCMで暗号化したセッションを保存します。Keychainの利用をmacOSから求められた場合は、Plumivoxの操作であることを確認して応答してください。投稿は投稿先と文面を指定して依頼します。ログイン確認のために投稿する必要はありません。

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
