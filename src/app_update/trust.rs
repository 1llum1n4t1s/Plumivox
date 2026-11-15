//! 取得した更新パッケージの署名者の検証（適用の直前に行う）。
//!
//! R2 からの HTTPS と Velopack のフィードのハッシュは、配信元の改ざんと転送の破損を防ぐが、
//! 「配信元に誰かが別のビルドを置いた」場合は防げない。そこで Windows では、適用前にパッケージ内の
//! 全ての PE（.exe・.dll）を取り出し、Authenticode の署名が有効で、署名者の CN が期待どおり
//! （本人の Certum 証明書）であることを確かめる。満たさなければ適用しない。
//!
//! 主実行ファイル（`lib/app/plumivox.exe`）だけでなく全ての PE を検査するのは、後で実行される Update.exe
//! （パッケージ内では `Squirrel.exe`）を差し替えられると、主実行ファイルの署名だけでは防げないため。
//! 主実行ファイルと Squirrel.exe は、ちょうど1つずつあることも確かめる。
//!
//! 限界（意図して範囲を絞っている）:
//! - 検査から適用までの間に利用者権限で nupkg を差し替えられる競合は防げない（同じ権限なら導入先の
//!   実行ファイルそのものも書き換えられるため、この検査の脅威モデルの外）。
//! - 失効確認（OCSP/CRL）は行わない（ログオン時にネットワークが無くても適用できるようにするため）。
//!
//! macOS では、UpdateMac が導入先へ展開するのと同じ関数（velopack の `extract_lib_contents_to_path`）で
//! nupkg を私的な一時フォルダーへ展開し、その .app を `codesign --verify --deep --strict` と、
//! 「Apple が発行した Developer ID Application 証明書で、チーム ID が本人のもの」という要件で検証し、
//! 公証を `spctl` で確かめる（Remnifold の実機検証済みの手順と同じ。Plumivox の macOS 梱包は未実施）。
type Result<T> = super::Result<T>;
use std::path::Path;

/// 期待する署名者の CN（本人の共通のコード署名証明書。thumbprint は年次更新で変わるため CN で照合する）。
#[cfg_attr(not(windows), allow(dead_code))]
pub const EXPECTED_SIGNER_COMMON_NAME: &str = "Open Source Developer Yuichiro Shinozaki";
/// パッケージ内の主実行ファイルのパス（Velopack の nupkg は `lib/app/` 配下に導入物を持つ）。
#[cfg_attr(not(windows), allow(dead_code))]
const MAIN_EXE_ENTRY: &str = "lib/app/plumivox.exe";
/// Velopack の Update.exe の、パッケージ内での名前。
#[cfg_attr(not(windows), allow(dead_code))]
const UPDATER_ENTRY: &str = "lib/app/Squirrel.exe";
/// 期待する Apple Developer のチーム ID（Developer ID Application 証明書の OU）。公開情報で秘密ではない。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const EXPECTED_TEAM_ID: &str = "SL228Y8UUR";
/// macOS のパッケージ内の主実行ファイル（nupkg の `lib/app/` が .app の中身そのもの）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAC_MAIN_EXE_ENTRY: &str = "lib/app/Contents/MacOS/plumivox";
/// macOS の更新器（UpdateMac）の、パッケージ内での名前。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAC_UPDATER_ENTRY: &str = "lib/app/Contents/MacOS/UpdateMac";

/// 署名の検証を省いてよいか。テスト用の feature を付けたビルドで、環境変数が明示されたときだけ真。
/// 本番ビルドでは常に偽で、この分岐自体がコンパイルされない。
fn unsigned_allowed_for_test() -> bool {
    #[cfg(feature = "update-feed-test")]
    {
        std::env::var_os("PLUMIVOX_UPDATE_ALLOW_UNSIGNED").is_some_and(|value| value == "1")
    }
    #[cfg(not(feature = "update-feed-test"))]
    {
        false
    }
}

/// nupkg の中身を検証する。通れば Ok、満たさなければ理由つきの Err（理由は診断・標準出力へ出してよい短文）。
pub fn verify_package(package: &Path) -> Result<()> {
    if unsigned_allowed_for_test() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        windows::verify_package(package, EXPECTED_SIGNER_COMMON_NAME)
    }
    #[cfg(target_os = "macos")]
    {
        macos::verify_package(package, EXPECTED_TEAM_ID)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = package;
        Err("Updates are not supported on this OS.".into())
    }
}

