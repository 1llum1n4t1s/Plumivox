//! Windows配布ビルドの署名。
//!
//! 署名はSigilune（https://github.com/1llum1n4t1s/Sigilune）で、Certum SimplySignの
//! クラウド鍵を使う。SimplySign Desktopと手入力のワンタイムコードは不要。
//! 資格情報はSigiluneだけが扱う（手元は`sigilune setup`で資格情報マネージャーへ登録、
//! CIはEnvironment `release`のSecretsを署名stepだけへ環境変数で渡す）。
//! このモジュールは資格情報を読まず、引数やログへも出さない。
use crate::{Result, run};
use std::{env, path::Path, path::PathBuf, process::Command};

const SUBJECT: &str = "Open Source Developer Yuichiro Shinozaki";

/// sigilune.exeを 環境変数SIGILUNE_EXE → PATH → %USERPROFILE%\.local\Sigilune\bin の順に探す。
/// 同じフォルダーにsigilune_dlib.dllが必要。見つからなければ未署名へ切り替えずに失敗する。
fn sigilune() -> Result<PathBuf> {
    let candidate = if let Some(explicit) = env::var_os("SIGILUNE_EXE") {
        PathBuf::from(explicit)
    } else if let Some(found) = env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|directory| directory.join("sigilune.exe"))
            .find(|path| path.is_file())
    }) {
        found
    } else {
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(r".local\Sigilune\bin\sigilune.exe")
    };
    if !candidate.is_absolute() || !candidate.is_file() {
        return Err("Sigiluneが見つかりません。https://github.com/1llum1n4t1s/Sigilune を導入して`sigilune setup`で登録するか、SIGILUNE_EXEにsigilune.exeの絶対パスを指定してください".into());
    }
    if !candidate.with_file_name("sigilune_dlib.dll").is_file() {
        return Err("sigilune_dlib.dllがsigilune.exeと同じフォルダーにありません".into());
    }
    Ok(candidate)
}

/// 署名済み実行物の信頼チェーン・本人の証明書・発行者・タイムスタンプを確認する。
/// Sigilune自身も署名後に検証するが、Plumivoxの配布条件としてここでも独立に確認する。
/// CIでは署名step（Secretsあり）と分け、Secretsなしのstepからこの確認だけを呼ぶ。
pub fn verify(executable: &Path) -> Result {
    // パスをPowerShellコードへ展開せず、この子プロセスだけの環境変数で渡す。
    let mut identity = Command::new("pwsh");
    identity.args(["-NoProfile", "-NonInteractive", "-Command",
        "$ErrorActionPreference = 'Stop'; $s = Get-AuthenticodeSignature -LiteralPath $env:PLUMIVOX_SIGNED_FILE; if ($s.Status -ne 'Valid' -or $s.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -cne 'Open Source Developer Yuichiro Shinozaki' -or $s.SignerCertificate.Issuer -notmatch 'Certum' -or $null -eq $s.TimeStamperCertificate) { throw 'Plumivox署名の発行者・信頼性・タイムスタンプを確認できません' }"])
        .env("PLUMIVOX_SIGNED_FILE", executable);
    run(&mut identity)?;
    // Sigiluneの検証（有効性・署名者・タイムスタンプ）も通す。ログインは不要。
    run(Command::new(sigilune()?)
        .args(["verify", "--signer", SUBJECT])
        .arg(executable))?;
    println!("Windows Authenticode signature and timestamp verified");
    Ok(())
}

/// 開発端末の`cargo xtask build`用。Cargoの出力物へ署名してから検証する。
pub fn sign(executable: &Path) -> Result {
    let sigilune = sigilune()?;
    // 先にログインし、認証の失敗をビルド出力の変更前に検出する（有効なセッションは再利用）。
    run(Command::new(&sigilune).arg("login"))?;
    // ソース無変更の再ビルドではCargoが署名済みの同じ出力物を返すため、--replaceで
    // 既存の署名を置き換える（旧signtoolと同じ挙動）。/d相当の説明はUACの表示に使われる。
    run(Command::new(&sigilune)
        .args(["sign", "--replace", "--description", "Plumivox"])
        .arg(executable))?;
    verify(executable)
}
