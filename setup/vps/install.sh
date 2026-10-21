#!/usr/bin/env bash
# Debian 12+/Ubuntu 22.04+専用。専用VPSへ対話的に導入する。
set +x +v
set -euo pipefail
export LC_ALL=C
umask 077
fail() { printf 'エラー: %s\n' "$*" >&2; exit 1; }
help() {
  cat <<'HELP'
Plumivox VPSプロキシの導入 (Debian 12+ / Ubuntu 22.04+)
sudo bash install.sh --public-key-file ./plumivox_ed25519.pub [オプション]
  --public-port PORT    認証付き公開IPv4プロキシ (既定: 3128)
  --remote-port PORT    SSH逆トンネルの127.0.0.1待受 (既定: 43129)
  --ssh-user NAME       新規のSSH専用ユーザー (既定: plumivox)
  --proxy-user NAME     Basic認証ユーザー (既定: plumivox)
  --check               事前確認だけ。パッケージ/設定/ユーザー/一時ファイルを作らない
  --help                この説明を表示
公開鍵はPCで作成したEd25519の.pubファイルだけを指定します。
初回導入でプロキシパスワードを非表示入力します (32文字以上)。
パスワードは引数・環境変数へ渡さないでください。
既に有効なUFWとSSHが必要です。SSHポート/管理者認証は変更しません。
VPS提供元のファイアウォールにも公開TCPポートを手動で許可してください。
既存Squid/同名ユーザー/競合ポート/変更済み設定は上書きしません。
HELP
}
public_port=3128 remote_port=43129 ssh_user=plumivox proxy_user=plumivox key_file='' check=0
while (($#)); do
  case "$1" in
    --help) help; exit 0 ;;
    --check) check=1; shift ;;
    --public-key-file|--public-port|--remote-port|--ssh-user|--proxy-user)
      (($# >= 2)) || fail "$1 の値がありません"
      case "$1" in
        --public-key-file) key_file=$2 ;;
        --public-port) public_port=$2 ;;
        --remote-port) remote_port=$2 ;;
        --ssh-user) ssh_user=$2 ;;
        --proxy-user) proxy_user=$2 ;;
      esac
      shift 2 ;;
    *) fail "未対応の引数です (--helpで確認してください)" ;;
  esac
done
for port in "$public_port" "$remote_port"; do
  [[ $port =~ ^[1-9][0-9]{0,4}$ ]] && ((10#$port >= 1024 && 10#$port <= 65535)) || fail 'ポートは1024～65535の整数です'
done
[[ $public_port != "$remote_port" ]] || fail '公開ポートと逆トンネルポートを分けてください'
for name in "$ssh_user" "$proxy_user"; do
  [[ $name =~ ^[a-z][a-z0-9_-]{0,30}$ ]] || fail 'ユーザー名は小文字英数字・_・-で指定してください'
done
[[ -n $key_file && -f $key_file && ! -L $key_file ]] || fail '通常ファイルのEd25519公開鍵を--public-key-fileへ指定してください'
((EUID == 0)) || fail 'sudoで実行してください (--checkもrootによる読取り確認が必要です)'
[[ $(uname -s) == Linux && -f /etc/os-release ]] || fail '対応するLinux VPS上で実行してください'
# os-releaseはOS管理ファイル。
. /etc/os-release
case "${ID:-}:${VERSION_ID:-}" in
  debian:*) [[ ${VERSION_ID%%.*} =~ ^[0-9]+$ ]] && ((${VERSION_ID%%.*} >= 12)) || fail 'Debian 12以降が必要です' ;;
  ubuntu:*) [[ $VERSION_ID =~ ^[0-9]+\.[0-9]+$ ]] && [[ $(printf '%s\n' 22.04 "$VERSION_ID" | sort -V | head -n1) == 22.04 ]] || fail 'Ubuntu 22.04以降が必要です' ;;
  *) fail 'Debian 12+またはUbuntu 22.04+が必要です' ;;
esac
for command in systemctl systemd-analyze sshd ssh-keygen ufw ss sha256sum stat getent awk sed grep install useradd apt-get dpkg-query flock cmp mktemp; do
  command -v "$command" >/dev/null || fail "必要なコマンドがありません: $command (SSH・UFW・iproute2を管理者が準備してください)"
