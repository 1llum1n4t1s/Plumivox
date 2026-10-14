#requires -Version 7.0
[CmdletBinding()]
param([switch]$PrepareKey, [switch]$Check, [string]$SettingsPath,
  [string]$PrivateKeyPath, [string]$Account = 'main')

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-PlumivoxSettingsPath {
  param([string]$ExplicitPath)
  if ($env:PLUMIVOX_SECRETS_FILE) { return $env:PLUMIVOX_SECRETS_FILE }
  if ($ExplicitPath) { return $ExplicitPath }
  $legacy = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'dev/Secret/secrets.json'
  if (Test-Path -LiteralPath $legacy) { return $legacy }
  $local = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) 'AppData/Local' }
  return Join-Path $local 'Kagayoi/Plumivox/settings.json'
}

function Assert-PlumivoxPath {
  param([string]$Path)
  if (-not [IO.Path]::IsPathFullyQualified($Path) -or $Path.StartsWith('\\')) { throw 'PATH_INVALID' }
  $full = [IO.Path]::GetFullPath($Path)
  $blocked = @((Split-Path $PSScriptRoot -Parent), (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex'))
  if ($env:CODEX_HOME) { $blocked += [IO.Path]::GetFullPath($env:CODEX_HOME) }
  foreach ($root in $blocked) {
    if ($full.Equals($root, [StringComparison]::OrdinalIgnoreCase) -or $full.StartsWith($root.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'PATH_PROTECTED' }
  }
  $cursor = $full
  while ($cursor) {
    if (Test-Path -LiteralPath $cursor) {
      if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'PATH_LINK' }
    }
    $cursor = Split-Path $cursor -Parent
  }
  return $full
}

function Set-PlumivoxPrivateAcl {
  param([string]$Path)
  $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
  $isDirectory = (Get-Item -LiteralPath $Path -Force).PSIsContainer
  $acl = if ($isDirectory) { [Security.AccessControl.DirectorySecurity]::new() } else { [Security.AccessControl.FileSecurity]::new() }
  $acl.SetOwner($sid)
  $acl.SetAccessRuleProtection($true, $false)
  $inheritance = if ($isDirectory) { [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit' } else { [Security.AccessControl.InheritanceFlags]::None }
  $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', $inheritance, 'None', 'Allow'))
  if ($isDirectory) { [IO.FileSystemAclExtensions]::SetAccessControl([IO.DirectoryInfo]::new($Path), $acl) }
  else { [IO.FileSystemAclExtensions]::SetAccessControl([IO.FileInfo]::new($Path), $acl) }
}

function Assert-PlumivoxPrivateAcl {
  param([string]$Path)
  $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
  $acl = Get-Acl -LiteralPath $Path
  foreach ($rule in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
    if ($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Value -ne $sid) { throw 'ACL_UNSAFE' }
  }
  if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid) { throw 'ACL_OWNER' }
}

function Read-PlumivoxSecret {
  param([string]$Prompt, [switch]$Optional)
  $secure = Read-Host $Prompt -AsSecureString
  $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
  try { $value = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer) }
  finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer); $secure.Dispose() }
  if (-not $Optional -and [string]::IsNullOrWhiteSpace($value)) { throw 'INPUT_REQUIRED' }
  return $value
}

function Read-PlumivoxValue {
  param([string]$Prompt, [string]$Default)
  $value = Read-Host $(if ($Default) { "$Prompt [$Default]" } else { $Prompt })
  if ([string]::IsNullOrWhiteSpace($value)) { return $Default }
  return $value
}

function Read-PlumivoxPort {
  param([string]$Prompt, [string]$Default)
  $value = Read-PlumivoxValue $Prompt $Default
  $number = 0
  if (-not [int]::TryParse($value, [ref]$number)) { throw 'PORT_INVALID' }
  return $number
}