/// 曖昧な名前（区切りの `\`、ドライブの `:`、`..`、絶対パス）を含むか。正規のパッケージには現れない。
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn ambiguous(name: &str) -> bool {
    name.contains(['\\', ':']) || name.starts_with('/') || name.split('/').any(|part| part == "..")
}

/// macOS の nupkg の名前を確かめる。曖昧な名前はパッケージごと拒否し、主実行ファイルと UpdateMac が
/// 大小を無視してちょうど1つずつあることを要求する。戻り値は展開後の合計サイズ（zip 爆弾の検出に使う）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_package_names(entries: &[(String, u64)]) -> Result<u64> {
    let (mut main, mut updater, mut total) = (0, 0, 0u64);
    for (name, size) in entries {
        if ambiguous(name) {
            return Err("The update package has an entry name that is not allowed.".into());
        }
        main += usize::from(name.eq_ignore_ascii_case(MAC_MAIN_EXE_ENTRY));
        updater += usize::from(name.eq_ignore_ascii_case(MAC_UPDATER_ENTRY));
        total = total.saturating_add(*size);
    }
    if main != 1 || updater != 1 {
        return Err(
            "The update package must contain exactly one main executable and one updater.".into(),
        );
    }
    Ok(total)
}

/// codesign に渡す要件。Apple のルートに連なり（anchor apple generic）、中間が Developer ID の CA
/// （1.2.840.113635.100.6.2.6）、葉が Developer ID Application（1.2.840.113635.100.6.1.13）で、
/// 葉の OU（チーム ID）が期待どおりであること。自己署名の証明書に同じ OU を書いても通らない。
/// codesign には `-R=<要件>` の1引数で渡す（Remnifold の実機で確認済みの形）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn developer_id_requirement(team_id: &str) -> String {
    format!(
        "anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"{team_id}\""
    )
}

/// 一時ファイル・フォルダーの名前に使う乱数の接尾辞。
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn random_suffix() -> String {
    format!("{}-{:016x}", std::process::id(), rand::random::<u64>())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{os::unix::fs::DirBuilderExt, path::PathBuf, process::Command};

    /// 展開後の合計サイズの上限（Plumivox の .app は数十 MB。これを大きく超えるものは異常として拒否する）。
    const MAX_EXTRACTED: u64 = 1024 * 1024 * 1024;

    pub fn verify_package(package: &Path, team_id: &str) -> Result<()> {
        // 名前とサイズを先に確かめる（展開前に、曖昧な名前と巨大な展開を拒否する）。
        let file = std::fs::File::open(package)
            .map_err(|_| "The update package could not be opened.".to_string())?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|_| "The update package is not a readable archive.".to_string())?;
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let entry = archive
                .by_index_raw(index)
                .map_err(|_| "The update package is not a readable archive.".to_string())?;
            let name = entry
                .name()
                .map_err(|_| "The update package has an unreadable entry name.".to_string())?
                .into_owned();
            entries.push((name, entry.size()));
        }
        drop(archive);
        if mac_package_names(&entries)? > MAX_EXTRACTED {
            return Err("The update package is too large.".into());
        }
        // 検査は私的な一時フォルダー（0700）で行い、結果にかかわらず消す。
        let work = std::env::temp_dir().join(format!("plumivox-update-verify-{}", random_suffix()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&work)
            .map_err(|_| "A private verification folder could not be created.".to_string())?;
        let result = verify_in(package, &work, team_id);
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    fn verify_in(package: &Path, work: &Path, team_id: &str) -> Result<()> {
        // .app としての名前で展開する（codesign はバンドルとして扱う）。展開は UpdateMac と同じ関数で行い、
        // 実際に導入先へ入れ替わる中身を検証する。
        let app: PathBuf = work.join("Plumivox.app");
        let bundle = velopack::bundle::load_bundle_from_file(package)
            .map_err(|_| "The update package is not a readable Velopack package.".to_string())?;
        bundle
            .extract_lib_contents_to_path(&app, |_| {})
            .map_err(|_| {
                "The update package could not be extracted for verification.".to_string()
            })?;
        verify_app(&app, team_id)
    }

    /// .app の署名が、バンドル全体で有効かつ期待の Developer ID のものであることを確かめる。
    pub fn verify_app(app: &Path, team_id: &str) -> Result<()> {
        let status = Command::new("/usr/bin/codesign")
            .arg("--verify")
            .arg("--deep")
            .arg("--strict")
            .arg(format!("-R={}", developer_id_requirement(team_id)))
            .arg(app)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|_| "codesign could not be started.".to_string())?;
        if !status.success() {
            return Err("The update package is not signed by the expected Developer ID.".into());
        }
        if !notarized(app) {
            return Err("The update package is not notarized.".into());
        }
        Ok(())
    }

    /// 公証の確認。利用者が Gatekeeper の判定自体を切っている（`spctl --status` が disabled）場合は省く
    /// （利用者が公証の保証を自ら外しているため。Developer ID の署名の確認は常に行う）。
    fn notarized(app: &Path) -> bool {
        let assessed = Command::new("/usr/sbin/spctl")
            .args(["--assess", "--type", "execute", "-vv"])
            .arg(app)
            .stdin(std::process::Stdio::null())
            .output();
        if let Ok(output) = assessed {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if output.status.success() && text.contains("source=Notarized Developer ID") {
                return true;
            }
        }
        Command::new("/usr/sbin/spctl")
            .arg("--status")
            .stdin(std::process::Stdio::null())
            .output()
            .is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout).contains("assessments disabled")
            })
    }
}

