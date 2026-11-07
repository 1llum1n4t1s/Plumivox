//! Windows配布ビルドの署名。秘密情報は共通の接続ヘルパーだけが扱う。
use crate::{Result, run};
use std::{env, fs, path::Path, process::Command};

const SUBJECT: &str = "Open Source Developer Yuichiro Shinozaki";
const DEFAULT_CONNECT: &str = r"D:\Dev\skills\vava\scripts\Connect-SimplySign.ps1";

fn commands(executable: &Path) -> Result<Vec<Command>> {
    let connect = env::var_os("PLUMIVOX_SIMPLYSIGN_CONNECT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| DEFAULT_CONNECT.into());
    if !connect.is_absolute() || !connect.is_file() {
        return Err("SimplySign接続ヘルパーが見つかりません。PLUMIVOX_SIMPLYSIGN_CONNECTに共通ヘルパーの絶対パスを指定してください".into());
    }
    let kits = Path::new(
        &env::var_os("ProgramFiles(x86)").unwrap_or_else(|| r"C:\Program Files (x86)".into()),
    )
    .join(r"Windows Kits\10\bin");
    let mut tools = fs::read_dir(kits)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let version = entry
                .file_name()
                .to_str()?
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()?;
            let path = entry.path().join(r"x64\signtool.exe");
            (version.len() == 4 && path.is_file()).then_some((version, path))
        })
        .collect::<Vec<_>>();
    tools.sort_by(|a, b| a.0.cmp(&b.0));
    let tool = tools
        .pop()
        .ok_or("Windows SDKのx64 signtool.exeが必要です")?
        .1;

    let mut authenticate = Command::new("pwsh");
    authenticate
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(connect)
        .args(["-CertificateSubject", SUBJECT, "-TimeoutSeconds", "60"]);
    let mut sign = Command::new(&tool);
    sign.args([
        "sign",
        "/a",
        "/n",
        SUBJECT,
        "/i",
        "Certum",
        "/fd",
        "SHA256",
        "/tr",
        "http://time.certum.pl",
        "/td",
        "SHA256",
        "/d",
        "Plumivox",
    ])
    .arg(executable);
    let mut verify = Command::new(&tool);
    verify.args(["verify", "/pa", "/all"]).arg(executable);
    // 信頼チェーンに加えて本人の証明書とタイムスタンプを確認する。
    // パスをPowerShellコードへ展開せず、この子プロセスだけの環境変数で渡す。
    let mut identity = Command::new("pwsh");
    identity.args(["-NoProfile", "-NonInteractive", "-Command",
        "$ErrorActionPreference = 'Stop'; $s = Get-AuthenticodeSignature -LiteralPath $env:PLUMIVOX_SIGNED_FILE; if ($s.Status -ne 'Valid' -or $s.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -cne 'Open Source Developer Yuichiro Shinozaki' -or $s.SignerCertificate.Issuer -notmatch 'Certum' -or $null -eq $s.TimeStamperCertificate) { throw 'Plumivox署名の発行者・信頼性・タイムスタンプを確認できません' }"])
        .env("PLUMIVOX_SIGNED_FILE", executable);
    Ok(vec![authenticate, sign, verify, identity])
}

pub fn sign(executable: &Path) -> Result {
    for command in &mut commands(executable)? {
        run(command)?;
    }
    println!("Windows Authenticode signature and timestamp verified");
    Ok(())
}