function Assert-PlumivoxRelay {
  param([System.Collections.IDictionary]$Relay)
  if ($Relay.ssh_host -notmatch '^[A-Za-z0-9._:\[\]-]{1,253}$' -or $Relay.ssh_user -notmatch '^[A-Za-z0-9._-]{1,64}$') { throw 'HOST_INVALID' }
  if ($Relay.host_key_sha256 -cnotmatch '^SHA256:[A-Za-z0-9+/]{43}$') { throw 'FINGERPRINT_INVALID' }
  foreach ($name in @('ssh_port','local_port','remote_port')) {
    $value = if ($Relay.Contains($name)) { $Relay[$name] } else { @{ssh_port=22;local_port=43128;remote_port=43129}[$name] }
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt $(if ($name -eq 'ssh_port') { 1 } else { 1024 }) -or $value -gt 65535) { throw 'PORT_INVALID' }
  }
  $key = Assert-PlumivoxPath $Relay.private_key_file
  if (-not (Test-Path -LiteralPath $key -PathType Leaf)) { throw 'KEY_MISSING' }
  Assert-PlumivoxPrivateAcl $key
  if ((Invoke-PlumivoxKeygen @('-y', '-P', '', '-f', $key)) -ne 0) { throw 'KEY_INVALID' }
}

function Invoke-PlumivoxKeygen {
  param([string[]]$Arguments)
  # ArgumentList は PowerShell 7.0 の空文字引数の欠落も回避する。
  $info = [Diagnostics.ProcessStartInfo]::new()
  $info.FileName = Get-PlumivoxKeygen
  $info.UseShellExecute = $false
  $info.CreateNoWindow = $true
  $info.RedirectStandardOutput = $true
  $info.RedirectStandardError = $true
  foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
  $process = [Diagnostics.Process]::new()
  $process.StartInfo = $info
  try {
    [void]$process.Start()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'SSH_KEYGEN_TIMEOUT' }
    [void]$stdout.GetAwaiter().GetResult()
    [void]$stderr.GetAwaiter().GetResult()
    return $process.ExitCode
  } finally { $process.Dispose() }
}

function Enter-PlumivoxSetupLock {
  param([string]$Path)
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
  $hasher = [Security.Cryptography.SHA256]::Create()
  try { $hash = [BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($Path.ToLowerInvariant()))).Replace('-', '') }
  finally { $hasher.Dispose() }
  $mutex = [Threading.Mutex]::new($false, "Local\Plumivox.Setup.$identity.$hash")
  try {
    try { $acquired = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $acquired = $true }
    if (-not $acquired) { throw 'SETUP_BUSY' }
    return $mutex
  } catch { $mutex.Dispose(); throw }
}

function Get-PlumivoxKeygen {
  $keygen = Join-Path $env:WINDIR 'System32/OpenSSH/ssh-keygen.exe'
  if (-not (Test-Path -LiteralPath $keygen)) {
    $command = Get-Command ssh-keygen -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $keygen = if ($command) { $command.Source } else { Join-Path $env:ProgramFiles 'Git/usr/bin/ssh-keygen.exe' }
    if (-not (Test-Path -LiteralPath $keygen)) { throw 'SSH_KEYGEN_REQUIRED' }
  }
  return $keygen
}

function Initialize-PlumivoxKey {
  param([string]$Path)
  $key = Assert-PlumivoxPath $Path
  if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath "$key.pub")) { throw 'KEY_EXISTS' }
  $directory = Split-Path $key -Parent
  if (Test-Path -LiteralPath $directory) { Assert-PlumivoxPrivateAcl $directory }
  else { [void][IO.Directory]::CreateDirectory($directory); Set-PlumivoxPrivateAcl $directory }
  if ((Invoke-PlumivoxKeygen @('-q', '-t', 'ed25519', '-N', '', '-C', 'plumivox', '-f', $key)) -ne 0 -or -not (Test-Path -LiteralPath "$key.pub")) { throw 'KEY_GENERATION_FAILED' }
  Set-PlumivoxPrivateAcl $key
  Set-PlumivoxPrivateAcl "$key.pub"
  Write-Host 'VPS側に登録する公開鍵:'
  Write-Host ([IO.File]::ReadAllText("$key.pub").Trim())
  Write-Host "公開鍵ファイル: $key.pub"
}