/// nupkg（zip）の中の名前が、書き出し先で曖昧にならないかを確かめ、署名を検査すべき PE（.exe・.dll）の
/// 位置を返す。主実行ファイルと Update.exe は、大小を無視してちょうど1つずつあること。
#[cfg_attr(not(windows), allow(dead_code))]
fn package_pe_entries(names: &[String]) -> Result<Vec<usize>> {
    let (mut main, mut updater) = (0, 0);
    let mut entries = Vec::new();
    for (index, name) in names.iter().enumerate() {
        if ambiguous(name) {
            return Err("The update package has an entry name that is not allowed.".into());
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".exe") || lower.ends_with(".dll") {
            entries.push(index);
        }
        main += usize::from(name.eq_ignore_ascii_case(MAIN_EXE_ENTRY));
        updater += usize::from(name.eq_ignore_ascii_case(UPDATER_ENTRY));
    }
    if main != 1 || updater != 1 {
        return Err(
            "The update package must contain exactly one main executable and one updater.".into(),
        );
    }
    Ok(entries)
}

#[cfg(windows)]
pub(crate) mod windows {
    use super::*;
    use std::{
        ffi::c_void,
        io::{Read, Write},
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::{
        Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            Security::{
                Cryptography::{
                    CERT_CONTEXT, CERT_FIND_SUBJECT_CERT, CERT_INFO, CERT_NAME_ATTR_TYPE,
                    CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
                    CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_CERT_INFO_PARAM, CertCloseStore,
                    CertFindCertificateInStore, CertFreeCertificateContext, CertGetNameStringW,
                    CryptMsgClose, CryptMsgGetParam, CryptQueryObject, PKCS_7_ASN_ENCODING,
                    X509_ASN_ENCODING, szOID_COMMON_NAME,
                },
                WinTrust::{
                    WINTRUST_DATA, WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
                    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
                },
            },
        },
        core::GUID,
    };

