#!/usr/bin/env bash
# Debian 12+/Ubuntu 22.04+専用。専用VPSへ対話的に導入する。
set +x +v +a
set -euo pipefail
export LC_ALL=C
umask 077
fail() { printf 'エラー: %s\n' "$*" >&2; exit 1; }
unset -v input_snapshot template_snapshot service_snapshot public_key_snapshot 2>/dev/null || fail '入力snapshot変数を安全に初期化できません'
export -n input_snapshot template_snapshot service_snapshot public_key_snapshot
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
Bashでの新規導入には--exchange/--no-copy対応のGNU coreutils、getfattr、getfacl、lsattrが必要です。
未対応・未導入の場合は管理者が準備します。自動更新・追加runtimeの導入は行いません。
ACL・拡張属性・SELinuxラベルの保持を保証できない環境では新規導入を拒否します。
導入中は同時のSSH管理操作を停止し、退避原本は旧FD利用終了と新旧内容・管理接続確認後に手動整理します。
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
# help・引数検証・対象OSの拒否はmacOS標準のBash 3.2でも実行できる。
# Linuxでの導入は連想配列と可変FDを使うためBash 4.1以降に限定する。
((BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 1))) || fail 'Linuxでの導入にはBash 4.1以降が必要です'
declare -A bash_dir_fds=() bash_dir_identities=()
declare -a bash_dir_order=()
bash_pin_holder=$BASHPID
bash_pins_ready=0
# os-releaseはOS管理ファイル。
. /etc/os-release
case "${ID:-}:${VERSION_ID:-}" in
  debian:*) [[ ${VERSION_ID%%.*} =~ ^[0-9]+$ ]] && ((${VERSION_ID%%.*} >= 12)) || fail 'Debian 12以降が必要です' ;;
  ubuntu:*) [[ $VERSION_ID =~ ^[0-9]+\.[0-9]+$ ]] && [[ $(printf '%s\n' 22.04 "$VERSION_ID" | sort -V | head -n1) == 22.04 ]] || fail 'Ubuntu 22.04以降が必要です' ;;
  *) fail 'Debian 12+またはUbuntu 22.04+が必要です' ;;
esac
for command in systemctl systemd-analyze sshd ssh-keygen ufw ss sha256sum stat getent awk sed grep install useradd apt-get dpkg-query flock cmp mktemp base64 dirname; do
  command -v "$command" >/dev/null || fail "必要なコマンドがありません: $command (SSH・UFW・iproute2を管理者が準備してください)"