function Invoke-PlumivoxSetup {
  param([switch]$PrepareKey, [switch]$Check, [string]$SettingsPath, [string]$PrivateKeyPath, [string]$Account = 'main')
  if (-not $IsWindows) { throw 'WINDOWS_REQUIRED' }
  if (-not $PrivateKeyPath) { $PrivateKeyPath = Join-Path $env:LOCALAPPDATA 'Kagayoi/Plumivox/keys/plumivox_ed25519' }
  if ($PrepareKey -and $Check) { throw 'MODE_INVALID' }
  if ($PrepareKey) {
    $keyLock = Enter-PlumivoxSetupLock (Assert-PlumivoxPath $PrivateKeyPath)
    try { Initialize-PlumivoxKey $PrivateKeyPath } finally { $keyLock.ReleaseMutex(); $keyLock.Dispose() }
    return
  }
  $node = Get-Command node -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $node) { throw 'NODE_REQUIRED' }
  $version = & $node.Source --version 2>$null
  if ($LASTEXITCODE -ne 0 -or $version -notmatch '^v(\d+)\.' -or [int]$Matches[1] -lt 22) { throw 'NODE_REQUIRED' }
  $ssh = Get-Command ssh -CommandType Application -ErrorAction SilentlyContinue
  if (-not $ssh -and -not (Test-Path -LiteralPath (Join-Path $env:WINDIR 'System32/OpenSSH/ssh.exe')) -and -not (Test-Path -LiteralPath (Join-Path $env:ProgramFiles 'Git/usr/bin/ssh.exe'))) { throw 'SSH_REQUIRED' }
  $destination = Assert-PlumivoxPath (Get-PlumivoxSettingsPath $SettingsPath)
  if ($Account -notmatch '^[A-Za-z0-9_-]{1,64}$') { throw 'ACCOUNT_INVALID' }
  $settingsLock = Enter-PlumivoxSetupLock $destination
  try {
  $exists = Test-Path -LiteralPath $destination -PathType Leaf
  $original = if ($exists) { [IO.File]::ReadAllBytes($destination) } else { $null }
  if ($exists -and $original.Length -gt 2097152) { throw 'SETTINGS_INVALID' }
  try { $document = if ($exists) { [Text.Encoding]::UTF8.GetString($original) | ConvertFrom-Json -AsHashtable } else { @{} } }
  catch { throw 'SETTINGS_INVALID' }
  if ($document -isnot [System.Collections.IDictionary]) { throw 'SETTINGS_INVALID' }
  if ($Check) {
    if (-not $document.Contains('twitterapi_io') -or -not $document.twitterapi_io.Contains($Account)) { throw 'ACCOUNT_MISSING' }
    $profile = $document.twitterapi_io[$Account]
    if ($profile.proxy_mode -ne 'managed' -or $profile.user_name -notmatch '^[A-Za-z0-9_]{1,15}$' -or [string]::IsNullOrWhiteSpace($profile.api_key) -or [string]::IsNullOrWhiteSpace($profile.email) -or [string]::IsNullOrWhiteSpace($profile.password)) { throw 'SETTINGS_INVALID' }
    Assert-PlumivoxRelay $document.plumivox.managed_proxy
    $proxy = [Uri]$profile.proxy
    if ($proxy.Scheme -notin @('http','https') -or $proxy.Host -cne $document.plumivox.managed_proxy.ssh_host -or $proxy.Port -lt 1 -or $proxy.IsDefaultPort -or $proxy.AbsolutePath -ne '/' -or $proxy.Query -or $proxy.Fragment -or $proxy.UserInfo -notmatch '^.+:.+$' -or $profile.proxy -match '[\s\x00-\x1f]') { throw 'PROXY_INVALID' }
    if ($profile.Contains('totp_secret') -and $profile.totp_secret -and ($profile.totp_secret -notmatch '^[A-Za-z2-7]+={0,6}$' -or $profile.totp_secret -match '^\d{6}$')) { throw 'TOTP_INVALID' }
    Assert-PlumivoxPrivateAcl $destination
    Write-Host '設定の形式・秘密鍵・ACLを確認しました。ネットワーク通信は実行していません。'
    return
  }
  if ($document.Contains('twitterapi_io') -and $document.twitterapi_io -isnot [System.Collections.IDictionary]) { throw 'SETTINGS_INVALID' }
  if ($document.Contains('plumivox') -and $document.plumivox -isnot [System.Collections.IDictionary]) { throw 'SETTINGS_INVALID' }
  if (($document.Contains('twitterapi_io') -and $document.twitterapi_io.Contains($Account)) -or ($document.Contains('plumivox') -and $document.plumivox.Contains('managed_proxy'))) { throw 'SETTINGS_EXISTS' }
  $user = Read-PlumivoxValue 'Xユーザー名 (@なし)' ''
  if ($user -notmatch '^[A-Za-z0-9_]{1,15}$') { throw 'USER_INVALID' }
  $relay = @{ ssh_host = (Read-PlumivoxValue 'VPSホスト名/IP' ''); ssh_user = (Read-PlumivoxValue 'SSHユーザー' 'plumivox');
    ssh_port = (Read-PlumivoxPort 'SSHポート' '22'); private_key_file = $PrivateKeyPath;
    local_port = (Read-PlumivoxPort 'PC内部ポート' '43128'); remote_port = (Read-PlumivoxPort 'VPS内部ポート' '43129') }
  Write-Host 'VPS提供元のコンソールで ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256 を実行し、表示された指紋を照合してください。'
  $relay.host_key_sha256 = Read-PlumivoxValue 'コンソールで確認済みのSHA256指紋' ''
  $relay.ssh_host = $relay.ssh_host.ToLowerInvariant()
  Assert-PlumivoxRelay $relay
  $publicPort = Read-PlumivoxPort '公開プロキシポート' '3128'
  if ($publicPort -lt 1024 -or $publicPort -gt 65535) { throw 'PORT_INVALID' }
  $proxyUser = Read-PlumivoxValue 'プロキシユーザー' 'plumivox'
  if ($proxyUser -notmatch '^[A-Za-z0-9_-]{1,64}$') { throw 'PROXY_USER_INVALID' }
  $profile = @{ user_name = $user; api_key = (Read-PlumivoxSecret 'twitterapi.io APIキー'); email = (Read-PlumivoxSecret 'Xログインメール'); password = (Read-PlumivoxSecret 'Xパスワード'); proxy_mode = 'managed' }
  $totp = Read-PlumivoxSecret 'Base32の2FA秘密鍵 (未使用なら空欄)' -Optional
  if ($totp -and ($totp -cnotmatch '^[A-Za-z2-7]+={0,6}$' -or $totp -match '^\d{6}$')) { throw 'TOTP_INVALID' }
  if ($totp) { $profile.totp_secret = $totp }
  $proxyPassword = Read-PlumivoxSecret 'プロキシパスワード'
  $profile.proxy = 'http://{0}:{1}@{2}:{3}' -f [Uri]::EscapeDataString($proxyUser), [Uri]::EscapeDataString($proxyPassword), $relay.ssh_host, $publicPort
  if (-not $document.Contains('twitterapi_io')) { $document.twitterapi_io = @{} }
  if (-not $document.Contains('plumivox')) { $document.plumivox = @{} }
  $document.twitterapi_io[$Account] = $profile
  $document.plumivox.managed_proxy = $relay
  $json = $document | ConvertTo-Json -Depth 100
  [void]($json | ConvertFrom-Json -AsHashtable)
  $directory = Split-Path $destination -Parent
  if (-not (Test-Path -LiteralPath $directory)) { [void][IO.Directory]::CreateDirectory($directory); Set-PlumivoxPrivateAcl $directory }
  $staging = Join-Path $directory ('.plumivox-' + [Guid]::NewGuid().ToString('N') + '.tmp')
  [IO.FileStream]::new($staging, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None).Dispose()
  Set-PlumivoxPrivateAcl $staging
  $stream = [IO.FileStream]::new($staging, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::None)
  try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($json)
    $stream.Write($bytes); $stream.Flush($true)
  } finally { $stream.Dispose() }
  # 置換直前に正本を照合し、別の設定変更を上書きしない。
  [void](Assert-PlumivoxPath $destination)
  $nowExists = Test-Path -LiteralPath $destination
  if ($exists -ne $nowExists -or ($exists -and [Convert]::ToBase64String([IO.File]::ReadAllBytes($destination)) -cne [Convert]::ToBase64String($original))) { throw 'SETTINGS_CHANGED' }
  if ($exists) {
    Set-PlumivoxPrivateAcl $destination
    [IO.File]::Replace($staging, $destination, [NullString]::Value)
  } else { [IO.File]::Move($staging, $destination, $false) }
  Assert-PlumivoxPrivateAcl $destination
  Write-Host "設定を保存しました: $destination"
  Write-Host '同じPCユーザーで -Check を実行して確認できます。'
  } finally { $settingsLock.ReleaseMutex(); $settingsLock.Dispose() }
}

