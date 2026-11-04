# 自分のVPSへプロキシを導入する

Debian 12以降またはUbuntu 22.04以降の**専用VPS**と、到達可能な固定の公開IPv4を用意してください。SSH管理アクセスとUFWを先に設定・確認します。インストーラーはUFWを自動で有効化しません。SSHのポートや管理者の認証方式も変更しません。

1. PC側のセットアップでEd25519キーを作ります。秘密鍵をVPSへ送らず、`.pub`だけを使います。
2. [Windows・macOSの転送例](../../docs/setup.md#2-構築資材をvpsへ送る)に従い、管理者本人のリモート`$HOME`に`umask 077; mkdir -m 700`で事前に存在しない転送先を作ります。ホームと親階層は本人以外が書込みできず、リンクでないことを確認します。既存なら停止し、削除・再利用せず未使用の別名を全手順で揃えます。新規転送先`$HOME/plumivox-setup-1`の`assets/vps/`へこのディレクトリの資材、同じ転送先直下へ`plumivox_ed25519.pub`を送ります。失敗したら導入へ進みません。x86_64・aarch64では同梱の静的Rust実行ファイルを優先します。VPSへのRust導入は不要です。`install.sh`、`squid.conf.in`、`plumivox-proxy.service`は他CPU用のBash互換経路として残しています。
3. 管理者の端末から、sudo実行前にCPU、転送先の所有者・書込み権限・リンクでないこと、想定する資材と公開鍵の配置を確認します。想定外のファイル・構造なら停止します。その後、CPUに合う実行ファイルで事前確認し、成功した場合だけ導入します。

   ```bash
   setupRoot="$HOME/plumivox-setup-1"
   uname -m
   ls -ld "$HOME" "$setupRoot" "$setupRoot/assets" "$setupRoot/assets/vps"
   ls -l "$setupRoot/assets/vps/" "$setupRoot/plumivox_ed25519.pub"
   # 上の結果を確認してから進める
   case "$(uname -m)" in
     x86_64) installer=("$setupRoot/assets/vps/plumivox-vps-setup-x86_64") ;;
     aarch64) installer=("$setupRoot/assets/vps/plumivox-vps-setup-aarch64") ;;
     *) installer=(bash "$setupRoot/assets/vps/install.sh") ;;
   esac
   if [ "${installer[0]}" = bash ] || chmod u+x "${installer[0]}"; then
     if sudo "${installer[@]}" --check --public-key-file "$setupRoot/plumivox_ed25519.pub"; then
       sudo "${installer[@]}" --public-key-file "$setupRoot/plumivox_ed25519.pub"
     else
       printf '%s\n' '事前確認に失敗しました。導入せず、原因を確認してください。'
     fi
   else
     printf '%s\n' '実行権限を設定できません。導入せず、資材を確認してください。'
   fi
   ```

4. 上の手順は確認が通った場合だけ、同じ端末で導入へ進みます。失敗した場合は原因を確認し、導入コマンドだけを別に実行しないでください。

   プロキシパスワードを非表示で2回入力します。パスワード管理ツールで32～72文字のランダムな半角印字可能文字を生成してください。引数・環境変数へ渡さず、端末ログへ貼り付けないでください。保存するbcryptの上限を超える文字列は拒否します。
5. VPS提供元のファイアウォールで、公開TCPポート (既定3128) を許可します。逆トンネル用43129は公開せず、SSH管理ポートは維持します。インストーラーが出したSSHホスト鍵のSHA256指紋を、VPSの信頼できる管理コンソールで確認し、PC側のセットアップへ登録します。
6. PC側でVPSのアドレス・SSH専用ユーザー・指紋・プロキシ認証を登録し、MCPの`start_proxy`を実行します。PCが接続していない間はXへの通信が失敗します。

名前やポートを変更する場合は、初回の確認・導入の両方に同じオプションを指定します。

```bash
sudo "${installer[@]}" --check --public-key-file "$setupRoot/plumivox_ed25519.pub" \
  --public-port 3128 --remote-port 43129 --ssh-user plumivox --proxy-user plumivox
```

他のCPUでは、上の選択で同梱の`install.sh`をBash互換経路として使います。どちらも同じCLI引数で進められます。Rust版が安全条件を満たさず停止した環境で、その条件を回避するためにBash版へ切り替えないでください。

公開鍵とBash用の`squid.conf.in`・`plumivox-proxy.service`は、sudo呼出し本人またはrootの所有で、第三者が書込みできず、リンクでない通常ファイルにしてください。親階層も本人またはrootが管理する必要があります。今回の本人専用転送先を使い、確認・導入中に編集・差替えしないでください。インストーラーは入力元と読取り中の変更を検査し、固定した内容を使います。安全に固定できなければ停止します。

公開口はIPv4のBasic認証付きSquidです。CONNECTの443番ポートと`x.com`・`twitter.com`・`twimg.com`の各ドメインに限定し、TLSを復号しません。プロキシ認証はVPSまで暗号化されないため、専用のランダムなパスワードを使い、他サービスのパスワードを使い回さないでください。Squidは`127.0.0.1:43129`のPCへの逆トンネルにのみ転送し、VPS回線へフォールバックしません。

SSH専用ユーザーは公開鍵だけで認証し、シェル・TTY・ローカル転送・agent転送・Unixソケット転送を禁止します。逆転送の待受先は指定した`127.0.0.1`のポートだけです。公開鍵ファイルとホームはrootが所有し、専用ユーザーは書き換えできません。既存sshd設定の本文を保持し、末尾に専用ユーザーの設定へのIncludeだけを追加します。変更前にSSH構文と有効設定を検証し、既存ユーザー用の代表コンテキストが変わらないことも確認します。

専用SSHユーザーには`ClientAliveInterval 15`・`ClientAliveCountMax 3`を設定し、有効設定でも一致を確認します。PCの電源断や通信断で応答がなくなった接続をVPS側でも検知し、逆転送ポートの長時間占有を防ぎます。正常なアイドル接続は維持します。

既存導入へは新しい配布設定を自動で上書きしません。以前の版で導入した場合は管理者が専用ユーザーの生存確認設定と有効値を確認してください。SSHのreloadは既存接続へ新しい設定を適用しないため、既存の中継を所有者が停止してから再接続します。競合したポートの所有者を自動終了したり、空きポートへ変更したりしません。

安全に自動判定できない既存のMatch・独自Include・ユーザー/グループ制限・CA認証があるSSH構成は拒否します。既存Squid・同名ユーザー/グループ・競合ポート・同名ファイルも拒否します。これらを解除するために既存サーバーの保護を弱めず、標準構成の専用VPSを使ってください。

資格情報は`/etc/plumivox-proxy/passwd`へroot専用の権限で保存し、systemdの`LoadCredential`経由でSquid認証ヘルパーへ渡します。標準`squid.service`はパッケージ導入前にmaskして、自動起動を防ぎます。専用の`plumivox-proxy.service`だけを起動します。

同じ配布ファイル・公開鍵・引数での再実行は、管理対象のハッシュ・権限・有効SSH設定・サービス稼働を確認して終了し、パスワードと公開鍵を保持します。設定の編集、引数の変更、途中で失敗した導入は自動修復・上書きしません。失敗後は標準Squidのmaskを維持します。既存SSH設定の導入前原本は`/etc/plumivox-proxy/sshd_config.before`にroot専用で保持します。管理者が現状と原本を照合し、SSH管理アクセスを維持したまま原因を修正してください。原本をそのまま戻すと導入後の他者の変更を失うことがあるため、自動復元は行いません。

Rust版・Bash版とも新規導入は、SSH設定のACL・拡張属性・SELinuxや、全属性を確認するための`CAP_SYS_ADMIN`などを安全に扱えない環境では変更前に拒否します。既存の同一導入の読取り確認は、この新規導入条件の対象外です。ファイル交換には同一ファイルシステム内の`RENAME_EXCHANGE`が必要で、対応しない環境では安全に停止する場合があります。`--check`では交換のための書込み試験を行わないため、合格しても導入時に停止し得ます。

新規導入では初期user namespace内の`CAP_SYS_ADMIN`を確認します。コンテナ内のrootや追加のuser namespaceだけでは十分ではありません。SSH設定と親ディレクトリのmount IDが一致しない場合（bind mountを含む）、確認不能な場合、保持できないinode flags（immutable・nodumpなど）がある場合も拒否します。既存のファイル・ディレクトリ名と競合するときは、再利用・上書き・権限変更で回避しません。

保存先の祖先ディレクトリもroot所有と安全な権限を要求し、他者が書込みできる`/tmp`にはsticky bitが必要です。`/run`や既存の導入ロックの所有者・形式・権限・参照先が安全でない場合も停止します。共有領域の保護を弱めたり、既存ロックを削除したりして回避せず、管理者が環境と実行中の導入を確認してください。

Bash版の新規導入には、`mv --exchange --no-copy -T`と`ln -T`に対応するGNU coreutils、`getfacl`・`getfattr`・`sync`・`readlink`・`lsattr`が必要です。標準の古いcoreutilsや必要なコマンドが未導入の環境では、変更前に停止します。インストーラーは必要なコマンドを自動導入せず、coreutilsも自動更新しません。管理者が環境を確認して準備するまで導入を進めないでください。

SSH設定を交換する間は、他の管理操作で同じ設定を変更しないでください。Rust版・Bash版とも新規導入時に旧inodeを`/etc/ssh/.plumivox-sshd-backup.*`のroot限定ディレクトリへ保持します。旧ファイル記述子を利用する処理の終了、新旧の内容、SSH管理接続の動作を確認した後に、管理者本人が整理してください。このディレクトリも自動復元・自動清掃の対象にしません。

清掃失敗時は、通知された導入用一時ディレクトリ（stage）とSSHバックアップの保持先を管理者が確認してください。今回分の識別、SSH設定の交換状態、新旧内容を確認するまで、一括削除や再実行で処理を進めないでください。

`--check`は読取り専用で、パッケージ・一時ファイル・設定・ユーザーを作りません。初回のSquid/systemd候補検証はパッケージ導入後、恒久ファイルを書き込む前に実行します。Rust版のCLI・変更拒否経路は`cargo test --locked --package plumivox-vps-installer`、Bash互換経路の構文とCLI拒否は`cargo test --locked --features test-fixture --test setup_vps_e2e`で検証できます。これらの検証は実Linux root環境でのパッケージ導入、Squid起動、PCからの実際のSSH接続の確認とは別です。実システムへの導入は未確認です。