done
systemctl is-active --quiet ssh.service || fail 'ssh.serviceが起動している必要があります'
[[ $(ufw status | head -n1) == 'Status: active' ]] || fail '既存のUFWが有効である必要があります。SSH管理アクセスを許可・確認したうえで管理者が有効化してください'
[[ -f /etc/ssh/sshd_config && ! -L /etc/ssh/sshd_config ]] || fail '通常ファイルのsshd_configが必要です'
sshd -t >/dev/null 2>&1 || fail '現在のsshd_configの検証に失敗しました'
input_caller_uid=${SUDO_UID:-$EUID}
[[ $input_caller_uid =~ ^[0-9]{1,10}$ ]] && ((10#$input_caller_uid <= 4294967295)) || fail 'sudo呼出し本人のUIDを確認できません'
input_caller_uid=$((10#$input_caller_uid))
bash_input_permissions() {
  local uid=$1 mode=$2 directory=$3
  [[ $uid == 0 || $uid == "$input_caller_uid" ]] || return 1
  [[ $mode =~ ^[0-7]{3,4}$ ]] || return 1
  (( (8#$mode & 022) == 0 || (directory == 1 && (8#$mode & 01000) != 0) ))
}
bash_input_snapshot_file() {
  local path=$1 parent current='/' part info uid mode fd descriptor identity
  local -a parts
  [[ $path == /* ]] || path=$(pwd -P)/$path
  parent=$(dirname -- "$path") || fail '入力元の親を確認できません'
  IFS=/ read -r -a parts <<<"$parent"
  for part in '' "${parts[@]}"; do
    [[ $part != .. ]] || fail '入力元の祖先を安全に確認できないパスです'
    [[ $part != . ]] || continue
    if [[ -n $part ]]; then current=${current%/}/$part; fi
    [[ -d $current && ! -L $current ]] || fail '入力元の祖先が通常ディレクトリではありません'
    info=$(stat -c '%u:%a' -- "$current") || fail '入力元の祖先を確認できません'
    IFS=: read -r uid mode <<<"$info"
    bash_input_permissions "$uid" "$mode" 1 || fail '入力元の祖先は本人/root所有ではないか第三者が差替可能です'
  done
  path=${current%/}/${path##*/}
  [[ -f $path && ! -L $path ]] || fail '入力元はリンクでない通常ファイルにしてください'
  info=$(stat -c '%u:%a' -- "$path") || fail '入力元の所有者・権限を確認できません'
  IFS=: read -r uid mode <<<"$info"
  bash_input_permissions "$uid" "$mode" 0 || fail '入力元は本人/root所有ではないか第三者が書込み可能です'
  exec {fd}< "$path" || fail '入力元を安全に開けません'
  descriptor=/proc/$BASHPID/fd/$fd
  identity=$(stat -L -c '%d:%i:%u:%g:%a:%s:%y:%z' "$descriptor") || fail '入力元FDを確認できません'
  [[ ! -L $path && $(stat -c '%d:%i:%u:%g:%a:%s:%y:%z' "$path") == "$identity" ]] || fail '入力元の参照先が変わりました'
  input_snapshot=$(base64 --wrap=0 "$descriptor" 2>/dev/null) || fail '入力元snapshotを読み取れません'
  [[ $(stat -L -c '%d:%i:%u:%g:%a:%s:%y:%z' "$descriptor") == "$identity" ]] || fail '読取り中に入力元が変更されました'
  exec {fd}<&-
}
bash_input_snapshot_file "$key_file"
public_key_snapshot=$input_snapshot
bash_public_key_from_snapshot() {
  key=$(printf '%s' "$1" | base64 --decode | awk 'NF {if(++n>1 || $1!="ssh-ed25519" || NF<2)exit 1; print $1" "$2} END{if(n!=1)exit 1}') || fail '公開鍵は一行のEd25519公開鍵にしてください'
  printf '%s\n' "$key" | ssh-keygen -l -f /dev/stdin >/dev/null 2>&1 || fail '公開鍵が不正です'
}
bash_public_key_from_snapshot "$public_key_snapshot"
base=/etc/plumivox-proxy
home=/var/lib/plumivox-ssh
unit=/etc/systemd/system/plumivox-proxy.service
ssh_conf=$base/ssh.conf
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
[[ -f $source_dir/squid.conf.in && -f $source_dir/plumivox-proxy.service ]] || fail '配布ファイルが不足しています'
bash_input_snapshot_file "$source_dir/squid.conf.in"
template_snapshot=$input_snapshot
bash_input_snapshot_file "$source_dir/plumivox-proxy.service"
service_snapshot=$input_snapshot
render_squid() {
  printf '%s' "$template_snapshot" | base64 --decode | sed -e "s/@PUBLIC_PORT@/$public_port/g" -e "s/@REMOTE_PORT@/$remote_port/g"
}
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
  cmp -s <(printf '%s' "$service_snapshot" | base64 --decode) "$unit" || fail '配布サービスが既存導入と異なります'
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
# sshdは継承FDを閉じるため、親Bashが保持するパイプをprocfs経由で渡す。
# コマンド置換の中でも実際の保持元PIDを使い、候補の一時ファイルを作らない。
candidate_sshd_command() {
  local candidate_fd status=0
  exec {candidate_fd}< <(candidate_sshd)
  sshd -f "/proc/$BASHPID/fd/$candidate_fd" "$@" || status=$?
  exec {candidate_fd}<&-
  return "$status"
}
if ((!owned)); then
  candidate_sshd_command -t >/dev/null 2>&1 || fail '候補SSH設定が不正です'
  verify_effective "$(candidate_sshd_command -T -C "user=$ssh_user,host=localhost,addr=127.0.0.1")"
  # 新規ユーザー以外の代表コンテキストに変更がないことも確認する。
  admin_user=plumivox-admin-check
  [[ $admin_user != "$ssh_user" ]] || admin_user=plumivox-admin-check2
  [[ $(sshd -T -C "user=$admin_user,host=localhost,addr=127.0.0.1") == "$(candidate_sshd_command -T -C "user=$admin_user,host=localhost,addr=127.0.0.1")" ]] || fail '既存SSHの他ユーザー設定へ影響するため導入を拒否しました'
fi
bash_ssh_metadata() {
  stat -L -c '%d:%i:%u:%g:%a:%h:%s:%y' -- "$1" 2>/dev/null
}
bash_ssh_hash() {
  sha256sum -- "$1" 2>/dev/null | awk '{print $1}'
}
bash_ssh_matches() {
  [[ -f $1 && ! -L $1 ]] && [[ $(bash_ssh_metadata "$1") == "$2" && $(bash_ssh_hash "$1") == "$3" ]]
}
bash_require_exchange_tools() {
  local command version usage
  for command in mv getfattr getfacl sync readlink lsattr ln mkdir chmod dirname rm rmdir cat cp sleep; do
    command -v "$command" >/dev/null || fail "安全なBash導入に必要なコマンドがありません: $command。管理者が準備してください (自動導入しません)"
  done
  version=$(mv --version 2>/dev/null) || fail 'GNU mvの形式を確認できません'
  usage=$(mv --help 2>/dev/null) || fail 'GNU mvの交換機能を確認できません'
  [[ $version == 'mv (GNU coreutils)'* && $usage == *--exchange* && $usage == *--no-copy* ]] || fail 'このGNU mvは安全な--exchange/--no-copyに未対応です。対応するcoreutilsを管理者が準備してください (自動更新しません)'
  usage=$(ln --help 2>/dev/null) || fail '新規保存に必要なlnの形式を確認できません'
  [[ $usage == *--no-target-directory* ]] || fail '新規保存に必要なGNU lnの-Tに対応していません'
}
bash_no_xattrs() {
  local attributes diagnostic
  attributes=$(getfattr --absolute-names -d -m - -- "$1" 2>/dev/null) || fail 'SSH設定の拡張属性を確認できないため導入しません'
  awk '!/^#/ && NF {found=1} END{exit found ? 1 : 0}' <<<"$attributes" || fail 'SSH設定にACLまたは拡張属性があります。保持を保証できないため導入しません'
  # 一部のgetfattrは一覧のENOTSUPを空一覧として扱う。読取りだけの名前指定でも対応を確認する。
  if diagnostic=$(getfattr -n user.plumivox_readonly_attribute_probe -- "$1" 2>&1); then
    fail 'SSH設定の属性一覧を完全に確認できないため導入しません'
  else
    [[ $diagnostic == *': No such attribute' || $diagnostic == *': No data available' ]] || fail 'このファイルシステムの拡張属性の読取りを確認できないため導入しません'
  fi
}
bash_no_default_acl() {
  local attributes
  attributes=$(getfacl -c -p -d -- "$1" 2>/dev/null) || fail 'SSH設定ディレクトリのdefault ACLを確認できないため導入しません'
  [[ -z $attributes ]] || fail 'SSH設定ディレクトリにdefault ACLがあります。権限拡大を避けるため導入しません'
}
bash_directory_permissions() {
  local uid=$1 gid=$2 mode=$3 temporary=$4
  [[ $uid == 0 && $gid == 0 && $mode =~ ^[0-7]{3,4}$ ]] || return 1
  (( (8#$mode & 06000) == 0 )) || return 1
  if [[ $temporary == 1 ]]; then (( (8#$mode & 022) == 0 || (8#$mode & 01000) != 0 )); else (( (8#$mode & 01022) == 0 )); fi
}
bash_no_access_acl() {
  local acl
  acl=$(getfacl -c -p -a -- "$1" 2>/dev/null) || fail '祖先ディレクトリのaccess ACLを確認できません'
  awk '/^(user::|group::|other::)[rwx-][rwx-][rwx-]$/ {n++;next} NF {bad=1} END {exit bad || n!=3}' <<<"$acl" || fail '祖先ディレクトリに安全を確認できないaccess ACLがあります'
}
bash_pin_directory_chain() {
  local path=$1 current='/' part uid gid mode info descriptor fd previous temporary
  local -a parts
  [[ $path == /* ]] || fail '保存先の祖先パスが不正です'
  IFS=/ read -r -a parts <<<"$path"
  for part in '' "${parts[@]}"; do
    [[ $part != . && $part != .. ]] || fail '保存先の祖先パスが不正です'
    if [[ -n $part ]]; then current=${current%/}/$part; fi
    [[ -d $current && ! -L $current ]] || fail '保存先の祖先が通常ディレクトリではありません'
    info=$(stat -c '%u:%g:%a' -- "$current") || fail '祖先の所有者・権限を確認できません'
    IFS=: read -r uid gid mode <<<"$info"
    temporary=0; [[ $current != /tmp ]] || temporary=1
    bash_directory_permissions "$uid" "$gid" "$mode" "$temporary" || fail '保存先の祖先が非rootから差替可能です。stickyなしの書込み可能な/tmpを含め導入しません'
    exec {fd}< "$current" || fail '祖先ディレクトリを安全に保持できません'
    descriptor=/proc/$bash_pin_holder/fd/$fd
    info=$(stat -L -c '%d:%i:%u:%g:%a' -- "$descriptor") || fail '祖先の保持FDを確認できません'
    [[ ! -L $current && $(stat -c '%d:%i:%u:%g:%a' -- "$current") == "$info" ]] || fail '保持中に祖先の参照先が変わりました'
    bash_no_access_acl "$descriptor"
    bash_no_default_acl "$descriptor"
    previous=${bash_dir_identities[$current]:-}
    if [[ -n $previous ]]; then
      [[ $previous == "$info" ]] || fail '検査済みの祖先ディレクトリが変更されました'
      exec {fd}<&-
    else
      bash_dir_fds[$current]=$fd
      bash_dir_identities[$current]=$info
      bash_dir_order+=("$current")
    fi
  done
}
bash_revalidate_directory_pins() {
  local path descriptor info uid gid mode temporary
  [[ ${bash_pins_ready:-0} == 1 ]] || return 0
  for path in "${bash_dir_order[@]}"; do
    descriptor=/proc/$bash_pin_holder/fd/${bash_dir_fds[$path]}
    info=$(stat -L -c '%d:%i:%u:%g:%a' -- "$descriptor") || fail '保持した祖先を確認できません'
    [[ -d $path && ! -L $path && $(stat -c '%d:%i:%u:%g:%a' -- "$path") == "$info" && $info == "${bash_dir_identities[$path]}" ]] || fail '検査済み祖先の同一参照を確認できません'
    IFS=: read -r uid gid mode <<<"$(stat -L -c '%u:%g:%a' "$descriptor")"
    temporary=0; [[ $path != /tmp ]] || temporary=1
    bash_directory_permissions "$uid" "$gid" "$mode" "$temporary" || fail '祖先の書込み権限が変更されました'
    bash_no_access_acl "$descriptor"
    bash_no_default_acl "$descriptor"
  done
}
bash_validate_initial_user_namespace() {
  [[ $1 == 6e736673 && $2 == 4026531837 && $3 == 'user:[4026531837]' ]]
}
bash_initial_user_namespace() {
  local namespace_fd descriptor magic inode kind status=0
  exec {namespace_fd}< "/proc/$BASHPID/ns/user" || fail 'user namespaceを安全に開けません'
  descriptor=/proc/$BASHPID/fd/$namespace_fd
  magic=$(stat -L -f -c '%t' "$descriptor" 2>/dev/null) || status=1
  inode=$(stat -L -c '%i' "$descriptor" 2>/dev/null) || status=1
  kind=$(readlink -- "$descriptor" 2>/dev/null) || status=1
  exec {namespace_fd}<&-
  ((status == 0)) && bash_validate_initial_user_namespace "$magic" "$inode" "$kind" || fail '初期user namespaceを確認できません。全拡張属性を保証できないため新規導入しません'
}
bash_mount_id_from_info() {
  local line value rest count=0 result=''
  while IFS= read -r line; do
    [[ $line == mnt_id:* ]] || continue
    read -r value rest <<<"${line#mnt_id:}"
    [[ $value =~ ^[1-9][0-9]{0,9}$ && -z $rest ]] || return 1
    result=$value
    ((count+=1))
  done <<<"$1"
  ((count == 1)) || return 1
  printf '%s\n' "$result"
}
bash_same_mount() {
  local target=$1 parent=$2 target_fd parent_fd descriptor_target descriptor_parent first='' second='' status=0 holder_pid=$BASHPID
  [[ -f $target && ! -L $target && -d $parent && ! -L $parent ]] || fail 'mount確認の対象形式が不正です'
  exec {target_fd}< "$target" || fail 'mount確認の対象を安全に開けません'
  exec {parent_fd}< "$parent" || fail 'mount確認の親ディレクトリを開けません'
  descriptor_target=/proc/$BASHPID/fd/$target_fd
  descriptor_parent=/proc/$BASHPID/fd/$parent_fd
  [[ $(bash_ssh_metadata "$descriptor_target") == "$(bash_ssh_metadata "$target")" && $(bash_ssh_metadata "$descriptor_parent") == "$(bash_ssh_metadata "$parent")" ]] || status=1
  first=$(bash_mount_id_from_info "$(cat "/proc/$holder_pid/fdinfo/$target_fd" 2>/dev/null)") || status=1
  second=$(bash_mount_id_from_info "$(cat "/proc/$holder_pid/fdinfo/$parent_fd" 2>/dev/null)") || status=1
  exec {target_fd}<&-
  exec {parent_fd}<&-
  ((status == 0)) && [[ $first == "$second" ]] || fail 'SSH設定と親のmount identityが不一致または確認不能です。bind mountを含め新規導入を拒否します'
}
bash_allowed_inode_flags() {
  local flags=$1
  [[ ${#flags} -ge 16 && ${#flags} -le 32 ]] || return 1
  if [[ ${2:-file} == directory ]]; then [[ $flags != *[!eI-]* ]]; else [[ $flags != *[!e-]* ]]; fi
}
bash_no_special_inode_flags() {
  local output flags rest
  output=$(lsattr -d -- "$1" 2>/dev/null) || fail 'SSH設定のinode flagsを確認できないため新規導入しません'
  [[ $output != *$'\n'* ]] || fail 'SSH設定のinode flags形式を確認できません'
  read -r flags rest <<<"$output"
  bash_allowed_inode_flags "$flags" "${2:-file}" || fail 'SSH設定にimmutable・nodump等の保持不能なinode flagsがあります。新規導入しません'
}
bash_creation_parent_attributes() {
  bash_no_default_acl "$1"
  bash_no_special_inode_flags "$1" directory
}
bash_verify_creation_parent() {
  local path=$1 mode
  if [[ ${bash_pins_ready:-0} == 1 ]]; then bash_revalidate_directory_pins; bash_pin_directory_chain "$path"; fi
  [[ -d $path && ! -L $path && $(stat -c '%u:%g' -- "$path") == 0:0 ]] || fail '新規保存の既存親ディレクトリの所有者・形式が不正です'
  mode=$(stat -c '%a' -- "$path") || fail '新規保存の既存親ディレクトリの権限を確認できません'
  (( (8#$mode & 07022) == 0 )) || fail '新規保存の既存親ディレクトリの権限が安全ではありません'
  bash_creation_parent_attributes "$path"
}
bash_parent_reference_matches() {
  [[ -d $1 && ! -L $1 && -d $2 && $(stat -c '%d:%i' -- "$1") == "$(stat -L -c '%d:%i' -- "$2")" ]]
}
bash_retained_path() {
  local parent
  parent=$(readlink -- "$1" 2>/dev/null) || fail '保持FDの実保存先を表示できません。自動削除せず管理者が確認してください'
  printf '%s/%s\n' "${parent%/}" "$2"
}
bash_mkdir_new() {
  local target=$1 mode=$2 parent parent_reference anchored
  [[ $mode =~ ^[0-7]{3,4}$ ]] || fail '新規ディレクトリの権限指定が不正です'
  [[ ! -e $target && ! -L $target ]] || fail "同名ディレクトリが既存です。権限を変更しません: $target"
  parent=$(dirname -- "$target")
  bash_verify_creation_parent "$parent"
  [[ -n ${bash_dir_fds[$parent]:-} ]] || fail '新規作成の親FDを保持できません'
  parent_reference=/proc/$bash_pin_holder/fd/${bash_dir_fds[$parent]}
  anchored=$parent_reference/${target##*/}
  mkdir -m "$mode" -- "$anchored" 2>/dev/null || fail "同名ディレクトリが既存または作成不可です。権限を変更しません: $target"
  bash_parent_reference_matches "$parent" "$parent_reference" || fail "作成先の親参照が変わりました。保持先: $(bash_retained_path "$parent_reference" "${target##*/}")。後続処理は実行しません"
  [[ -d $anchored && ! -L $anchored && $(stat -c '%u:%g:%a' -- "$anchored") == "0:0:$mode" ]] || fail '新規ディレクトリの所有者・権限を確認できません'
  [[ $(stat -c '%d:%i' -- "$target") == "$(stat -c '%d:%i' -- "$anchored")" ]] || fail '作成したディレクトリの実名を確認できません'
  if [[ ${bash_pins_ready:-0} == 1 ]]; then bash_pin_directory_chain "$target"; fi
}
bash_publish_new() {
  local candidate=$1 target=$2 descriptor=$3 identity hash parent
  if [[ ${bash_pins_ready:-0} == 1 ]]; then
    bash_revalidate_directory_pins
    parent=$(dirname -- "$target")
    bash_pin_directory_chain "$parent"
    target=/proc/$bash_pin_holder/fd/${bash_dir_fds[$parent]}/${target##*/}
  fi
  # link(2)の既存名拒否を使う。FIFO・symlink・directoryも開かず上書きしない。
  ln -T -- "$candidate" "$target" 2>/dev/null || fail "新規ファイルのno-replace公開に失敗しました。既存を上書きしません。候補保持先: $candidate"
  identity=$(bash_ssh_metadata "$descriptor") hash=$(bash_ssh_hash "$descriptor")
  bash_ssh_matches "$target" "$identity" "$hash" || fail "公開したファイルの参照先を確認できません。候補保持先: $candidate"
}
bash_write_new() {
  local source=$1 target=$2 mode=$3 file_fd descriptor work candidate identity hash parent parent_reference work_fd work_reference work_identity target_reference
  [[ $mode =~ ^[0-7]{3,4}$ ]] || fail '新規ファイルの権限指定が不正です'
  parent=$(dirname -- "$target")
  bash_verify_creation_parent "$parent"
  [[ -n ${bash_dir_fds[$parent]:-} ]] || fail '新規保存の親FDを保持できません'
  parent_reference=/proc/$bash_pin_holder/fd/${bash_dir_fds[$parent]}
  target_reference=$parent_reference/${target##*/}
  work=$(mktemp -d "$parent_reference/.plumivox-file.XXXXXXXX") || fail '新規保存の候補ディレクトリを作成できません'
  new_file_candidate_path=$(bash_retained_path "$parent_reference" "${work##*/}")
  bash_parent_reference_matches "$parent" "$parent_reference" || fail "作成先の親参照が変わりました。保持先: $new_file_candidate_path。後続処理は実行しません"
  [[ -d $work && ! -L $work && $(stat -c '%u:%g:%a' "$work") == 0:0:700 ]] || fail "新規保存先の所有者・権限が不正です。保持先: $work"
  work_identity=$(stat -c '%d:%i:%u:%g:%a' "$work") || fail '新規保存先の実体を確認できません'
  exec {work_fd}< "$work" || fail '新規保存先をFDで保持できません'
  work_reference=/proc/$BASHPID/fd/$work_fd
  [[ $(stat -L -c '%d:%i:%u:%g:%a' "$work_reference") == "$work_identity" ]] || fail '新規保存先のFDと実名が一致しません'
  bash_no_xattrs "$work"
  candidate=$(mktemp "$work_reference/file.XXXXXXXX") || fail "新規候補を排他的に作成できません。保持先: $new_file_candidate_path"
  new_file_candidate_path=$(bash_retained_path "$work_reference" "${candidate##*/}")
  exec {file_fd}> "$candidate" || fail "新規候補を開けません。保持先: $candidate"
  descriptor=/proc/$BASHPID/fd/$file_fd
  [[ $(stat -L -c '%u:%g:%h' "$descriptor") == 0:0:1 ]] || fail '新規ファイルの所有者・link数を確認できません'
  bash_no_xattrs "$descriptor"
  cat -- "$source" >&"$file_fd" || fail '新規ファイルの書込みに失敗しました'
  chmod "$mode" "$descriptor" || fail '新規ファイルの権限設定に失敗しました'
  [[ $(stat -L -c '%u:%g:%a' "$descriptor") == "0:0:$mode" ]] || fail '新規ファイルの保存権限を確認できません'
  bash_no_xattrs "$descriptor"
  sync "$descriptor" || fail '新規ファイルの保存を確定できません'
  identity=$(bash_ssh_metadata "$descriptor") hash=$(bash_ssh_hash "$descriptor")
  bash_ssh_matches "$candidate" "$identity" "$hash" || fail "新規候補の参照先が変更されました。保持先: $candidate"
  bash_publish_new "$candidate" "$target" "$descriptor"
  bash_parent_reference_matches "$parent" "$parent_reference" || fail "新規ファイル公開後の親参照が不明です。候補保持先: $new_file_candidate_path"
  sync "$target_reference" "$parent_reference" || fail "新規ファイルは公開済みですが保存確定を確認できません。候補保持先: $new_file_candidate_path"
  # 公開失敗・状態不明時は候補を残す。正常時だけ、同じinodeの自分の追加名を整理する。
  identity=$(bash_ssh_metadata "$descriptor") hash=$(bash_ssh_hash "$descriptor")
  bash_ssh_matches "$candidate" "$identity" "$hash" || fail "公開後の候補状態が不明です。保持先: $candidate"
  rm -- "$candidate" || fail "新規ファイルは公開済みですが候補を清掃できません。保持先: $candidate"
  new_file_candidate_path=$(bash_retained_path "$parent_reference" "${work##*/}")
  [[ $(stat -c '%d:%i:%u:%g:%a' "$work") == "$work_identity" ]] || fail "候補ディレクトリの実名が変わりました。保持先: $new_file_candidate_path"
  rmdir -- "$work" || fail "新規ファイルは公開済みですが候補ディレクトリを清掃できません。保持先: $work"
  new_file_candidate_path=''
  exec {file_fd}>&-
  exec {work_fd}<&-
}
bash_verify_ssh_replacement() {
  local capabilities mode path state
  bash_require_exchange_tools
  bash_initial_user_namespace
  [[ $(awk '/^Gid:/{print $3}' "/proc/$BASHPID/status") == 0 ]] || fail '新規ファイルのrootグループ所有を保証できないため導入しません'
  capabilities=$(awk '/^CapEff:/{print $2}' "/proc/$BASHPID/status") || fail 'SSH設定の属性確認に必要なcapabilityを確認できません'
  [[ $capabilities =~ ^[a-fA-F0-9]+$ ]] && (( (16#$capabilities & 0x200000) != 0 )) || fail '全拡張属性の確認に必要なCAP_SYS_ADMINがないため新規導入しません'
  for path in /sys/fs/selinux/enforce /selinux/enforce; do
    if state=$(stat -- "$path" 2>&1); then
      fail 'SELinuxラベルを保持できないため新規導入しません'
    else
      [[ $state == *'No such file or directory'* ]] || fail 'SELinuxの状態を安全に確認できないため新規導入しません'
    fi
  done
  for path in /etc /etc/ssh; do
    [[ -d $path && ! -L $path && $(stat -c '%u:%g' -- "$path") == 0:0 ]] || fail 'SSH設定ディレクトリの所有者・形式が不正です'
    mode=$(stat -c '%a' -- "$path") || fail 'SSH設定ディレクトリの権限を確認できません'
    (( (8#$mode & 07022) == 0 )) || fail 'SSH設定ディレクトリの権限が安全ではありません'
  done
  [[ -f /etc/ssh/sshd_config && ! -L /etc/ssh/sshd_config && $(stat -c '%u:%g:%h' /etc/ssh/sshd_config) == 0:0:1 ]] || fail 'SSH設定の所有者・形式・link数が不正です'
  mode=$(stat -c '%a' /etc/ssh/sshd_config) || fail 'SSH設定の権限を確認できません'
  (( (8#$mode & 07022) == 0 )) || fail 'SSH設定が別の利用者から書込み可能です'
  bash_no_xattrs /etc/ssh/sshd_config
  bash_no_default_acl /etc/ssh
  bash_no_special_inode_flags /etc/ssh/sshd_config
  bash_no_special_inode_flags /etc/ssh directory
  bash_same_mount /etc/ssh/sshd_config /etc/ssh
  for path in /etc /var/lib /etc/systemd/system; do bash_verify_creation_parent "$path"; done
  for path in /etc/ssh /etc/systemd/system /var/lib /tmp /run; do bash_pin_directory_chain "$path"; done
  bash_pins_ready=1
  bash_revalidate_directory_pins
}
bash_probe_exchange() {
  local parent=$1 probe probe_meta first_meta second_meta first_hash second_hash status=0 path
  probe=$(mktemp -d "$parent/.plumivox-exchange-check.XXXXXXXX") || fail '交換機能の検査先を作成できません'
  probe_meta=$(stat -c '%d:%i:%u:%g:%a' "$probe") || fail "交換検査先の形式を確認できません。保持先: $probe"
  [[ $(stat -c '%a' "$probe") == 700 ]] || fail "交換検査先の権限が不正です。保持先: $probe"
  if ! (bash_no_xattrs "$probe") >/dev/null 2>&1; then
    fail "交換検査先の属性を安全に確認できません。保持先: $probe。SSH設定・サービス・パッケージは変更していません"
  fi
  printf 'Plumivox first probe\n' > "$probe/first"
  printf 'Plumivox second probe\n' > "$probe/second"
  first_meta=$(bash_ssh_metadata "$probe/first") first_hash=$(bash_ssh_hash "$probe/first")
  second_meta=$(bash_ssh_metadata "$probe/second") second_hash=$(bash_ssh_hash "$probe/second")
  mv --exchange --no-copy -T -- "$probe/first" "$probe/second" >/dev/null 2>&1 || status=$?
  if ((status == 0)); then
    bash_ssh_matches "$probe/first" "$second_meta" "$second_hash" && bash_ssh_matches "$probe/second" "$first_meta" "$first_hash" || fail "交換検査の結果が不明です。検査先を保持します: $probe。SSH設定・サービスは変更していません"
  fi
  # 合成した2候補だけを照合して清掃し、未知の変更は消さない。
  for path in "$probe/first" "$probe/second"; do
    [[ $(stat -c '%d:%i:%u:%g:%a' "$probe") == "$probe_meta" ]] || fail "交換検査ディレクトリが変更されました。保持先: $probe"
    bash_ssh_matches "$path" "$first_meta" "$first_hash" || bash_ssh_matches "$path" "$second_meta" "$second_hash" || fail "交換検査先が変更されました。保持先: $probe"
    rm -- "$path" || fail "交換検査先を清掃できません。保持先: $probe"
  done
  [[ $(stat -c '%d:%i:%u:%g:%a' "$probe") == "$probe_meta" ]] || fail "交換検査ディレクトリが変更されました。保持先: $probe"
  rmdir -- "$probe" || fail "交換検査ディレクトリを清掃できません。保持先: $probe"
  ((status == 0)) || fail 'このファイルシステムでは原子的な交換を確認できません。SSH設定・サービス・パッケージは変更していません'
}
bash_atomic_replace_ssh() {
  local target=$1 before=$2 candidate_source=$3 expected_meta=$4 parent mode original_hash candidate_meta candidate_hash status=0 candidate_fd descriptor backup_meta restore_noclobber=0 ignored original_flags candidate_flags
  local logical_target=$1 parent_reference parent_fd backup_fd backup_reference backup_descriptor ssh_candidate
  ssh_exchange_state=preparing
  if [[ ${bash_pins_ready:-0} == 1 ]]; then bash_revalidate_directory_pins; fi
  parent=$(dirname -- "$target")
  if [[ ${bash_pins_ready:-0} == 1 ]]; then
    [[ -n ${bash_dir_fds[$parent]:-} ]] || fail 'SSH交換の親FDを保持できません'
    parent_reference=/proc/$bash_pin_holder/fd/${bash_dir_fds[$parent]}
  else
    exec {parent_fd}< "$parent" || fail 'SSH交換の親を保持できません'
    parent_reference=/proc/$BASHPID/fd/$parent_fd
  fi
  bash_parent_reference_matches "$parent" "$parent_reference" || fail 'SSH交換の親参照が不明です。変更しません'
  target=$parent_reference/${logical_target##*/}
  original_hash=$(bash_ssh_hash "$before") || fail 'SSH原本のハッシュを確認できません'
  bash_ssh_matches "$target" "$expected_meta" "$original_hash" || fail 'SSH設定が保存直前に変更されました。交換しません'
  bash_no_xattrs "$target"
  bash_no_special_inode_flags "$target"
  bash_no_special_inode_flags "$parent" directory
  bash_same_mount "$target" "$parent"
  bash_no_default_acl "$parent"
  backup_reference=$(mktemp -d "$parent_reference/.plumivox-sshd-backup.XXXXXXXX") || fail 'SSH原本のroot限定退避先を作成できません'
  ssh_backup_dir=$(bash_retained_path "$parent_reference" "${backup_reference##*/}")
  ssh_backup_path=$ssh_backup_dir/sshd_config.before
  bash_parent_reference_matches "$parent" "$parent_reference" || fail "SSH保存先の親参照が不明です。候補ディレクトリ保持先: $ssh_backup_dir。reloadは実行していません"
  backup_meta=$(stat -c '%d:%i:%u:%g:%a' "$backup_reference") || fail 'SSH退避先の形式を確認できません'
  [[ $(stat -c '%a' "$backup_reference") == 700 ]] || fail 'SSH退避先の権限が不正です'
  exec {backup_fd}< "$backup_reference" || fail 'SSH退避先をFDで保持できません'
  backup_descriptor=/proc/$BASHPID/fd/$backup_fd
  [[ $(stat -L -c '%d:%i:%u:%g:%a' "$backup_descriptor") == "$backup_meta" ]] || fail 'SSH退避先の実名とFDが一致しません'
  backup_reference=$backup_descriptor/.
  ssh_candidate=$backup_reference/sshd_config.before
  bash_no_xattrs "$backup_reference"
  # 新規ファイルだけを排他的に開き、書込みは保持したFDへ行う。
  [[ -o noclobber ]] || restore_noclobber=1
  set -C
  exec {candidate_fd}> "$ssh_candidate" || fail 'SSH候補設定を排他的に作成できません'
  if ((restore_noclobber)); then set +C; fi
  descriptor=/proc/$BASHPID/fd/$candidate_fd
  bash_no_xattrs "$ssh_candidate"
  cat -- "$candidate_source" >&"$candidate_fd" || fail 'SSH候補設定の書込みに失敗しました'
  IFS=: read -r ignored ignored ignored ignored mode ignored <<<"$expected_meta"
  [[ $mode =~ ^[0-7]{3,4}$ ]] || fail 'SSH設定の元の権限を確認できません'
  chmod "$mode" "$descriptor" || fail 'SSH候補設定の元の権限を保持できません'
  bash_no_xattrs "$ssh_candidate"
  [[ $(stat -L -c '%u:%g:%a' "$descriptor") == "$(stat -c '%u:%g:%a' "$target")" ]] || fail 'SSH候補設定の所有者・権限を保持できません'
  sync "$ssh_candidate" "$backup_reference" || fail 'SSH候補設定の保存を確定できません'
  candidate_meta=$(bash_ssh_metadata "$descriptor") candidate_hash=$(bash_ssh_hash "$descriptor")
  bash_ssh_matches "$ssh_candidate" "$candidate_meta" "$candidate_hash" || fail 'SSH候補設定の参照先が変わりました'
  [[ $(stat -c '%d:%i:%u:%g:%a' "$backup_reference") == "$backup_meta" ]] || fail 'SSH退避先が変わりました'
  bash_no_xattrs "$backup_reference"
  bash_no_default_acl "$parent"
  bash_no_xattrs "$target"
  bash_no_special_inode_flags "$target"
  bash_no_special_inode_flags "$ssh_candidate"
  bash_same_mount "$target" "$parent"
  original_flags=$(lsattr -d -- "$target" 2>/dev/null | awk '{print $1}') || fail '交換直前のSSH原本のinode flagsを確認できません'
  candidate_flags=$(lsattr -d -- "$ssh_candidate" 2>/dev/null | awk '{print $1}') || fail '交換直前のSSH候補のinode flagsを確認できません'
  bash_parent_reference_matches "$parent" "$parent_reference" || fail "SSH交換直前の親参照が不明です。保持先: $ssh_backup_path。reloadは実行していません"
  bash_ssh_matches "$target" "$expected_meta" "$original_hash" || fail '保存直前にSSH設定が変更されました。交換しません'
  ssh_exchange_state=unknown
  mv --exchange --no-copy -T -- "$ssh_candidate" "$target" >/dev/null 2>&1 || status=$?
  exec {candidate_fd}>&-
  ssh_backup_dir=$(readlink -- "$backup_descriptor") || fail 'SSH交換後の退避先実pathを確認できません。両方を保持しreloadは実行していません'
  ssh_backup_path=$ssh_backup_dir/sshd_config.before
  bash_parent_reference_matches "$parent" "$parent_reference" || fail "SSH設定の置換状態不明。親参照が変わりました。退避先: $ssh_backup_path。両方を保持しreloadは実行していません"
  if ((status != 0)); then
    if bash_ssh_matches "$target" "$expected_meta" "$original_hash" && bash_ssh_matches "$ssh_candidate" "$candidate_meta" "$candidate_hash"; then
      ssh_exchange_state=unchanged
      fail "SSH交換が失敗しました。両pathを再照合し元の設定の維持を確認しました。候補保持先: $ssh_backup_path。reloadは実行していません"
    fi
    fail "SSH設定の置換状態不明。退避先: $ssh_backup_path。両pathを自動削除・自動復元しません。reloadは実行していません"
  fi
  bash_ssh_matches "$target" "$candidate_meta" "$candidate_hash" && bash_ssh_matches "$ssh_candidate" "$expected_meta" "$original_hash" || fail "SSH設定の置換状態不明。退避先: $ssh_backup_path。両方を保持しreloadは実行していません"
  if ! (bash_no_xattrs "$target" && bash_no_xattrs "$ssh_candidate" && bash_no_xattrs "$backup_reference" &&
        bash_no_special_inode_flags "$target" && bash_no_special_inode_flags "$ssh_candidate" &&
        [[ $(lsattr -d -- "$target" | awk '{print $1}') == "$candidate_flags" && $(lsattr -d -- "$ssh_candidate" | awk '{print $1}') == "$original_flags" ]]) >/dev/null 2>&1; then
    fail "SSH設定の置換状態不明。交換後の属性・inode flagsを確証できません。退避先: $ssh_backup_path。両方を保持しreloadは実行していません"
  fi
  [[ $(stat -c '%d:%i:%u:%g:%a' "$backup_reference") == "$backup_meta" ]] || fail "SSH退避先の状態不明。保持先: $ssh_backup_path。reloadは実行していません"
  ssh_exchange_state=exchanged
  sync "$backup_reference" "$parent_reference" || fail "SSH設定は置換済みですが保存確定を確認できません。退避先: $ssh_backup_path。reloadは実行していません"
  exec {backup_fd}<&-
  if [[ -n ${parent_fd:-} ]]; then exec {parent_fd}<&-; fi
  # 照合後も旧FDに書かれ得るため、正常時を含め旧inodeを自動清掃しない。
}
collect_fingerprints() {
  local effective line field host_key public derived fingerprint result='' count=0
  effective=$(sshd -T 2>/dev/null) || fail 'SSHホスト鍵の有効設定を確認できません'
  while IFS= read -r line; do
    read -r field host_key <<<"$line"
    case "$field" in
      hostkeyagent) [[ $host_key == none ]] || fail 'HostKeyAgentを使うSSHホスト鍵は安全に照合できません' ;;
      hostcertificate) [[ $host_key == none ]] || fail 'SSHホスト証明書を使う構成は安全に照合できません' ;;
    esac
  done <<<"$effective"
  while IFS= read -r line; do
    read -r field host_key <<<"$line"
    [[ $field == hostkey ]] || continue
    [[ $host_key == /* && ! $host_key =~ [[:space:]] && $host_key != *%* && /$host_key/ != */./* && /$host_key/ != */../* ]] || fail 'SSHホスト鍵のカスタムパスを安全に扱えません'
    [[ -e $host_key || -L $host_key ]] || continue
    [[ -f $host_key && ! -L $host_key && -f $host_key.pub && ! -L $host_key.pub ]] || fail '実効SSHホスト鍵と対応する公開鍵を安全に確認できません'
    public=$(awk 'NF {if(++n>1 || NF<2)exit 1; print $1" "$2} END{if(n!=1)exit 1}' "$host_key.pub") || fail 'SSHホスト公開鍵の形式が不正です'
    derived=$(ssh-keygen -y -P '' -f "$host_key" </dev/null 2>/dev/null) || fail '実効SSHホスト鍵の公開部分を安全に導出できません'
    derived=$(awk 'NF {if(++n>1 || NF<2)exit 1; print $1" "$2} END{if(n!=1)exit 1}' <<<"$derived") || fail '導出したSSHホスト公開部分の形式が不正です'
    [[ $public == "$derived" ]] || fail '実効SSHホスト秘密鍵と公開鍵が一致しません。管理者が確認してください'
    # .pubを再読込みせず、照合済みの固定公開部分だけで指紋を計算する。
    fingerprint=$(printf '%s\n' "$derived" | ssh-keygen -l -E sha256 -f /dev/stdin 2>/dev/null) || fail 'SSHホスト鍵の指紋を取得できません'
    fingerprint=$(awk 'NF {if(++n>1 || NF<4 || $2 !~ /^SHA256:/)exit 1; print $2" "$NF} END{if(n!=1)exit 1}' <<<"$fingerprint") || fail 'SSHホスト鍵の指紋形式が不正です'
    result+="$fingerprint"$'\n'
    ((count+=1))
  done <<<"$effective"
  ((count>0)) || fail '有効なSSHホスト公開鍵の指紋が0件です。管理コンソールとの照合ができません'
  printf 'SSHホスト鍵 (SHA256):\n%s' "$result"
}
# 照合不能・不一致・0件は、無変更成功の表示と変更処理より前に拒否する。
if ((!owned)); then bash_verify_ssh_replacement; fi
fingerprints_before=$(collect_fingerprints) || fail 'SSHホスト鍵の指紋を安全に確認できないため導入しません'
if ((owned)); then
  verify_effective "$effective"
  /usr/sbin/squid -k parse -f "$base/squid.conf" >/dev/null 2>&1 || fail 'Squid設定の検証に失敗しました'
  systemctl is-active --quiet plumivox-proxy.service || fail '既存サービスが停止しています。管理者が原因を確認してください'
  printf '既存導入は同一です。パスワード・公開鍵・設定を保持しました。\n'
elif ((check)); then
  printf '事前確認合格。交換機能は未検査です (--checkでは書込み検査を行いません)。導入時にSquid/apache2-utilsを追加し、候補設定を検証します。\n'
fi
if ((check || owned)); then printf '%s\n' "$fingerprints_before"; exit 0; fi
# この先だけが変更処理。root専用の固定ロックを取り、並行導入を拒否する。
bash_acquire_install_lock() {
  local path=$1 parent descriptor identity
  if [[ ${bash_pins_ready:-0} == 1 ]]; then
    bash_revalidate_directory_pins
    parent=$(dirname -- "$path")
    bash_pin_directory_chain "$parent"
    path=/proc/$bash_pin_holder/fd/${bash_dir_fds[$parent]}/${path##*/}
  fi
  if [[ -e $path || -L $path ]]; then
    [[ -f $path && ! -L $path && $(stat -c '%u:%g:%a:%h:%s' -- "$path") == 0:0:600:1:0 ]] || fail '既存の導入ロックの所有者・形式・内容が不正です。変更しません'
  fi
  # append openは既存内容を切詰めない。安全な/runと保持FDを照合してからflockする。
  exec 9>> "$path" || fail '導入ロックを安全に開けません'
  descriptor=/proc/$BASHPID/fd/9
  [[ -f $descriptor && $(stat -L -c '%u:%g:%a:%h:%s' "$descriptor") == 0:0:600:1:0 ]] || fail '開いた導入ロックの所有者・形式・内容が不正です'
  identity=$(stat -L -c '%d:%i:%u:%g:%a:%h:%s' "$descriptor") || fail '導入ロックFDを確認できません'
  [[ ! -L $path && $(stat -c '%d:%i:%u:%g:%a:%h:%s' "$path") == "$identity" ]] || fail '導入ロックの参照先が変わりました'
  flock -n 9 || fail '別の導入が実行中です'
}
bash_acquire_install_lock /run/plumivox-proxy-install.lock
bash_probe_exchange /etc/ssh
[[ ! -e $base && ! -e $home && ! -e $unit && ! -e $ssh_conf ]] || fail '事前確認後に導入状態が変わりました'
! getent passwd "$ssh_user" >/dev/null && ! getent group "$ssh_user" >/dev/null || fail '事前確認後にユーザーまたはグループが作成されました'
for port in "$public_port" "$remote_port"; do
  [[ -z $(ss -H -ltn "sport = :$port") ]] || fail '事前確認後にポートが使用されました'
done
stage=$(mktemp -d /tmp/plumivox-proxy.XXXXXXXX)
stage_cleanup_path=$stage
stage_identity=$(stat -c '%d:%i:%u:%g:%a' -- "$stage") || fail "一時ディレクトリの形式を確認できません。保持先: $stage"
[[ -d $stage && ! -L $stage && $(stat -c '%u:%a' -- "$stage") == "$EUID:700" ]] || fail "一時ディレクトリの所有者・権限が不正です。保持先: $stage"
installation_completed=0
ssh_exchange_state=not-started
cleanup() {
  local status=$? cleanup_failed=0 notice_failed=0 label current_identity cleanup_stage=${stage_cleanup_path:-$stage}
  # trap内の失敗で後続の残留・退避案内を飛ばさず、同じEXIT trapへ再入しない。
  trap - EXIT
  set +e
  unset -v password confirmation 2>/dev/null || :
  # 導入失敗後もSquidの標準サービスはmaskを維持し、不明な公開口を作らない。
  if current_identity=$(stat -c '%d:%i:%u:%g:%a' -- "$cleanup_stage" 2>&1); then
    if [[ -d $cleanup_stage && ! -L $cleanup_stage && -n ${stage_identity:-} && $current_identity == "$stage_identity" ]]; then
      if ! rm -rf -- "$cleanup_stage" >/dev/null 2>&1; then cleanup_failed=1; fi
      if [[ -e $cleanup_stage || -L $cleanup_stage ]]; then cleanup_failed=1; fi
    else
      cleanup_failed=1
    fi
  elif [[ $current_identity != *'No such file or directory'* ]]; then
    cleanup_failed=1
  fi
  if [[ ${installation_completed:-0} == 1 ]]; then
    label='導入済み'
  elif [[ ${ssh_exchange_state:-not-started} == unknown ]]; then
    label='SSH設定の置換状態不明・導入未完了'
  elif [[ ${ssh_exchange_state:-not-started} == exchanged ]]; then
    label='SSH設定は置換済み・導入未完了'
  else
    label='導入未完了'
  fi
  if ((cleanup_failed)); then
    printf '%s。一時ディレクトリの清掃に失敗しました。資格情報を含むファイルが残っている可能性があります。保持先: %s。自動再試行せず、管理者が所有・内容・利用終了を確認して整理してください。\n' "$label" "$cleanup_stage" >&2 || notice_failed=1
  fi
  if ((status != 0 || cleanup_failed)) && [[ -n ${ssh_backup_path:-} ]]; then
    printf '%s。SSH候補または旧inodeの保持先: %s。自動削除・自動復元は行っていません。同時管理操作と旧FDの利用を終了し、管理者が新旧内容とSSH接続を確認してください。\n' "$label" "$ssh_backup_path" >&2 || notice_failed=1
  fi
  if ((status != 0 || cleanup_failed)) && [[ -n ${new_file_candidate_path:-} ]]; then
    printf '%s。新規保存の候補に資格情報が残る可能性があります。候補保存先: %s。自動削除せず管理者が状態を確認してください。\n' "$label" "$new_file_candidate_path" >&2 || notice_failed=1
  fi
  if ((status == 0 && (cleanup_failed || notice_failed))); then status=1; fi
  exit "$status"
}
trap cleanup EXIT
bash_pin_directory_chain "$stage_cleanup_path"
[[ ${bash_dir_identities[$stage_cleanup_path]} == "$stage_identity" ]] || fail '作成したstageの実体が差し替えられました'
stage=/proc/$bash_pin_holder/fd/${bash_dir_fds[$stage_cleanup_path]}
render_squid > "$stage/squid.conf"
render_ssh > "$stage/ssh.conf"
render_key > "$stage/authorized_keys"
render_options > "$stage/options"
printf '%s' "$service_snapshot" | base64 --decode > "$stage/plumivox-proxy.service" || fail '固定したサービス定義を準備できません'
# 既存設定の末尾へ専用Includeを追加する。先頭IncludeからMatchを開始しない。
ssh_source_meta=$(bash_ssh_metadata /etc/ssh/sshd_config) || fail 'SSH設定の保存前状態を確認できません'
exec {ssh_source_fd}< /etc/ssh/sshd_config
ssh_source_ref=/proc/$BASHPID/fd/$ssh_source_fd
[[ $(bash_ssh_metadata "$ssh_source_ref") == "$ssh_source_meta" ]] || fail 'SSH設定の読取り中に参照先が変わりました'
cat "$ssh_source_ref" > "$stage/sshd_config.before"
{ cat "$stage/sshd_config.before"; printf '\nInclude %s\n' "$stage/ssh.conf"; } > "$stage/sshd_config"
sshd -t -f "$stage/sshd_config" >/dev/null 2>&1 || fail '候補SSH設定が不正です'
verify_effective "$(sshd -T -f "$stage/sshd_config" -C "user=$ssh_user,host=localhost,addr=127.0.0.1")"
bash_prepare_password_variables() {
  # 呼出し環境のexport属性を継承した変数へ秘密を代入しない。
  set +a
  unset -v password confirmation 2>/dev/null || fail 'パスワード変数を安全に初期化できません。readonly等の指定を解除して再実行してください'
  export -n password confirmation 2>/dev/null || fail 'パスワード変数のexport属性を解除できません'
}
bash_prepare_password_variables
[[ -r /dev/tty && -w /dev/tty ]] || fail '対話端末が必要です (パスワードを引数や環境変数へ渡せません)'
printf '32文字以上のランダムなプロキシパスワードを入力してください: ' >/dev/tty
IFS= read -r -s password </dev/tty
printf '\n確認のため再入力: ' >/dev/tty
IFS= read -r -s confirmation </dev/tty
printf '\n' >/dev/tty
[[ ${#password} -ge 32 && ${#password} -le 72 && $password == "$confirmation" && $password =~ ^[[:graph:]]+$ ]] || fail 'パスワードは同一の32～72文字の半角印字可能文字で入力してください'
bash_verify_ssh_replacement
bash_ssh_matches /etc/ssh/sshd_config "$ssh_source_meta" "$(bash_ssh_hash "$stage/sshd_config.before")" || fail 'パスワード入力中にSSH設定が変更されました'
[[ $(systemctl show squid.service -p LoadState --value) == not-found ]] || fail '準備中にSquidが導入されました。既存サービスを変更しません'
for path in /etc/squid /var/log/squid /var/spool/squid "$base" "$home" "$unit" "$ssh_conf"; do [[ ! -e $path && ! -L $path ]] || fail '準備中に既存データが作成されました。上書きしません'; done
! dpkg-query -W -f='${Status}' squid 2>/dev/null | grep -q 'install ok installed' || fail '準備中にSquidパッケージが導入されました'
! getent passwd "$ssh_user" >/dev/null && ! getent group "$ssh_user" >/dev/null || fail '準備中に専用ユーザーまたはグループが作成されました'
[[ $(ufw status | head -n1) == 'Status: active' ]] || fail 'パスワード入力中にUFWの状態が変わりました'
for port in "$public_port" "$remote_port"; do [[ -z $(ss -H -ltn "sport = :$port") ]] || fail 'パスワード入力中にポートが使用されました'; done
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
bash_verify_ssh_replacement
bash_ssh_matches /etc/ssh/sshd_config "$ssh_source_meta" "$(bash_ssh_hash "$stage/sshd_config.before")" || fail '恒久保存の前にSSH設定が変更されました'
for path in "$base" "$home" "$unit" "$ssh_conf"; do [[ ! -e $path && ! -L $path ]] || fail '恒久保存の前に既存データが作成されました。上書きしません'; done
! getent passwd "$ssh_user" >/dev/null && ! getent group "$ssh_user" >/dev/null || fail '恒久保存の前に専用ユーザーまたはグループが作成されました'
# 作成後に一部失敗した場合はmanifestを作らず、再実行では変更を拒否する。
bash_mkdir_new "$base" 755
bash_mkdir_new "$home" 755
bash_mkdir_new "$home/.ssh" 755
useradd --user-group --no-create-home --home-dir "$home" --shell /usr/sbin/nologin --password '*' "$ssh_user"
bash_write_new "$stage/passwd" "$base/passwd" 600
bash_write_new "$stage/squid.conf" "$base/squid.conf" 644
bash_write_new "$stage/options" "$base/options" 600
bash_write_new "$stage/authorized_keys" "$home/.ssh/authorized_keys" 644
bash_write_new "$stage/plumivox-proxy.service" "$unit" 644
bash_write_new "$stage/ssh.conf" "$ssh_conf" 644
bash_write_new "$stage/sshd_config.before" "$base/sshd_config.before" 600
{ cat "$stage/sshd_config.before"; printf '\nInclude %s\n' "$ssh_conf"; } > "$stage/sshd_config.final"
bash_verify_ssh_replacement
bash_atomic_replace_ssh /etc/ssh/sshd_config "$stage/sshd_config.before" "$stage/sshd_config.final" "$ssh_source_meta"
exec {ssh_source_fd}<&-
sshd -t >/dev/null 2>&1 || fail '実際のSSH設定の検証に失敗しました (reloadは実行していません)'
verify_effective "$(sshd -T -C "user=$ssh_user,host=localhost,addr=127.0.0.1")"
bash_revalidate_directory_pins
systemctl reload ssh.service
systemctl daemon-reload
bash_revalidate_directory_pins
systemctl enable --now plumivox-proxy.service >/dev/null
sleep 2
systemctl is-active --quiet plumivox-proxy.service || fail 'プロキシ起動を確認できません。管理者が原因を確認してください'
ufw allow proto tcp from 0.0.0.0/0 to any port "$public_port" comment 'plumivox-proxy' >/dev/null
sha256sum "$base/options" "$base/squid.conf" "$base/passwd" "$unit" "$ssh_conf" "$home/.ssh/authorized_keys" /etc/ssh/sshd_config "$base/sshd_config.before" > "$stage/manifest.sha256"
bash_write_new "$stage/manifest.sha256" "$base/manifest.sha256" 600
installation_completed=1
printf '導入完了: 公開TCP %s / 逆トンネル127.0.0.1:%s / SSHユーザー %s / 認証ユーザー %s\n' "$public_port" "$remote_port" "$ssh_user" "$proxy_user"
printf 'VPS提供元のファイアウォールで公開TCP %sを手動で許可してください。\n' "$public_port"
printf 'SSH設定の旧inodeをroot限定で保持しました: %s\n自動削除しません。同時管理操作と旧ファイルを開いた処理を終了し、新旧内容とSSH管理接続を確認した管理者が手動整理してください。\n' "$ssh_backup_path"
if fingerprints_after=$(collect_fingerprints); then
  printf '%s\n' "$fingerprints_after"
else
  printf '導入済みですが、導入後のSSHホスト鍵の指紋取得に失敗しました。現在の指紋は管理コンソールで確認してください。\n以下は事前確認時の指紋です。\n%s\n' "$fingerprints_before"
fi