if ($MyInvocation.InvocationName -ne '.') {
  try { Invoke-PlumivoxSetup @PSBoundParameters }
  catch {
    # 内部コードに対応する固定文だけを表示し、提供元の生エラーや入力値を出さない。
    $messages = @{
      KEY_EXISTS = '専用SSH鍵は既にあります。既存鍵をそのまま再利用し、-PrepareKeyを付けずに設定を続けてください。鍵は上書きしません。'
      SETTINGS_EXISTS = 'このアカウントまたは共有中継の設定は既にあります。docs/setup.mdの既存設定の説明を確認し、手動で編集してください。既存設定は上書きしません。'
      FINGERPRINT_INVALID = 'VPS提供元のコンソールでホスト鍵指紋を照合し、SHA256:に続く43文字の指紋を入力してください。'
      PORT_INVALID = 'ポートには整数を指定してください。SSHは1〜65535、それ以外は1024〜65535です。'
      SSH_KEYGEN_REQUIRED = 'ssh-keygenが見つかりません。WindowsのOpenSSHクライアントまたはGit for Windowsをインストールしてください。'
      SSH_REQUIRED = 'sshが見つかりません。WindowsのOpenSSHクライアントまたはGit for Windowsをインストールしてください。'
      NODE_REQUIRED = 'Node.js 22以降をインストールし、新しいPowerShellで再実行してください。'
      KEY_MISSING = '専用SSH秘密鍵が見つかりません。最初に-PrepareKeyを実行するか、-PrivateKeyPathで準備済みの鍵を指定してください。'
      KEY_INVALID = 'SSH秘密鍵を読み込めません。専用のパスフレーズなしの鍵を準備し、秘密鍵ファイルを指定してください。'
      KEY_GENERATION_FAILED = '専用SSH鍵を生成できませんでした。保存先の権限とssh-keygenの導入状態を確認してください。既存の鍵は上書きしません。'
      SSH_KEYGEN_TIMEOUT = 'SSH鍵の処理が完了しませんでした。ssh-keygenの導入状態と鍵ファイルを確認してください。'
      ACL_UNSAFE = '秘密ファイルまたは鍵フォルダーを他の利用者が読み取れる状態です。現在のWindows利用者だけがアクセスできる権限にして再実行してください。'
      ACL_OWNER = '秘密ファイルまたは鍵フォルダーの所有者が現在のWindows利用者ではありません。自分が所有する保存先と専用鍵を使用してください。'
      SETTINGS_CHANGED = '入力中に設定ファイルが変更されました。今回の保存は中止しました。設定を確認して再実行してください。'
      SETUP_BUSY = '同じ保存先のセットアップが実行中です。その処理を終了してから再実行してください。'
      PATH_INVALID = '保存先には、このPCの絶対パスを指定してください。ネットワーク共有は使用できません。'
      PATH_PROTECTED = '設定と秘密鍵はプラグイン・リポジトリ・Codex設定フォルダーの外に保存してください。'
      PATH_LINK = '保存先にリンクやジャンクションがあります。リンクを含まない保存先を指定してください。'
      TOTP_INVALID = '2FAには6桁の認証コードではなくBase32の秘密鍵を入力してください。未使用なら空欄にしてください。'
      USER_INVALID = 'Xユーザー名は@を付けず、英数字とアンダースコアの1〜15文字で入力してください。'
      HOST_INVALID = 'VPSホスト名とSSHユーザー名の形式を確認してください。'
      PROXY_USER_INVALID = 'プロキシユーザー名は英数字・アンダースコア・ハイフンの1〜64文字で入力してください。'
      INPUT_REQUIRED = '必須入力が空欄でした。APIキー、Xログイン情報、プロキシパスワードを入力してください。'
      ACCOUNT_INVALID = '-Accountは英数字・アンダースコア・ハイフンの1〜64文字で指定してください。'
      ACCOUNT_MISSING = '指定されたアカウント設定がありません。設定ウィザードを実行するか、-Accountと保存先を確認してください。'
      SETTINGS_INVALID = '設定ファイルのJSON形式とアカウント情報を確認してください。既存設定は上書きしません。'
      PROXY_INVALID = '認証付きプロキシURLとVPSホスト名・ポートの設定を確認してください。'
      MODE_INVALID = '-PrepareKeyと-Checkは同時に指定できません。'
      WINDOWS_REQUIRED = 'このセットアップはWindowsのPowerShell 7以降で実行してください。'
    }
    $code = $_.Exception.Message
    Write-Host $(if ($messages.ContainsKey($code)) { $messages[$code] } else { 'セットアップを完了できませんでした。既存設定・入力形式・SSH鍵・保存先の権限を確認してください。詳細や秘密値は表示しません。' })
    exit 1
  }
}