done
systemctl is-active --quiet ssh.service || fail 'ssh.serviceが起動している必要があります'
[[ $(ufw status | head -n1) == 'Status: active' ]] || fail '既存のUFWが有効である必要があります。SSH管理アクセスを許可・確認したうえで管理者が有効化してください'
[[ -f /etc/ssh/sshd_config && ! -L /etc/ssh/sshd_config ]] || fail '通常ファイルのsshd_configが必要です'
sshd -t >/dev/null 2>&1 || fail '現在のsshd_configの検証に失敗しました'
key=$(awk 'NF {if(++n>1 || $1!="ssh-ed25519" || NF<2)exit 1; print $1" "$2} END{if(n!=1)exit 1}' "$key_file") || fail '公開鍵は一行のEd25519公開鍵にしてください'
ssh-keygen -l -f "$key_file" >/dev/null 2>&1 || fail '公開鍵が不正です'
base=/etc/plumivox-proxy
home=/var/lib/plumivox-ssh
unit=/etc/systemd/system/plumivox-proxy.service
ssh_conf=$base/ssh.conf
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
[[ -f $source_dir/squid.conf.in && -f $source_dir/plumivox-proxy.service ]] || fail '配布ファイルが不足しています'
render_squid() { sed -e "s/@PUBLIC_PORT@/$public_port/g" -e "s/@REMOTE_PORT@/$remote_port/g" "$source_dir/squid.conf.in"; }
render_ssh() {
  cat <<SSH
# Plumivox所有: このユーザーだけに適用
Match User $ssh_user
    AuthenticationMethods publickey
    PubkeyAuthentication yes
    AuthorizedKeysFile .ssh/authorized_keys
    AuthorizedKeysCommand none
    PasswordAuthentication no
    KbdInteractiveAuthentication no
    ClientAliveInterval 15
    ClientAliveCountMax 3
    AllowTcpForwarding remote
    AllowStreamLocalForwarding no
    GatewayPorts no
    PermitListen 127.0.0.1:$remote_port
    PermitOpen none
    AllowAgentForwarding no
    X11Forwarding no
    PermitTTY no
    ForceCommand /usr/sbin/nologin
Match all
SSH
}
render_key() { printf 'restrict,port-forwarding,permitlisten="127.0.0.1:%s",command="/usr/sbin/nologin" %s\n' "$remote_port" "$key"; }
render_options() { printf 'format=1\npublic_port=%s\nremote_port=%s\nssh_user=%s\nproxy_user=%s\n' "$public_port" "$remote_port" "$ssh_user" "$proxy_user"; }
owned=0
if [[ -e $base || -L $base ]]; then
  [[ -d $base && ! -L $base && $(stat -c '%u:%a' "$base") == 0:755 && -f $base/manifest.sha256 && ! -L $base/manifest.sha256 ]] || fail '既存の導入ディレクトリは所有・形式を確認できません'
  # マニフェスト自体もroot所有・非書込可能とし、固定の対象だけ検証する。
  [[ $(stat -c '%u:%a' "$base/manifest.sha256") == 0:600 ]] || fail 'マニフェストの権限が変更されています'
  expected_paths=("$base/options" "$base/squid.conf" "$base/passwd" "$unit" "$ssh_conf" "$home/.ssh/authorized_keys" /etc/ssh/sshd_config "$base/sshd_config.before")
  mapfile -t manifest_lines < "$base/manifest.sha256"
  ((${#manifest_lines[@]} == ${#expected_paths[@]})) || fail 'マニフェストが変更されています'
  for index in "${!expected_paths[@]}"; do
    path=${expected_paths[$index]}
    [[ ${manifest_lines[$index]:0:64} =~ ^[a-f0-9]{64}$ && ${manifest_lines[$index]:64} == "  $path" && -f $path && ! -L $path && $(stat -c %u "$path") == 0 ]] || fail '管理対象またはマニフェストが変更されています'
  done
  for path in "$base/options" "$base/passwd" "$base/sshd_config.before"; do
    [[ $(stat -c '%u:%g:%a' "$path") == 0:0:600 ]] || fail 'root専用ファイルの所有者・権限が変更されています'
  done
  for path in "$base/squid.conf" "$unit" "$ssh_conf" "$home/.ssh/authorized_keys"; do
    [[ $(stat -c '%u:%g:%a' "$path") == 0:0:644 ]] || fail '管理対象ファイルの所有者・権限が変更されています'
  done
  managed_count=0
  for path in "$base"/* "$base"/.[!.]* "$base"/..?*; do
    [[ -e $path || -L $path ]] || continue
    ((managed_count+=1))
    case "$path" in "$base/options"|"$base/squid.conf"|"$base/passwd"|"$base/ssh.conf"|"$base/sshd_config.before"|"$base/manifest.sha256") ;; *) fail '導入ディレクトリに未管理のファイルがあります' ;; esac
  done
  ((managed_count == 6)) || fail '導入ディレクトリのファイル数が変わっています'
  sha256sum --check --status "$base/manifest.sha256" || fail '管理対象ファイルが変更されています。再実行で上書きしません'
  [[ $(stat -c %a "$base/passwd") == 600 && $(stat -c %a "$home/.ssh/authorized_keys") == 644 && $(stat -c %a "$base/sshd_config.before") == 600 ]] || fail '資格情報・公開鍵・復元原本の権限が変更されています'
  [[ $(render_options) == "$(cat "$base/options")" && $(render_squid) == "$(cat "$base/squid.conf")" && $(render_ssh) == "$(cat "$ssh_conf")" && $(render_key) == "$(cat "$home/.ssh/authorized_keys")" ]] || fail '導入条件・公開鍵・配布設定が変わっています。既存導入の変更は管理者が確認してください'
  cmp -s "$source_dir/plumivox-proxy.service" "$unit" || fail '配布サービスが既存導入と異なります'
  getent passwd "$ssh_user" | awk -F: -v home="$home" '$6==home && $7=="/usr/sbin/nologin" {ok=1} END{exit !ok}' || fail 'SSHユーザーの設定が変更されています'
  [[ $(stat -c '%u:%a' "$home") == 0:755 && $(stat -c '%u:%a' "$home/.ssh") == 0:755 ]] || fail 'SSHホームの権限が変更されています'
  owned=1
else
  for path in "$home" "$unit"; do [[ ! -e $path && ! -L $path ]] || fail '同名の既存ファイルがあります'; done
  ! getent passwd "$ssh_user" >/dev/null || fail 'SSHユーザーが既に存在します。新しい専用ユーザー名を指定してください'
  ! getent group "$ssh_user" >/dev/null || fail '同名のグループが既に存在します。新しい専用ユーザー名を指定してください'
fi
# 未知の接続元にだけ適用されるMatchを見落とさないよう、初期SSH構成を限定する。
baseline=/etc/ssh/sshd_config
((!owned)) || baseline=$base/sshd_config.before
awk ' /^[ \t]*#/ {next} tolower($1)=="match" {exit 1} tolower($1)=="include" && ($2!="/etc/ssh/sshd_config.d/*.conf" || NF!=2) {exit 1}' "$baseline" || fail '既存SSHにMatchまたは独自Includeがあります。標準構成の専用VPSで実行してください'
for dropin in /etc/ssh/sshd_config.d/*.conf; do
  [[ -e $dropin ]] || continue
  [[ -f $dropin && ! -L $dropin ]] || fail 'SSH drop-inの形式を確認できません'
  awk ' /^[ \t]*#/ {next} tolower($1)=="match" || tolower($1)=="include" {exit 1}' "$dropin" || fail '既存SSH drop-inにMatchまたは独自Includeがあります。管理者が確認してください'
done
[[ $(systemctl show squid.service -p LoadState --value) == not-found ]] || {
  # 自身がインストールしたSquidは標準サービスの起動をmaskしたまま使う。
  ((owned)) && [[ $(systemctl is-enabled squid.service 2>/dev/null || :) == masked ]] || fail '既存Squidがあります。専用VPSを使ってください'
}
if ((!owned)); then
  for path in /etc/squid /var/log/squid /var/spool/squid; do [[ ! -e $path && ! -L $path ]] || fail '既存Squidの設定・データがあります。専用VPSを使ってください'; done
  ! dpkg-query -W -f='${Status}' squid 2>/dev/null | grep -q 'install ok installed' || fail '既存Squidパッケージがあります。専用VPSを使ってください'
  for port in "$public_port" "$remote_port"; do
    [[ -z $(ss -H -ltn "sport = :$port") ]] || fail "TCPポート$port は使用中です"
  done
fi
effective=$(sshd -T -C "user=$ssh_user,host=localhost,addr=127.0.0.1") || fail 'SSH有効設定を確認できません'
[[ $(awk '$1=="disableforwarding"{print $2}' <<<"$effective") == no ]] || fail '既存SSH設定が転送を禁止しています。管理者が確認してください'
# ユーザー/グループ・CAの許可条件を自動で拡張しない。
! grep -Eq '^(allowusers|allowgroups|denyusers|denygroups) ' <<<"$effective" || fail 'SSHのユーザー/グループ制限が設定されています。専用VPSで管理者が適用範囲を確認してください'
[[ $(awk '$1=="trustedusercakeys"{print $2}' <<<"$effective") == none ]] || fail 'SSHのCA認証があります。公開鍵だけの専用ユーザーを保証できません'
if ((!owned)); then
  [[ $(awk '$1=="allowtcpforwarding"{print $2}' <<<"$effective") =~ ^(yes|all|remote)$ ]] || fail '既存SSH設定でremote forwardingが許可されていません。管理者が確認してください'
fi
verify_effective() {
  local effective=$1 line
  for line in 'authenticationmethods publickey' 'pubkeyauthentication yes' 'authorizedkeysfile .ssh/authorized_keys' 'authorizedkeyscommand none' 'passwordauthentication no' 'kbdinteractiveauthentication no' 'clientaliveinterval 15' 'clientalivecountmax 3' 'allowtcpforwarding remote' 'allowstreamlocalforwarding no' 'gatewayports no' "permitlisten 127.0.0.1:$remote_port" 'permitopen none' 'allowagentforwarding no' 'x11forwarding no' 'permittty no' 'forcecommand /usr/sbin/nologin' 'disableforwarding no'; do
    grep -Fxq "$line" <<<"$effective" || fail 'SSHの既存Match制約と専用設定が競合します。管理者が確認してください'
  done
}
candidate_sshd() { cat /etc/ssh/sshd_config; printf '\n'; render_ssh; }
# process substitutionはメモリー上の候補を読ませ、一時ファイルを作らない。
if ((!owned)); then
  sshd -t -f <(candidate_sshd) >/dev/null 2>&1 || fail '候補SSH設定が不正です'
  verify_effective "$(sshd -T -f <(candidate_sshd) -C "user=$ssh_user,host=localhost,addr=127.0.0.1")"
  # 新規ユーザー以外の代表コンテキストに変更がないことも確認する。
  admin_user=plumivox-admin-check
  [[ $admin_user != "$ssh_user" ]] || admin_user=plumivox-admin-check2
  [[ $(sshd -T -C "user=$admin_user,host=localhost,addr=127.0.0.1") == "$(sshd -T -f <(candidate_sshd) -C "user=$admin_user,host=localhost,addr=127.0.0.1")" ]] || fail '既存SSHの他ユーザー設定へ影響するため導入を拒否しました'
fi
if ((owned)); then
  verify_effective "$effective"
  /usr/sbin/squid -k parse -f "$base/squid.conf" >/dev/null 2>&1 || fail 'Squid設定の検証に失敗しました'
  systemctl is-active --quiet plumivox-proxy.service || fail '既存サービスが停止しています。管理者が原因を確認してください'
  printf '既存導入は同一です。パスワード・公開鍵・設定を保持しました。\n'
elif ((check)); then
  printf '事前確認合格。導入時にSquid/apache2-utilsを追加し、候補設定を検証します。\n'
fi
show_fingerprints() {
  printf 'SSHホスト鍵 (SHA256):\n'
  for host_key in /etc/ssh/ssh_host_*_key.pub; do
    [[ -f $host_key ]] && ssh-keygen -l -E sha256 -f "$host_key" | awk '{print $2" "$4}'
  done
}
if ((check || owned)); then show_fingerprints; exit 0; fi
# この先だけが変更処理。root専用の固定ロックを取り、並行導入を拒否する。
[[ ! -L /run/plumivox-proxy-install.lock ]] || fail '導入ロックが不正です'
exec 9>/run/plumivox-proxy-install.lock
flock -n 9 || fail '別の導入が実行中です'
[[ ! -e $base && ! -e $home && ! -e $unit && ! -e $ssh_conf ]] || fail '事前確認後に導入状態が変わりました'
! getent passwd "$ssh_user" >/dev/null && ! getent group "$ssh_user" >/dev/null || fail '事前確認後にユーザーまたはグループが作成されました'
for port in "$public_port" "$remote_port"; do
  [[ -z $(ss -H -ltn "sport = :$port") ]] || fail '事前確認後にポートが使用されました'
done
stage=$(mktemp -d /tmp/plumivox-proxy.XXXXXXXX)
cleanup() {
  unset password confirmation
  # 導入失敗後もSquidの標準サービスはmaskを維持し、不明な公開口を作らない。
  rm -rf -- "$stage"
}
trap cleanup EXIT
render_squid > "$stage/squid.conf"
render_ssh > "$stage/ssh.conf"
render_key > "$stage/authorized_keys"
render_options > "$stage/options"
cp "$source_dir/plumivox-proxy.service" "$stage/plumivox-proxy.service"
# 既存設定の末尾へ専用Includeを追加する。先頭IncludeからMatchを開始しない。
cat /etc/ssh/sshd_config > "$stage/sshd_config.before"
{ cat "$stage/sshd_config.before"; printf '\nInclude %s\n' "$stage/ssh.conf"; } > "$stage/sshd_config"
sshd -t -f "$stage/sshd_config" >/dev/null 2>&1 || fail '候補SSH設定が不正です'
verify_effective "$(sshd -T -f "$stage/sshd_config" -C "user=$ssh_user,host=localhost,addr=127.0.0.1")"
[[ -r /dev/tty && -w /dev/tty ]] || fail '対話端末が必要です (パスワードを引数や環境変数へ渡せません)'
printf '32文字以上のランダムなプロキシパスワードを入力してください: ' >/dev/tty
IFS= read -r -s password </dev/tty
printf '\n確認のため再入力: ' >/dev/tty
IFS= read -r -s confirmation </dev/tty
printf '\n' >/dev/tty
[[ ${#password} -ge 32 && ${#password} -le 72 && $password == "$confirmation" && $password =~ ^[[:graph:]]+$ ]] || fail 'パスワードは同一の32～72文字の半角印字可能文字で入力してください'
# aptによる標準Squidの自動起動を事前に抑止する。管理者の既存maskには触れない。
systemctl mask squid.service >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends squid apache2-utils
[[ -x /usr/lib/squid/basic_ncsa_auth ]] || fail '対応するSquid認証ヘルパーがありません'
printf '%s\n' "$password" | htpasswd -i -B -c "$stage/passwd" "$proxy_user" >/dev/null 2>&1
unset password confirmation
/usr/sbin/squid -k parse -f "$stage/squid.conf" >/dev/null 2>&1 || fail '候補Squid設定の検証に失敗しました'
systemd-analyze verify "$stage/plumivox-proxy.service" >/dev/null 2>&1 || fail '候補systemd設定の検証に失敗しました'
cmp -s /etc/ssh/sshd_config "$stage/sshd_config.before" || fail '準備中にSSH設定が変更されました'
[[ $(ufw status | head -n1) == 'Status: active' ]] || fail '準備中にUFWの状態が変わりました'
for port in "$public_port" "$remote_port"; do
  [[ -z $(ss -H -ltn "sport = :$port") ]] || fail '準備中にポートが使用されました'
done
# 作成後に一部失敗した場合はmanifestを作らず、再実行では変更を拒否する。
install -d -o root -g root -m 0755 "$base" "$home"
install -d -o root -g root -m 0755 "$home/.ssh"
useradd --user-group --no-create-home --home-dir "$home" --shell /usr/sbin/nologin --password '*' "$ssh_user"
install -o root -g root -m 0600 "$stage/passwd" "$base/passwd"
install -o root -g root -m 0644 "$stage/squid.conf" "$base/squid.conf"
install -o root -g root -m 0600 "$stage/options" "$base/options"
install -o root -g root -m 0644 "$stage/authorized_keys" "$home/.ssh/authorized_keys"
install -o root -g root -m 0644 "$stage/plumivox-proxy.service" "$unit"
install -o root -g root -m 0644 "$stage/ssh.conf" "$ssh_conf"
install -o root -g root -m 0600 "$stage/sshd_config.before" "$base/sshd_config.before"
{ cat "$stage/sshd_config.before"; printf '\nInclude %s\n' "$ssh_conf"; } > "$stage/sshd_config.final"
install -o root -g root -m 0644 "$stage/sshd_config.final" /etc/ssh/sshd_config
sshd -t >/dev/null 2>&1 || fail '実際のSSH設定の検証に失敗しました (reloadは実行していません)'
verify_effective "$(sshd -T -C "user=$ssh_user,host=localhost,addr=127.0.0.1")"
systemctl reload ssh.service
systemctl daemon-reload
systemctl enable --now plumivox-proxy.service >/dev/null
sleep 2
systemctl is-active --quiet plumivox-proxy.service || fail 'プロキシ起動を確認できません。管理者が原因を確認してください'
ufw allow proto tcp from 0.0.0.0/0 to any port "$public_port" comment 'plumivox-proxy' >/dev/null
sha256sum "$base/options" "$base/squid.conf" "$base/passwd" "$unit" "$ssh_conf" "$home/.ssh/authorized_keys" /etc/ssh/sshd_config "$base/sshd_config.before" > "$base/manifest.sha256"
chmod 0600 "$base/manifest.sha256"
printf '導入完了: 公開TCP %s / 逆トンネル127.0.0.1:%s / SSHユーザー %s / 認証ユーザー %s\n' "$public_port" "$remote_port" "$ssh_user" "$proxy_user"
printf 'VPS提供元のファイアウォールで公開TCP %sを手動で許可してください。\n' "$public_port"
show_fingerprints