    /// WINTRUST_ACTION_GENERIC_VERIFY_V2 {00AAC56B-CD44-11d0-8CC2-00C04FC295EE}。
    const GENERIC_VERIFY_V2: GUID = GUID {
        data1: 0x00AA_C56B,
        data2: 0xCD44,
        data3: 0x11D0,
        data4: [0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
    };
    /// 1つの PE の展開サイズの上限（zip 爆弾で一時領域を使い切られないため）。
    const MAX_ENTRY: u64 = 512 * 1024 * 1024;

    pub fn verify_package(package: &Path, expected_common_name: &str) -> Result<()> {
        let unreadable = || "The update package is not a readable archive.".to_string();
        let file = std::fs::File::open(package)
            .map_err(|_| "The update package could not be opened.".to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|_| unreadable())?;
        let mut names = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index).map_err(|_| unreadable())?;
            names.push(entry.name().map_err(|_| unreadable())?.into_owned());
        }
        let entries = package_pe_entries(&names)?;
        // 検査する実体は、検査のために取り出した一時ファイル。検査後に必ず消す。
        for index in entries {
            let temporary = std::env::temp_dir()
                .join(format!("plumivox-update-verify-{}.bin", random_suffix()));
            let result = (|| -> Result<()> {
                let mut entry = archive.by_index(index).map_err(|_| unreadable())?;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(|_| "A verification file could not be created.".to_string())?;
                let copied = std::io::copy(&mut entry.by_ref().take(MAX_ENTRY + 1), &mut file)
                    .map_err(|_| unreadable())?;
                if copied > MAX_ENTRY {
                    return Err("An executable in the update package is too large.".into());
                }
                file.flush().map_err(|_| unreadable())?;
                drop(file);
                verify_file(&temporary, expected_common_name)
            })();
            let _ = std::fs::remove_file(&temporary);
            result?;
        }
        Ok(())
    }

    /// ファイルの Authenticode 署名が有効で、署名者の CN が期待と一致することを確かめる。
    pub fn verify_file(path: &Path, expected_common_name: &str) -> Result<()> {
        if !signature_is_valid(path) {
            return Err(
                "An executable in the update package has no valid Authenticode signature.".into(),
            );
        }
        let actual = signer_common_name(path)
            .ok_or_else(|| "The signer of the update package could not be read.".to_string())?;
        if actual != expected_common_name {
            return Err("The update package is signed by an unexpected publisher.".into());
        }
        Ok(())
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn signature_is_valid(path: &Path) -> bool {
        let path = wide(path);
        let mut file = WINTRUST_FILE_INFO {
            cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: path.as_ptr(),
            hFile: null_mut(),
            pgKnownSubject: null_mut(),
        };
        // SAFETY: ゼロ初期化した構造体に必要な項目を設定してから渡す。state は VERIFY の後に CLOSE で解放する。
        unsafe {
            let mut data: WINTRUST_DATA = std::mem::zeroed();
            data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
            data.dwUIChoice = WTD_UI_NONE;
            data.fdwRevocationChecks = WTD_REVOKE_NONE;
            data.dwUnionChoice = WTD_CHOICE_FILE;
            data.Anonymous.pFile = &mut file;
            data.dwStateAction = WTD_STATEACTION_VERIFY;
            let mut action = GENERIC_VERIFY_V2;
            let status = WinVerifyTrust(
                INVALID_HANDLE_VALUE,
                &mut action,
                &mut data as *mut WINTRUST_DATA as *mut c_void,
            );
            data.dwStateAction = WTD_STATEACTION_CLOSE;
            WinVerifyTrust(
                INVALID_HANDLE_VALUE,
                &mut action,
                &mut data as *mut WINTRUST_DATA as *mut c_void,
            );
            status == 0
        }
    }

    /// 埋め込み署名の最初の署名者の証明書から、CN を読む（CryptQueryObject の定番の手順）。
    fn signer_common_name(path: &Path) -> Option<String> {
        let path = wide(path);
        let mut store = null_mut();
        let mut message: *mut c_void = null_mut();
        // SAFETY: 出力はいずれも自分の変数。得たハンドル・コンテキストは末尾で必ず解放する。
        unsafe {
            if CryptQueryObject(
                CERT_QUERY_OBJECT_FILE,
                path.as_ptr() as *const c_void,
                CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
                CERT_QUERY_FORMAT_FLAG_BINARY,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut store,
                &mut message,
                null_mut(),
            ) == 0
            {
                return None;
            }
            let name = (|| {
                let mut size = 0u32;
                if CryptMsgGetParam(
                    message,
                    CMSG_SIGNER_CERT_INFO_PARAM,
                    0,
                    null_mut(),
                    &mut size,
                ) == 0
                {
                    return None;
                }
                // CERT_INFO を指すポインタを含むため、8 バイト境界の領域に取る。
                let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
                if CryptMsgGetParam(
                    message,
                    CMSG_SIGNER_CERT_INFO_PARAM,
                    0,
                    buffer.as_mut_ptr() as *mut c_void,
                    &mut size,
                ) == 0
                {
                    return None;
                }
                let info = buffer.as_ptr() as *const CERT_INFO;
                let certificate: *mut CERT_CONTEXT = CertFindCertificateInStore(
                    store,
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    0,
                    CERT_FIND_SUBJECT_CERT,
                    info as *const c_void,
                    null(),
                );
                if certificate.is_null() {
                    return None;
                }
                let length = CertGetNameStringW(
                    certificate,
                    CERT_NAME_ATTR_TYPE,
                    0,
                    szOID_COMMON_NAME as *const c_void,
                    null_mut(),
                    0,
                );
                let mut text = vec![0u16; length as usize];
                let written = CertGetNameStringW(
                    certificate,
                    CERT_NAME_ATTR_TYPE,
                    0,
                    szOID_COMMON_NAME as *const c_void,
                    text.as_mut_ptr(),
                    length,
                );
                CertFreeCertificateContext(certificate);
                if written <= 1 {
                    return None;
                }
                Some(String::from_utf16_lossy(&text[..written as usize - 1]))
            })();
            CertCloseStore(store, 0);
            CryptMsgClose(message);
            name
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn unsigned_executable_is_rejected() {
            // このテスト実行ファイル自身は未署名。
            let me = std::env::current_exe().unwrap();
            assert!(verify_file(&me, EXPECTED_SIGNER_COMMON_NAME).is_err());
        }

        /// 未署名の PE を含む nupkg は、形が正しくても拒否される（署名の検査まで進むことの確認）。
        #[test]
        fn package_with_unsigned_executables_is_rejected() {
            use zip::write::SimpleFileOptions;
            let dir = crate::app_update::tests::scratch("trust-nupkg");
            let package = dir.join("TestApp-1.0.0-full.nupkg");
            let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
            let mut writer = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
            for name in ["lib/app/plumivox.exe", "lib/app/Squirrel.exe"] {
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&me).unwrap();
            }
            writer.finish().unwrap();
            let error = verify_package(&package, EXPECTED_SIGNER_COMMON_NAME).unwrap_err();
            assert!(error.contains("Authenticode"), "{error}");
            let _ = std::fs::remove_dir_all(dir);
        }

        /// 署名済みファイルがある環境でだけ、署名者の CN の取得と照合を確かめる。
        /// `PLUMIVOX_TEST_SIGNED_FILE` と `PLUMIVOX_TEST_SIGNED_CN` を両方指定したときだけ走る。
        #[test]
        fn signed_file_probe() {
            let (Some(file), Some(cn)) = (
                std::env::var_os("PLUMIVOX_TEST_SIGNED_FILE"),
                std::env::var("PLUMIVOX_TEST_SIGNED_CN").ok(),
            ) else {
                return;
            };
            let file = std::path::PathBuf::from(file);
            assert_eq!(signer_common_name(&file).as_deref(), Some(cn.as_str()));
            verify_file(&file, &cn).unwrap();
            assert!(verify_file(&file, "Someone Else").is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn main_executable_and_updater_must_appear_exactly_once() {
        let base = [
            "a.nuspec",
            "lib/app/Plumivox.EXE",
            "lib/app/squirrel.exe",
            "lib/app/x.dll",
            "lib/app/README.md",
        ];
        // 検査するのは全ての .exe と .dll（README は対象外）。
        assert_eq!(package_pe_entries(&names(&base)).unwrap(), vec![1, 2, 3]);
        assert!(package_pe_entries(&names(&["lib/app/x.dll", "lib/app/Squirrel.exe"])).is_err());
        assert!(package_pe_entries(&names(&["lib/app/plumivox.exe"])).is_err());
        let mut duplicated = base.to_vec();
        duplicated.push("LIB/APP/plumivox.exe");
        assert!(package_pe_entries(&names(&duplicated)).is_err());
    }

    #[test]
    fn ambiguous_entry_names_reject_the_whole_package() {
        for bad in [
            "lib/app/..\\x.exe",
            "lib/../plumivox.exe",
            "C:/x.exe",
            "/abs.exe",
            "lib\\app\\plumivox.exe",
        ] {
            assert!(
                package_pe_entries(&names(&[
                    "lib/app/plumivox.exe",
                    "lib/app/Squirrel.exe",
                    bad
                ]))
                .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn mac_package_requires_one_main_executable_and_updater() {
        let entries = |items: &[&str]| -> Vec<(String, u64)> {
            items.iter().map(|item| (item.to_string(), 10)).collect()
        };
        let base = [
            "PlumivoxApp.nuspec",
            "lib/app/Contents/Info.plist",
            "lib/app/Contents/MacOS/plumivox",
            "lib/app/Contents/MacOS/UpdateMac",
            "lib/app/Contents/_CodeSignature/CodeResources",
        ];
        assert_eq!(mac_package_names(&entries(&base)).unwrap(), 50);
        assert!(mac_package_names(&entries(&base[..3])).is_err());
        for bad in ["lib/app/../x", "/abs", "lib\\app\\x", "C:/x"] {
            let mut items = base.to_vec();
            items.push(bad);
            assert!(mac_package_names(&entries(&items)).is_err(), "{bad}");
        }
        let requirement = developer_id_requirement("ABCDE12345");
        assert!(requirement.starts_with("anchor apple generic"));
        assert!(requirement.ends_with("certificate leaf[subject.OU] = \"ABCDE12345\""));
    }

    /// 本番ビルド（update-feed-test 無し）では、環境変数があっても未署名の許可が効かない。
    #[cfg(not(feature = "update-feed-test"))]
    #[test]
    fn unsigned_override_is_compiled_out_of_production_builds() {
        // 環境変数を書き換えずに判定だけを見る（並行テストへ影響させない）。
        assert!(!unsigned_allowed_for_test());
    }
}
