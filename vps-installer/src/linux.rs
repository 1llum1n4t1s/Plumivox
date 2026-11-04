use super::{Host, Metadata, Output, Result, require};
use std::{
    ffi::{CStr, CString},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use zeroize::Zeroizing;

pub(super) struct LinuxHost {
    candidates: Vec<File>,
    lock: Option<File>,
    stage: Option<(PathBuf, u64, u64)>,
    pinned_dirs: std::cell::RefCell<Vec<(PathBuf, File)>>,
}

fn io_error(_: std::io::Error) -> String {
    // 提供元の生エラーや資格情報を診断へ混ぜない。
    "ファイルまたはシステム操作に失敗しました。管理者が現状を確認してください".into()
}

fn executable(program: &str) -> Option<PathBuf> {
    let paths = if program.starts_with('/') {
        vec![PathBuf::from(program)]
    } else {
        ["/usr/sbin", "/usr/bin", "/sbin", "/bin"]
            .iter()
            .map(|d| Path::new(d).join(program))
            .collect()
    };
    paths
        .into_iter()
        .find(|path| fs::metadata(path).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0))
}

fn open_read(path: &str) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(io_error)?;
    require(
        file.metadata().map_err(io_error)?.is_file(),
        "通常ファイルが必要です",
    )?;
    Ok(file)
}

fn trusted_os_directory(path: &str) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(io_error)?;
    require(
        m.is_dir() && m.uid() == 0 && m.mode() & 0o022 == 0,
        "OS情報のディレクトリの所有者・権限・形式が不正です",
    )
}

fn standard_os_release_target(target: &Path) -> bool {
    matches!(
        target.to_str(),
        Some("/usr/lib/os-release" | "../usr/lib/os-release")
    )
}

fn read_os_release() -> Result<String> {
    trusted_os_directory("/etc")?;
    let path = "/etc/os-release";
    let m = fs::symlink_metadata(path).map_err(io_error)?;
    let source = if m.file_type().is_symlink() {
        require(
            m.uid() == 0 && standard_os_release_target(&fs::read_link(path).map_err(io_error)?),
            "OS情報のリンク先を安全に確認できません",
        )?;
        trusted_os_directory("/usr")?;
        trusted_os_directory("/usr/lib")?;
        "/usr/lib/os-release"
    } else {
        path
    };
    let file = open_read(source)?;
    let source_meta = file.metadata().map_err(io_error)?;
    require(
        source_meta.uid() == 0 && source_meta.mode() & 0o022 == 0 && source_meta.len() <= 65536,
        "OS情報ファイルの所有者・権限・サイズが不正です",
    )?;
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes).map_err(io_error)?;
    require(bytes.len() <= 65536, "OS情報ファイルが大きすぎます")?;
    // symlinkはOS情報専用で許可する。通常の設定・公開鍵のNOFOLLOWは維持する。
    let current = fs::symlink_metadata(path).map_err(io_error)?;
    require(
        current.dev() == m.dev()
            && current.ino() == m.ino()
            && current.uid() == m.uid()
            && current.mode() == m.mode()
            && current.ctime() == m.ctime()
            && current.ctime_nsec() == m.ctime_nsec(),
        "読取り中にOS情報の参照先が変更されました",
    )?;
    String::from_utf8(bytes).map_err(|_| "OS情報の文字コードが不正です".into())
}

fn ssh_metadata_valid(m: &fs::Metadata) -> bool {
    m.is_file() && m.uid() == 0 && m.gid() == 0 && m.nlink() == 1 && m.mode() & 0o7022 == 0
}

fn no_file_xattrs(file: &File) -> Result<()> {
    let size = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    require(
        size >= 0,
        "SSH設定の拡張属性を確認できません。このファイルシステムでは安全な置換を保証できないため導入しません",
    )?;
    require(
        size == 0,
        "SSH設定にACLまたは拡張属性があります。属性を保持した置換を保証できないため導入しません",
    )
}

fn no_default_acl(directory: &File) -> Result<()> {
    let name = c"system.posix_acl_default";
    let size = unsafe {
        libc::fgetxattr(
            directory.as_raw_fd(),
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    if size < 0 {
        return require(
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA),
            "SSH設定ディレクトリのdefault ACLを確認できないため導入しません",
        );
    }
    Err("SSH設定ディレクトリにdefault ACLがあります。別の利用者への権限拡大を避けるため導入しません".into())
}

fn parse_mount_id(info: &str) -> Result<u64> {
    let values: Vec<_> = info
        .lines()
        .filter_map(|line| line.strip_prefix("mnt_id:"))
        .collect();
    require(values.len() == 1, "mount identityを一意に確認できません")?;
    let value = values[0].trim();
    require(
        !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
        "mount identityの形式が不正です",
    )?;
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id != 0)
        .ok_or_else(|| "mount identityの値が不正です".into())
}

fn same_mount_ids(first: &str, second: &str) -> Result<()> {
    require(
        parse_mount_id(first)? == parse_mount_id(second)?,
        "SSH設定と親ディレクトリが別mountです。bind mountを含め安全な交換を保証できないため導入しません",
    )
}

fn same_mount(file: &File, directory: &File) -> Result<()> {
    let info = |file: &File| {
        fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))
            .map_err(|_| "安全に開いた対象のmount identityを確認できません".to_string())
    };
    same_mount_ids(&info(file)?, &info(directory)?)
}

fn validate_initial_user_namespace(fs_type: u64, inode: u64, ns_type: i32) -> Result<()> {
    require(
        fs_type == 0x6e736673 && inode == 0xeffffffd && ns_type == libc::CLONE_NEWUSER,
        "初期user namespaceを確認できません。全拡張属性の参照を保証できないため新規導入しません",
    )
}

fn initial_user_namespace() -> Result<()> {
    let file = File::open("/proc/self/ns/user")
        .map_err(|_| "user namespaceを安全に開けません".to_string())?;
    let mut fs_info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    require(
        unsafe { libc::fstatfs(file.as_raw_fd(), fs_info.as_mut_ptr()) } == 0,
        "user namespaceのnsfs種別を確認できません",
    )?;
    let fs_info = unsafe { fs_info.assume_init() };
    // Linux UAPI NS_GET_NSTYPE = _IO(0xb7, 3)。固定のproc/user FDで種別も検査する。
    let ns_type = unsafe { libc::ioctl(file.as_raw_fd(), 0xb703 as _) };
    validate_initial_user_namespace(
        fs_info.f_type as u64,
        file.metadata().map_err(io_error)?.ino(),
        ns_type,
    )
}

fn validate_inode_flags(flags: libc::c_long) -> Result<()> {
    // FS_EXTENT_FLはデータ配置の実装フラグ。immutable/nodump等の意味を持つ属性は保持不能として拒否。
    require(
        flags >= 0 && flags & !(0x0008_0000 as libc::c_long) == 0,
        "SSH設定にimmutable・nodump等の保持不能なinode flagsがあります。新規導入しません",
    )
}

fn read_inode_flags(file: &File) -> Result<libc::c_long> {
    let mut flags: libc::c_long = 0;
    let request =
        (2u64 << 30) | ((std::mem::size_of::<libc::c_long>() as u64) << 16) | (0x66 << 8) | 1;
    require(
        unsafe { libc::ioctl(file.as_raw_fd(), request as _, &mut flags) } == 0,
        "SSH設定のinode flagsを取得できないため新規導入しません",
    )?;
    Ok(flags)
}

fn safe_inode_flags(file: &File) -> Result<()> {
    validate_inode_flags(read_inode_flags(file)?)
}

fn safe_parent_inode_flags(file: &File) -> Result<()> {
    let flags = read_inode_flags(file)?;
    require(
        flags >= 0 && flags & !(0x0008_1000 as libc::c_long) == 0,
        "SSH設定ディレクトリのinode flagsを安全に継承できないため新規導入しません",
    )
}

fn creation_parent_attributes(directory: &File) -> Result<()> {
    no_default_acl(directory)?;
    safe_parent_inode_flags(directory)
}

fn validate_creation_parent(path: &Path) -> Result<()> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(io_error)?;
    let m = directory.metadata().map_err(io_error)?;
    require(
        m.uid() == 0 && m.gid() == 0 && m.mode() & 0o7022 == 0,
        "新規保存の親ディレクトリの所有者・権限を安全に確認できません",
    )?;
    creation_parent_attributes(&directory)
}

fn directory_permissions(uid: u32, gid: u32, mode: u32, temporary_root: bool) -> Result<()> {
    let writable = mode & 0o022 != 0;
    require(
        uid == 0
            && gid == 0
            && mode & 0o6000 == 0
            && if temporary_root {
                !writable || mode & 0o1000 != 0
            } else {
                mode & 0o1022 == 0
            },
        "保存先の祖先がroot所有ではないか非rootから差替可能です。stickyなしの書込み可能な/tmpを含め新規導入しません",
    )
}

fn no_access_acl(directory: &File) -> Result<()> {
    let result = unsafe {
        libc::fgetxattr(
            directory.as_raw_fd(),
            c"system.posix_acl_access".as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    if result < 0 {
        return require(
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA),
            "祖先ディレクトリのaccess ACLを確認できません",
        );
    }
    Err(
        "祖先ディレクトリにaccess ACLがあります。安全な書込み権限を保証できないため導入しません"
            .into(),
    )
}

fn trusted_directory_chain(path: &Path) -> Result<Vec<(PathBuf, File)>> {
    require(
        path.is_absolute()
            && !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "保存先の祖先パスが不正です",
    )?;
    let mut result = Vec::new();
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(ancestor)
            .map_err(io_error)?;
        let m = file.metadata().map_err(io_error)?;
        directory_permissions(
            m.uid(),
            m.gid(),
            m.mode() & 0o7777,
            ancestor == Path::new("/tmp"),
        )?;
        no_access_acl(&file)?;
        no_default_acl(&file)?;
        result.push((ancestor.to_path_buf(), file));
    }
    Ok(result)
}

fn same_directory_identity(first: &fs::Metadata, second: &fs::Metadata) -> Result<()> {
    require(
        first.is_dir()
            && second.is_dir()
            && first.dev() == second.dev()
            && first.ino() == second.ino(),
        "保持したディレクトリの実体が差し替えられました",
    )
}

fn input_permissions(uid: u32, mode: u32, caller: u32, directory: bool) -> Result<()> {
    require(
        (uid == 0 || uid == caller) && (mode & 0o022 == 0 || (directory && mode & 0o1000 != 0)),
        "公開鍵または親が呼出し本人/root所有ではないか第三者が書込み可能です",
    )
}

fn trusted_input_snapshot(path: &Path, caller: u32) -> Result<Vec<u8>> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(io_error)?.join(path)
    };
    require(
        !absolute
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        "公開鍵の親を安全に検査できないパスです",
    )?;
    let parent = absolute
        .parent()
        .ok_or_else(|| "公開鍵の親を確認できません".to_string())?;
    let mut directories = Vec::new();
    for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(ancestor)
            .map_err(io_error)?;
        let m = file.metadata().map_err(io_error)?;
        input_permissions(m.uid(), m.mode() & 0o7777, caller, true)?;
        directories.push(file);
    }
    let path = absolute
        .to_str()
        .ok_or_else(|| "公開鍵のパスを安全に確認できません".to_string())?;
    let mut file = open_read(path)?;
    let before = file.metadata().map_err(io_error)?;
    input_permissions(before.uid(), before.mode() & 0o7777, caller, false)?;
    let named = fs::symlink_metadata(path).map_err(io_error)?;
    require(
        named.is_file() && named.dev() == before.dev() && named.ino() == before.ino(),
        "公開鍵の参照先が変わりました",
    )?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    require(
        bytes.len() <= 16 * 1024 * 1024,
        "公開鍵ファイルが大きすぎます",
    )?;
    require(
        unchanged_file(&file.metadata().map_err(io_error)?, &before),
        "読取り中に公開鍵が変更されました",
    )?;
    Ok(bytes)
}

fn ssh_security_supported() -> Result<()> {
    initial_user_namespace()?;
    require(
        unsafe { libc::getegid() } == 0,
        "新規ファイルのrootグループ所有を保証できないため導入しません",
    )?;
    // listxattrは権限不足のtrusted.*を一覧から省く。省略された空一覧を安全と扱わない。
    let status = fs::read_to_string("/proc/self/status").map_err(io_error)?;
    let capabilities = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .and_then(|value| u64::from_str_radix(value.trim(), 16).ok());
    require(
        capabilities.is_some_and(|caps| caps & (1 << 21) != 0),
        "SSH設定の全拡張属性を確認するCAP_SYS_ADMINがありません。属性を保持できることを保証できないため導入しません",
    )?;
    // SELinuxは新規inodeへpolicyに従うlabelを付けるため、単純なmodeコピーだけでは保持できない。
    for path in ["/sys/fs/selinux/enforce", "/selinux/enforce"] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err("SELinuxが有効な環境のSSHラベルを保持できないため導入しません。管理者が確認してください".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err("SELinuxの状態を安全に確認できないため導入しません".into()),
        }
    }
    let file = open_read(super::SSH)?;
    safe_inode_flags(&file)?;
    no_file_xattrs(&file)?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/etc/ssh")
        .map_err(io_error)?;
    same_mount(&file, &directory)?;
    safe_parent_inode_flags(&directory)?;
    no_default_acl(&directory)?;
    for path in ["/etc", "/var/lib", "/etc/systemd/system"] {
        validate_creation_parent(Path::new(path))?;
    }
    Ok(())
}

fn unchanged_file(current: &fs::Metadata, original: &fs::Metadata) -> bool {
    current.is_file()
        && current.dev() == original.dev()
        && current.ino() == original.ino()
        && current.uid() == original.uid()
        && current.gid() == original.gid()
        && current.mode() == original.mode()
        && current.nlink() == original.nlink()
        && current.len() == original.len()
        && current.mtime() == original.mtime()
        && current.mtime_nsec() == original.mtime_nsec()
        && current.ctime() == original.ctime()
        && current.ctime_nsec() == original.ctime_nsec()
}

struct PendingSshFile {
    path: PathBuf,
    dev: u64,
    ino: u64,
    directory: PathBuf,
    directory_dev: u64,
    directory_ino: u64,
    cleanup: bool,
}
impl Drop for PendingSshFile {
    fn drop(&mut self) {
        if self.cleanup
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.is_file() && m.dev() == self.dev && m.ino() == self.ino)
        {
            if fs::remove_file(&self.path).is_err() {
                eprintln!(
                    "エラー: SSH候補設定を清掃できません。管理者が{}を確認してください",
                    self.path.display()
                );
            }
        }
        if self.cleanup
            && fs::symlink_metadata(&self.directory).is_ok_and(|m| {
                m.is_dir() && m.dev() == self.directory_dev && m.ino() == self.directory_ino
            })
        {
            if fs::remove_dir(&self.directory).is_err() {
                eprintln!(
                    "エラー: SSH候補設定の一時ディレクトリを清掃できません。管理者が{}を確認してください",
                    self.directory.display()
                );
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    static BEFORE_SSH_EXCHANGE: std::cell::RefCell<Option<Box<dyn FnOnce(&Path)>>> = const {std::cell::RefCell::new(None)};
    static BEFORE_SSH_CANDIDATE_CREATE: std::cell::RefCell<Option<Box<dyn FnOnce(&Path)>>> = const {std::cell::RefCell::new(None)};
    static SSH_EXCHANGE_UNSUPPORTED: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
    static SSH_EXCHANGE_ERROR_AFTER_COMMIT: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
    static SSH_BACKUP_SYNC_FAILURE: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
}

fn exchange_paths(from: &CStr, to: &CStr) -> bool {
    #[cfg(test)]
    if SSH_EXCHANGE_UNSUPPORTED.with(|flag| flag.replace(false)) {
        return false;
    }
    let success = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_EXCHANGE,
        ) == 0
    };
    #[cfg(test)]
    if success && SSH_EXCHANGE_ERROR_AFTER_COMMIT.with(|flag| flag.replace(false)) {
        return false;
    }
    success
}

fn matches_inode_contents(path: &Path, expected: &fs::Metadata, contents: &[u8]) -> bool {
    (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(io_error)?;
        let m = file.metadata().map_err(io_error)?;
        require(
            m.is_file()
                && m.dev() == expected.dev()
                && m.ino() == expected.ino()
                && m.uid() == expected.uid()
                && m.gid() == expected.gid()
                && m.mode() == expected.mode()
                && m.nlink() == expected.nlink()
                && m.len() == contents.len() as u64,
            "交換対象を再照合できません",
        )?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(contents.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        require(bytes == contents, "交換対象の内容を再照合できません")?;
        no_file_xattrs(&file)
    })()
    .is_ok()
}

fn sync_retained_ssh_backup(directory: &File) -> std::io::Result<()> {
    #[cfg(test)]
    if SSH_BACKUP_SYNC_FAILURE.with(|flag| flag.replace(false)) {
        return Err(std::io::Error::from_raw_os_error(libc::EIO));
    }
    directory.sync_all()
}

fn probe_exchange_in_directory(parent: &Path, owner: u32) -> Result<()> {
    let parent_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(parent)
        .map_err(io_error)?;
    let m = parent_file.metadata().map_err(io_error)?;
    require(
        m.uid() == owner && m.mode() & 0o022 == 0,
        "交換機能の検査先を安全に確認できません",
    )?;
    no_default_acl(&parent_file)?;
    let mut pattern = CString::new(
        parent
            .join(".plumivox-exchange-check.XXXXXXXX")
            .as_os_str()
            .as_bytes(),
    )
    .map_err(|_| "交換機能の検査先が不正です".to_string())?
    .into_bytes_with_nul();
    let ptr = unsafe { libc::mkdtemp(pattern.as_mut_ptr().cast()) };
    require(!ptr.is_null(), "交換機能の検査ディレクトリを作成できません")?;
    let directory = PathBuf::from(std::ffi::OsStr::from_bytes(
        unsafe { CStr::from_ptr(ptr) }.to_bytes(),
    ));
    let created_directory = fs::symlink_metadata(&directory).map_err(|_| {
        format!(
            "交換検査ディレクトリを確認できません。管理者が{}を確認してください",
            directory.display()
        )
    })?;
    let mut owned = Vec::<(fs::Metadata, Vec<u8>)>::new();
    let first = directory.join("first");
    let second = directory.join("second");
    let operation = (|| -> Result<()> {
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&directory)
            .map_err(io_error)?;
        let m = dir.metadata().map_err(io_error)?;
        require(
            m.uid() == owner && m.mode() & 0o7777 == 0o700,
            "交換機能の検査ディレクトリの権限が不正です",
        )?;
        no_file_xattrs(&dir)?;
        for (path, contents) in [
            (&first, b"Plumivox exchange probe first".as_slice()),
            (&second, b"Plumivox exchange probe second".as_slice()),
        ] {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
                .map_err(io_error)?;
            file.write_all(contents).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            no_file_xattrs(&file)?;
            owned.push((file.metadata().map_err(io_error)?, contents.to_vec()));
        }
        let from = CString::new(first.as_os_str().as_bytes())
            .map_err(|_| "交換検査のパスが不正です".to_string())?;
        let to = CString::new(second.as_os_str().as_bytes())
            .map_err(|_| "交換検査のパスが不正です".to_string())?;
        require(
            exchange_paths(&from, &to),
            "このファイルシステムでRENAME_EXCHANGEを利用できません。SSH設定・サービス・パッケージは変更していません",
        )?;
        require(
            matches_inode_contents(&first, &owned[1].0, &owned[1].1)
                && matches_inode_contents(&second, &owned[0].0, &owned[0].1),
            "交換機能の検査結果を確認できません。SSH設定・サービス・パッケージは変更していません",
        )?;
        Ok(())
    })();
    let cleanup = (|| -> Result<()> {
        let current_directory = fs::symlink_metadata(&directory).map_err(io_error)?;
        require(
            current_directory.is_dir()
                && current_directory.dev() == created_directory.dev()
                && current_directory.ino() == created_directory.ino()
                && current_directory.uid() == owner
                && current_directory.mode() & 0o7777 == 0o700,
            "交換検査のディレクトリが変わったため清掃を停止しました",
        )?;
        for path in [&first, &second] {
            match fs::symlink_metadata(path) {
                Ok(_) => {
                    require(
                        owned
                            .iter()
                            .any(|(m, bytes)| matches_inode_contents(path, m, bytes)),
                        "交換検査のファイルが変わったため清掃を停止しました",
                    )?;
                    fs::remove_file(path).map_err(io_error)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(io_error(e)),
            }
        }
        fs::remove_dir(&directory).map_err(io_error)
    })();
    if cleanup.is_err() {
        return Err(format!(
            "交換機能の検査用ファイルを清掃できません。保存先: {}。SSH設定・サービス・パッケージは変更していません",
            directory.display()
        ));
    }
    operation
}

#[cfg(test)]
fn atomic_replace_ssh(
    path: &Path,
    before: &[u8],
    contents: &[u8],
    original: &fs::Metadata,
) -> Result<String> {
    let parent = path
        .parent()
        .ok_or_else(|| "SSH設定の保存先が不正です".to_string())?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(parent)
        .map_err(io_error)?;
    atomic_replace_ssh_at(path, before, contents, original, &directory)
}

fn atomic_replace_ssh_at(
    path: &Path,
    before: &[u8],
    contents: &[u8],
    original: &fs::Metadata,
    directory: &File,
) -> Result<String> {
    #[cfg(test)]
    let logical_target = path.to_path_buf();
    let logical_parent = path
        .parent()
        .ok_or_else(|| "SSH設定の親が不正です".to_string())?
        .to_path_buf();
    let parent_meta = directory.metadata().map_err(io_error)?;
    same_directory_identity(
        &parent_meta,
        &fs::symlink_metadata(&logical_parent).map_err(io_error)?,
    )?;
    let anchored_parent = PathBuf::from(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        directory.as_raw_fd()
    ));
    let anchored_target = anchored_parent.join(
        path.file_name()
            .ok_or_else(|| "SSH設定名が不正です".to_string())?,
    );
    let parent = anchored_parent.as_path();
    let path = anchored_target.as_path();
    require(
        parent_meta.uid() == original.uid() && parent_meta.mode() & 0o022 == 0,
        "SSH設定ディレクトリの所有者・権限が不正です",
    )?;
    no_default_acl(&directory)?;
    let source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(io_error)?;
    same_mount(&source, &directory)?;
    safe_parent_inode_flags(&directory)?;
    safe_inode_flags(&source)?;
    no_file_xattrs(&source)?;
    #[cfg(test)]
    BEFORE_SSH_CANDIDATE_CREATE.with(|hook| {
        if let Some(callback) = hook.borrow_mut().take() {
            callback(&logical_parent);
        }
    });
    let template = CString::new(
        parent
            .join(".plumivox-sshd-backup.XXXXXXXX")
            .as_os_str()
            .as_bytes(),
    )
    .map_err(|_| "SSH候補設定の保存先が不正です".to_string())?;
    let mut name = template.into_bytes_with_nul();
    let ptr = unsafe { libc::mkdtemp(name.as_mut_ptr().cast()) };
    require(
        !ptr.is_null(),
        "SSH設定のroot限定退避ディレクトリを作成できません",
    )?;
    let name = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    let backup_directory = PathBuf::from(std::ffi::OsStr::from_bytes(name));
    let backup_meta = fs::symlink_metadata(&backup_directory).map_err(io_error)?;
    let mut pending = PendingSshFile {
        path: backup_directory.join("sshd_config.before"),
        dev: 0,
        ino: 0,
        directory: backup_directory,
        directory_dev: backup_meta.dev(),
        directory_ino: backup_meta.ino(),
        cleanup: true,
    };
    let retained_name = pending
        .directory
        .file_name()
        .ok_or_else(|| "退避名を確認できません".to_string())?
        .to_os_string();
    let retained_display =
        || match fs::read_link(format!("/proc/self/fd/{}", directory.as_raw_fd())) {
            Ok(real_parent) => real_parent
                .join(&retained_name)
                .join("sshd_config.before")
                .to_string_lossy()
                .into_owned(),
            Err(_) => format!(
                "{}/{} (保持FD参照・実名確認不能)",
                anchored_parent.display(),
                retained_name.to_string_lossy()
            ),
        };
    if fs::symlink_metadata(&logical_parent)
        .ok()
        .is_none_or(|m| same_directory_identity(&parent_meta, &m).is_err())
    {
        pending.cleanup = false;
        return Err(format!(
            "SSH保存先の親参照が不明です。候補保持先: {}。後続処理とreloadは実行していません",
            retained_display()
        ));
    }
    require(
        backup_meta.is_dir()
            && backup_meta.uid() == original.uid()
            && backup_meta.mode() & 0o7777 == 0o700,
        "SSH退避ディレクトリの所有者・権限が不正です",
    )?;
    let backup_directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(&pending.directory)
        .map_err(io_error)?;
    no_file_xattrs(&backup_directory)?;
    no_default_acl(&backup_directory)?;
    let mut candidate = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&pending.path)
        .map_err(io_error)?;
    let created = candidate.metadata().map_err(io_error)?;
    pending.dev = created.dev();
    pending.ino = created.ino();
    // default ACLやLSMの予期しない継承があれば、内容書込みやchmodで公開する前に拒否する。
    no_file_xattrs(&candidate)?;
    safe_inode_flags(&candidate)?;
    // 元ファイルには一切書き込まず、完全な候補をsyncしてからrenameする。
    candidate.write_all(contents).map_err(io_error)?;
    let candidate_meta = candidate.metadata().map_err(io_error)?;
    if candidate_meta.uid() != original.uid() || candidate_meta.gid() != original.gid() {
        require(
            unsafe { libc::fchown(candidate.as_raw_fd(), original.uid(), original.gid()) } == 0,
            "SSH候補設定の所有者を保持できません",
        )?;
    }
    candidate
        .set_permissions(fs::Permissions::from_mode(original.mode() & 0o7777))
        .map_err(io_error)?;
    no_file_xattrs(&candidate)?;
    candidate.sync_all().map_err(io_error)?;
    backup_directory.sync_all().map_err(io_error)?;
    let mut current = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(io_error)?;
    same_mount(&current, &directory)?;
    safe_inode_flags(&current)?;
    require(
        unchanged_file(&current.metadata().map_err(io_error)?, original),
        "保存直前にSSH設定の所有者・権限・参照先が変更されました",
    )?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut current)
        .take(before.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    require(bytes == before, "保存直前にSSH設定が変更されました")?;
    no_file_xattrs(&current)?;
    no_default_acl(&directory)?;
    let named = fs::symlink_metadata(path).map_err(io_error)?;
    require(
        unchanged_file(&named, original),
        "保存直前にSSH設定が置換されました",
    )?;
    let current_parent = fs::symlink_metadata(&logical_parent).map_err(io_error)?;
    if !(current_parent.is_dir()
        && current_parent.dev() == parent_meta.dev()
        && current_parent.ino() == parent_meta.ino()
        && current_parent.uid() == parent_meta.uid()
        && current_parent.mode() == parent_meta.mode())
    {
        pending.cleanup = false;
        return Err(format!(
            "保存直前にSSH設定の親参照が変更されました。候補保持先: {}。reloadは実行していません",
            retained_display()
        ));
    }
    let named_candidate = fs::symlink_metadata(&pending.path).map_err(io_error)?;
    let candidate_meta = candidate.metadata().map_err(io_error)?;
    let backup_meta = fs::symlink_metadata(&pending.directory).map_err(io_error)?;
    require(
        backup_meta.is_dir()
            && backup_meta.dev() == pending.directory_dev
            && backup_meta.ino() == pending.directory_ino
            && backup_meta.uid() == original.uid()
            && backup_meta.mode() & 0o7777 == 0o700,
        "保存直前にSSH退避ディレクトリが変更されました",
    )?;
    no_file_xattrs(&backup_directory)?;
    require(
        named_candidate.is_file()
            && named_candidate.dev() == candidate_meta.dev()
            && named_candidate.ino() == candidate_meta.ino()
            && candidate_meta.uid() == original.uid()
            && candidate_meta.gid() == original.gid()
            && candidate_meta.mode() == original.mode()
            && candidate_meta.nlink() == 1,
        "保存直前にSSH候補設定が変更されました",
    )?;
    no_file_xattrs(&candidate)?;
    #[cfg(test)]
    BEFORE_SSH_EXCHANGE.with(|hook| {
        if let Some(callback) = hook.borrow_mut().take() {
            callback(&logical_target);
        }
    });
    let from = CString::new(pending.path.as_os_str().as_bytes())
        .map_err(|_| "SSH候補設定の保存先が不正です".to_string())?;
    let to = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "SSH設定の保存先が不正です".to_string())?;
    // 通常renameと違い、最後の確認直後に別管理者が保存しても追い出したinodeを消さない。
    if !exchange_paths(&from, &to) {
        if fs::symlink_metadata(&logical_parent)
            .is_ok_and(|m| same_directory_identity(&parent_meta, &m).is_ok())
            && matches_inode_contents(path, original, before)
            && matches_inode_contents(&pending.path, &candidate_meta, contents)
        {
            return Err("SSH設定の原子的な交換が失敗しました。両方のパスのinodeと内容を再照合し、元の設定が維持されていることを確認しました。reloadは実行していません".into());
        }
        pending.cleanup = false;
        return Err(format!(
            "SSH設定の置換状態不明。交換APIは失敗を返しましたが両方のパスのinode・内容を確証できません。退避先: {}。reloadは実行していません。両方のパスを自動削除・自動復元しません。現在の保存状態を管理者が確認してください",
            retained_display()
        ));
    }
    // 交換後は退避側の自動削除を止める。状態不明時にrollbackやcleanupでデータを失わせない。
    pending.cleanup = false;
    let uncertain = || {
        format!(
            "SSH設定の置換状態不明。退避先: {}。reloadは実行していません。自動復元は行っていません。同時の管理操作と旧ファイルを開いた処理を終了し、管理者が両方の設定を確認してください",
            retained_display()
        )
    };
    let displaced_ok = (|| -> Result<()> {
        same_directory_identity(
            &parent_meta,
            &fs::symlink_metadata(&logical_parent).map_err(io_error)?,
        )?;
        let backup_meta = fs::symlink_metadata(&pending.directory).map_err(io_error)?;
        require(
            backup_meta.is_dir()
                && backup_meta.dev() == pending.directory_dev
                && backup_meta.ino() == pending.directory_ino
                && backup_meta.uid() == original.uid()
                && backup_meta.mode() & 0o7777 == 0o700,
            "交換後のSSH退避ディレクトリが変更されています",
        )?;
        no_file_xattrs(&backup_directory)?;
        let mut displaced = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&pending.path)
            .map_err(io_error)?;
        safe_inode_flags(&displaced)?;
        let m = displaced.metadata().map_err(io_error)?;
        // rename自体がctimeを変えるFSがあるため、交換後のctimeは比較しない。
        require(
            m.is_file()
                && m.dev() == original.dev()
                && m.ino() == original.ino()
                && m.uid() == original.uid()
                && m.gid() == original.gid()
                && m.mode() == original.mode()
                && m.nlink() == original.nlink()
                && m.len() == original.len()
                && m.mtime() == original.mtime()
                && m.mtime_nsec() == original.mtime_nsec(),
            "交換されたSSH設定が一致しません",
        )?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut displaced)
            .take(before.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        require(bytes == before, "交換されたSSH設定の内容が一致しません")?;
        no_file_xattrs(&displaced)?;
        let mut installed = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(io_error)?;
        safe_inode_flags(&installed)?;
        let m = installed.metadata().map_err(io_error)?;
        require(
            m.is_file()
                && m.dev() == candidate_meta.dev()
                && m.ino() == candidate_meta.ino()
                && m.uid() == original.uid()
                && m.gid() == original.gid()
                && m.mode() == original.mode(),
            "交換後のSSH設定が変更されています",
        )?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut installed)
            .take(contents.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        require(bytes == contents, "交換後のSSH設定内容が変更されています")?;
        no_file_xattrs(&installed)?;
        Ok(())
    })();
    displaced_ok.map_err(|_| uncertain())?;
    // 検査後も旧FDに書かれ得るため、正常時も旧inodeを自動削除しない。
    // 同時管理操作と旧FDの利用が終わり、両内容を確認した管理者だけが手動整理する。
    sync_retained_ssh_backup(&backup_directory).and_then(|_| directory.sync_all())
        .map_err(|_| format!("SSH設定は置換済みですが、退避先の保存確定を確認できません。退避先: {}。自動削除は行っていません。reloadは実行していません", retained_display()))?;
    Ok(retained_display())
}

impl LinuxHost {
    pub(super) fn new() -> Self {
        // 子プロセスが作るパスワードファイルも本人専用にする。
        unsafe {
            libc::umask(0o077);
        }
        Self {
            candidates: Vec::new(),
            lock: None,
            stage: None,
            pinned_dirs: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn pin_directory(&self, path: &Path) -> Result<()> {
        let chain = trusted_directory_chain(path)?;
        let mut pins = self.pinned_dirs.borrow_mut();
        for (path, file) in chain {
            if let Some((_, existing)) = pins.iter().find(|(p, _)| p == &path) {
                let old = existing.metadata().map_err(io_error)?;
                let new = file.metadata().map_err(io_error)?;
                same_directory_identity(&old, &new)?;
            } else {
                pins.push((path, file));
            }
        }
        Ok(())
    }

    fn verify_pinned_directories(&self) -> Result<()> {
        for (path, old) in self.pinned_dirs.borrow().iter() {
            let current = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                .open(path)
                .map_err(io_error)?;
            let old = old.metadata().map_err(io_error)?;
            let new = current.metadata().map_err(io_error)?;
            same_directory_identity(&old, &new)?;
            directory_permissions(
                new.uid(),
                new.gid(),
                new.mode() & 0o7777,
                path == Path::new("/tmp"),
            )?;
            no_access_acl(&current)?;
            no_default_acl(&current)?;
        }
        Ok(())
    }

    fn parent_file(&self, path: &Path) -> Result<File> {
        self.verify_pinned_directories()?;
        let parent = path
            .parent()
            .ok_or_else(|| "保存先の親がありません".to_string())?;
        if let Some((stage, _, _)) = &self.stage {
            let pins = self.pinned_dirs.borrow();
            if let Some((_, file)) = pins.iter().find(|(p, _)| p == stage) {
                if parent
                    == Path::new(&format!(
                        "/proc/{}/fd/{}",
                        std::process::id(),
                        file.as_raw_fd()
                    ))
                {
                    return file.try_clone().map_err(io_error);
                }
            }
        }
        self.pin_directory(parent)?;
        validate_creation_parent(parent)?;
        self.pinned_dirs
            .borrow()
            .iter()
            .find(|(p, _)| p == parent)
            .ok_or_else(|| "保存先の親を保持できません".to_string())?
            .1
            .try_clone()
            .map_err(io_error)
    }
}

impl Host for LinuxHost {
    fn uid(&self) -> u32 {
        unsafe { libc::geteuid() }
    }
    fn os_release(&self) -> Result<String> {
        read_os_release()
    }
    fn public_key_snapshot(&self, path: &str) -> Result<Vec<u8>> {
        let caller = if unsafe { libc::geteuid() } != 0 {
            unsafe { libc::getuid() }
        } else {
            match std::env::var("SUDO_UID") {
                Ok(value) => value
                    .parse::<u32>()
                    .map_err(|_| "sudo呼出し本人のUIDを確認できません".to_string())?,
                Err(std::env::VarError::NotPresent) => 0,
                Err(_) => return Err("sudo呼出し本人のUIDを確認できません".into()),
            }
        };
        trusted_input_snapshot(Path::new(path), caller)
    }
    fn ssh_security(&self) -> Result<()> {
        ssh_security_supported()?;
        for path in [
            "/etc/ssh",
            "/etc/systemd/system",
            "/var/lib",
            "/tmp",
            "/run",
        ] {
            self.pin_directory(Path::new(path))?;
        }
        Ok(())
    }

    fn metadata(&self, path: &str) -> Result<Option<Metadata>> {
        match fs::symlink_metadata(path) {
            Ok(m) => Ok(Some(Metadata {
                directory: m.is_dir(),
                file: m.is_file(),
                uid: m.uid(),
                gid: m.gid(),
                mode: m.mode() & 0o7777,
                links: m.nlink(),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(e)),
        }
    }

    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.verify_pinned_directories()?;
        let mut file = open_read(path)?;
        require(
            file.metadata().map_err(io_error)?.len() <= 16 * 1024 * 1024,
            "ファイルが大きすぎます",
        )?;
        let mut value = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut value)
            .map_err(io_error)?;
        require(value.len() <= 16 * 1024 * 1024, "ファイルが大きすぎます")?;
        Ok(value)
    }

    fn entries(&self, path: &str) -> Result<Vec<String>> {
        let mut entries = fs::read_dir(path)
            .map_err(io_error)?
            .map(|e| {
                e.map_err(io_error)?
                    .path()
                    .into_os_string()
                    .into_string()
                    .map_err(|_| "ファイル名の文字コードが不正です".into())
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort();
        Ok(entries)
    }

    fn available(&self, program: &str) -> bool {
        executable(program).is_some()
    }

    fn command(&mut self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Output> {
        self.verify_pinned_directories()?;
        let path =
            executable(program).ok_or_else(|| "必要な管理コマンドがありません".to_string())?;
        let mut child = Command::new(path)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .env("DEBIAN_FRONTEND", "noninteractive")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "管理コマンドを起動できません".to_string())?;
        if let Some(bytes) = input {
            let write = child
                .stdin
                .take()
                .ok_or_else(|| "管理コマンドの入力に失敗しました".to_string())?
                .write_all(bytes);
            if write.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("管理コマンドの入力に失敗しました".into());
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|_| "管理コマンドの終了を確認できません".to_string())?;
        Ok(Output {
            success: output.status.success(),
            text: String::from_utf8_lossy(&output.stdout).into_owned(),
        })
    }

    fn candidate(&mut self, contents: &[u8]) -> Result<String> {
        let name = CString::new("plumivox-sshd-candidate")
            .map_err(|_| "候補設定を準備できません".to_string())?;
        // sshdのclosefrom(3)後も親プロセスが保持するFDを参照できるようにする。
        // 子へFDを継承せず、ファイルシステムにも一時ファイルを作らない。
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        require(fd >= 0, "メモリー上の候補SSH設定を準備できません")?;
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(contents).map_err(io_error)?;
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        self.candidates.push(file);
        Ok(format!("/proc/{}/fd/{fd}", std::process::id()))
    }

    fn lock(&mut self) -> Result<()> {
        let parent = self.parent_file(Path::new("/run/plumivox-proxy-install.lock"))?;
        let path = format!(
            "/proc/self/fd/{}/plumivox-proxy-install.lock",
            parent.as_raw_fd()
        );
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|_| "導入ロックが不正です".to_string())?;
        let m = file.metadata().map_err(io_error)?;
        require(
            m.is_file()
                && m.uid() == 0
                && m.gid() == 0
                && m.mode() & 0o7777 == 0o600
                && m.nlink() == 1
                && m.len() == 0,
            "導入ロックの所有者・権限・形式が不正です",
        )?;
        let named = fs::symlink_metadata(&path).map_err(io_error)?;
        require(
            named.is_file() && named.dev() == m.dev() && named.ino() == m.ino(),
            "導入ロックの参照先が差し替えられました",
        )?;
        use std::os::fd::AsRawFd;
        require(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "別の導入が実行中です",
        )?;
        self.lock = Some(file);
        Ok(())
    }

    fn probe_exchange(&mut self) -> Result<()> {
        probe_exchange_in_directory(Path::new("/etc/ssh"), 0)
    }

    fn stage(&mut self) -> Result<String> {
        self.pin_directory(Path::new("/tmp"))?;
        self.verify_pinned_directories()?;
        require(self.stage.is_none(), "導入用一時ディレクトリは作成済みです")?;
        let mut pattern = CString::new("/tmp/plumivox-proxy.XXXXXXXX")
            .map_err(|_| "一時ディレクトリを準備できません".to_string())?
            .into_bytes_with_nul();
        let ptr = unsafe { libc::mkdtemp(pattern.as_mut_ptr().cast()) };
        require(!ptr.is_null(), "導入用一時ディレクトリを作成できません")?;
        let path = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .map_err(|_| "一時ディレクトリの名前が不正です".to_string())?
            .to_owned();
        let m = fs::symlink_metadata(&path).map_err(io_error)?;
        self.stage = Some((PathBuf::from(&path), m.dev(), m.ino()));
        require(
            m.is_dir() && m.uid() == 0 && m.mode() & 0o7777 == 0o700,
            "一時ディレクトリの権限が不正です",
        )?;
        self.pin_directory(Path::new(&path))?;
        let pins = self.pinned_dirs.borrow();
        let file = &pins
            .iter()
            .find(|(p, _)| p == Path::new(&path))
            .ok_or_else(|| "一時ディレクトリを保持できません".to_string())?
            .1;
        same_directory_identity(&m, &file.metadata().map_err(io_error)?)?;
        Ok(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            file.as_raw_fd()
        ))
    }

    fn write_new(&mut self, path: &str, contents: &[u8], mode: u32) -> Result<()> {
        let parent = self.parent_file(Path::new(path))?;
        let name = Path::new(path)
            .file_name()
            .ok_or_else(|| "保存名が不正です".to_string())?;
        let anchored = PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&anchored)
            .map_err(|_| "保存先が既存または書込み不可です。上書きしません".to_string())?;
        file.write_all(contents).map_err(io_error)?;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        let m = file.metadata().map_err(io_error)?;
        require(
            m.uid() == 0 && m.gid() == 0 && m.mode() & 0o7777 == mode && m.is_file(),
            "保存したファイルの所有者・権限が不正です",
        )
    }

    fn mkdir_new(&mut self, path: &str, mode: u32) -> Result<()> {
        let parent = self.parent_file(Path::new(path))?;
        let name = Path::new(path)
            .file_name()
            .ok_or_else(|| "保存名が不正です".to_string())?;
        let anchored = PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(name);
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(mode)
            .create(&anchored)
            .map_err(|_| "保存先が既存または作成不可です。上書きしません".to_string())?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&anchored)
            .map_err(io_error)?;
        directory
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(io_error)?;
        let m = directory.metadata().map_err(io_error)?;
        require(
            m.is_dir() && m.uid() == 0 && m.gid() == 0 && m.mode() & 0o7777 == mode,
            "作成したディレクトリの所有者・権限が不正です",
        )?;
        self.pin_directory(Path::new(path))?;
        Ok(())
    }

    fn replace_ssh(&mut self, before: &[u8], contents: &[u8]) -> Result<String> {
        self.verify_pinned_directories()?;
        ssh_security_supported()?;
        let parent = self.parent_file(Path::new(super::SSH))?;
        let file = open_read(&format!("/proc/self/fd/{}/sshd_config", parent.as_raw_fd()))?;
        let m = file.metadata().map_err(io_error)?;
        require(
            ssh_metadata_valid(&m),
            "SSH設定の所有者・権限・形式が不正です",
        )?;
        atomic_replace_ssh_at(Path::new(super::SSH), before, contents, &m, &parent)
    }

    fn password(&mut self, prompt: &str) -> Result<Zeroizing<String>> {
        // rpasswordはUnixでは/dev/ttyを開き、エコーを停止・復元する。stdinは受け付けない。
        rpassword::prompt_password(prompt)
            .map(Zeroizing::new)
            .map_err(|_| "対話端末が必要です (パスワードを引数や環境変数へ渡せません)".into())
    }

    fn wait_service(&mut self) {
        thread::sleep(Duration::from_secs(2));
    }

    fn cleanup(&mut self) -> Result<()> {
        self.candidates.clear();
        if let Some((path, dev, ino)) = self.stage.as_ref() {
            let m = fs::symlink_metadata(path).map_err(io_error)?;
            require(
                path.parent() == Some(Path::new("/tmp"))
                    && m.is_dir()
                    && m.dev() == *dev
                    && m.ino() == *ino
                    && m.uid() == 0
                    && m.mode() & 0o7777 == 0o700,
                "一時ディレクトリの状態が変わったため清掃を停止しました",
            )?;
            fs::remove_dir_all(path)
                .map_err(|_| "導入用一時ディレクトリの清掃に失敗しました".to_string())?;
            self.pinned_dirs
                .borrow_mut()
                .retain(|(p, _)| !p.starts_with(path));
            self.stage = None;
        }
        Ok(())
    }
}

impl Drop for LinuxHost {
    fn drop(&mut self) {
        if self.stage.is_some() && self.cleanup().is_err() {
            eprintln!(
                "エラー: 導入用一時ディレクトリを清掃できません。管理者が/tmp/plumivox-proxy.*の今回分を確認してください"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installer_function(name: &str) -> String {
        let source = include_str!("../../setup/vps/install.sh").replace("\r\n", "\n");
        let marker = format!("{name}() {{\n");
        let start = source
            .find(&marker)
            .expect("実際のinstall.shに検証対象helperがありません");
        let rest = &source[start..];
        let end = rest.find("\n}").expect("実際のhelperの終端がありません") + 2;
        rest[..end].to_string()
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let base = std::env::temp_dir();
            assert!(base.is_absolute());
            assert!(!base.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()));
            assert!(fs::symlink_metadata(&base).unwrap().is_dir());
            let mut pattern = CString::new(
                base.join("plumivox-vps-unit.XXXXXXXX")
                    .as_os_str()
                    .as_bytes(),
            )
            .unwrap()
            .into_bytes_with_nul();
            let ptr = unsafe { libc::mkdtemp(pattern.as_mut_ptr().cast()) };
            assert!(!ptr.is_null());
            Self(PathBuf::from(unsafe {
                std::ffi::OsStr::from_bytes(CStr::from_ptr(ptr).to_bytes())
            }))
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn os_release_only_accepts_standard_system_link_targets() {
        for target in ["/usr/lib/os-release", "../usr/lib/os-release"] {
            assert!(standard_os_release_target(Path::new(target)));
        }
        for target in [
            "/tmp/os-release",
            "../../tmp/os-release",
            "os-release",
            "/usr/lib/../local/os-release",
            "../usr/lib/os-release/extra",
        ] {
            assert!(!standard_os_release_target(Path::new(target)));
        }
    }

    #[test]
    fn reads_native_os_release_including_standard_symlink() {
        let contents = read_os_release().unwrap();
        assert!(contents.lines().any(|line| line.starts_with("ID=")));
    }

    #[test]
    fn parent_memfd_is_readable_without_inherited_descriptor() {
        let mut host = LinuxHost::new();
        let path = host.candidate(b"Port 22\n").unwrap();
        assert!(path.starts_with(&format!("/proc/{}/fd/", std::process::id())));
        let file = host.candidates.last().unwrap();
        assert_ne!(
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        // /proc/<親PID>/fdは独立したopenで読み、子のFD継承には依存しない。
        assert_eq!(fs::read(&path).unwrap(), b"Port 22\n");
    }

    #[test]
    #[ignore = "requires OpenSSH sshd/ssh-keygen; run explicitly on Linux without starting a daemon"]
    fn parent_memfd_passes_real_sshd_syntax_and_effective_config_checks() {
        let sshd = executable("sshd").expect(
            "このLinux検証にはopenssh-serverのsshdが必要です。未実施を成功扱いにはしません",
        );
        let keygen = executable("ssh-keygen")
            .expect("このLinux検証にはopenssh-clientのssh-keygenが必要です");
        let dir = TestDirectory::new();
        let key = dir.0.join("synthetic_host_key");
        let generated = Command::new(keygen)
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(generated.success(), "合成HostKeyの生成に失敗しました");
        let contents = format!(
            "Port 22222\nHostKey {}\nPidFile {}\nUsePAM no\nPasswordAuthentication yes\nAllowTcpForwarding yes\n",
            key.display(),
            dir.0.join("synthetic_sshd.pid").display()
        );
        let mut host = LinuxHost::new();
        let baseline = host.candidate(contents.as_bytes()).unwrap();
        let options = crate::Options::parse([
            "--public-key-file".to_string(),
            key.with_extension("pub").to_string_lossy().into_owned(),
        ])
        .unwrap()
        .unwrap();
        let candidate = host
            .candidate(format!("{contents}\n{}", options.ssh()).as_bytes())
            .unwrap();
        for config in [&baseline, &candidate] {
            let result = Command::new(&sshd)
                .args(["-t", "-f", config])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "実sshd -tの候補設定検査が失敗しました: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let effective = |config: &str, context: &str| {
            let result = Command::new(&sshd)
                .args(["-T", "-f", config, "-C", context])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "実sshd -T -Cの候補設定検査が失敗しました: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            String::from_utf8(result.stdout).unwrap()
        };
        let dedicated = effective(&candidate, &options.context());
        crate::verify_effective(&dedicated, &options).unwrap();
        for (name, value) in [
            ("passwordauthentication", "no"),
            ("kbdinteractiveauthentication", "no"),
            ("allowtcpforwarding", "remote"),
            ("allowstreamlocalforwarding", "no"),
            ("permitlisten", "127.0.0.1:43129"),
            ("permitopen", "none"),
            ("permittty", "no"),
            ("forcecommand", "/usr/sbin/nologin"),
            ("clientaliveinterval", "15"),
            ("clientalivecountmax", "3"),
        ] {
            assert_eq!(
                crate::setting(&dedicated, name),
                Some(value),
                "実sshd専用ユーザー制限: {name}"
            );
        }
        let admin_context = "user=synthetic-administrator,host=localhost,addr=127.0.0.1";
        assert_eq!(
            effective(&baseline, admin_context),
            effective(&candidate, admin_context),
            "専用Match追加前後で管理ユーザーの有効設定が変わりました"
        );

        // install.shから抽出した実helperを試す。candidate_sshdだけを専用configへ接続する。
        let bash = executable("bash").expect("Bash互換経路の検証にはBashが必要です");
        let baseline_file = dir.0.join("synthetic_baseline.conf");
        fs::write(&baseline_file, &contents).unwrap();
        let script = format!(
            "set -euo pipefail\nexport LC_ALL=C\nbaseline=$1\nsshd_path=$2\nssh_user=plumivox\nremote_port=43129\nsshd() {{ \"$sshd_path\" \"$@\"; }}\n{}\ncandidate_sshd() {{ cat \"$baseline\"; printf '\\n'; render_ssh; }}\n{}\ncandidate_sshd_command -t\nvalue=$(candidate_sshd_command -T -C \"user=$ssh_user,host=localhost,addr=127.0.0.1\")\nadmin_before=$(\"$sshd_path\" -T -f \"$baseline\" -C 'user=synthetic-administrator,host=localhost,addr=127.0.0.1')\nadmin_after=$(candidate_sshd_command -T -C 'user=synthetic-administrator,host=localhost,addr=127.0.0.1')\n[[ $admin_before == \"$admin_after\" ]]\nprintf '%s\\n' \"$value\"\n",
            installer_function("render_ssh"),
            installer_function("candidate_sshd_command")
        );
        let result = Command::new(&bash)
            .args([
                "--noprofile",
                "--norc",
                "-c",
                &script,
                "plumivox-bash-regression",
            ])
            .arg(&baseline_file)
            .arg(&sshd)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "実install.sh helperでsshd検査が失敗しました: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        crate::verify_effective(&String::from_utf8(result.stdout).unwrap(), &options).unwrap();

        // 指紋helperも実コードを使用し、カスタムHostKeyと.pub競合を実ssh-keygenで確認する。
        let public_path = key.with_extension("pub");
        let original_public = fs::read(&public_path).unwrap();
        let keygen = executable("ssh-keygen").unwrap();
        let mut expected = Command::new(&keygen)
            .args(["-l", "-E", "sha256", "-f", "/dev/stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        expected
            .stdin
            .take()
            .unwrap()
            .write_all(&original_public)
            .unwrap();
        let expected = expected.wait_with_output().unwrap();
        assert!(expected.status.success());
        let expected = String::from_utf8(expected.stdout).unwrap();
        let expected_fingerprint = expected.split_whitespace().nth(1).unwrap();
        let replacement = dir.0.join("synthetic_replacement_host_key");
        assert!(
            Command::new(&keygen)
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(&replacement)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        let fingerprint_script = format!(
            "set -euo pipefail\nexport LC_ALL=C\nbaseline=$1\nsshd_path=$2\nkeygen_path=$3\npublic_path=$4\nreplacement_public=$5\nmutate=$6\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\nsshd() {{ \"$sshd_path\" -f \"$baseline\" \"$@\"; }}\nssh-keygen() {{ if [[ ${{1:-}} == -l && $mutate == 1 ]]; then cp -- \"$replacement_public\" \"$public_path\"; fi; \"$keygen_path\" \"$@\"; }}\n{}\ncollect_fingerprints\n",
            installer_function("collect_fingerprints")
        );
        let fingerprint_result = |mutate: &str| {
            Command::new(&bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &fingerprint_script,
                    "plumivox-bash-fingerprint",
                ])
                .arg(&baseline_file)
                .arg(&sshd)
                .arg(&keygen)
                .arg(&public_path)
                .arg(replacement.with_extension("pub"))
                .arg(mutate)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };
        let result = fingerprint_result("1");
        assert!(
            result.status.success(),
            "実install.shの指紋helper検査が失敗しました: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains(expected_fingerprint));
        assert_ne!(
            fs::read(&public_path).unwrap(),
            original_public,
            "照合後の.pub競合を注入できていません"
        );
        let mismatch = fingerprint_result("0");
        assert!(
            !mismatch.status.success(),
            ".pub不一致を安全に拒否しませんでした"
        );
        assert!(String::from_utf8_lossy(&mismatch.stderr).contains("一致しません"));
        assert!(!String::from_utf8_lossy(&mismatch.stdout).contains("SHA256:"));
        fs::write(&public_path, &original_public).unwrap();
        let input_functions = [
            "bash_input_permissions",
            "bash_input_snapshot_file",
            "bash_public_key_from_snapshot",
        ]
        .map(installer_function)
        .join("\n");
        let script = format!(
            "set -euo pipefail\ninput_caller_uid=$1\npublic_path=$2\nreplacement_public=$3\nkeygen_path=$4\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\nssh-keygen() {{ cp -- \"$replacement_public\" \"$public_path\"; \"$keygen_path\" \"$@\"; }}\n{input_functions}\nbash_input_snapshot_file \"$public_path\"\nbash_public_key_from_snapshot \"$input_snapshot\"\nprintf '%s\\n' \"$key\"\n"
        );
        let result = Command::new(&bash)
            .args([
                "--noprofile",
                "--norc",
                "-c",
                &script,
                "public-input-fixture",
            ])
            .arg(unsafe { libc::getuid() }.to_string())
            .arg(&public_path)
            .arg(replacement.with_extension("pub"))
            .arg(&keygen)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let words: Vec<_> = String::from_utf8(original_public.clone())
            .unwrap()
            .split_whitespace()
            .take(2)
            .map(str::to_string)
            .collect();
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!("{} {}\n", words[0], words[1])
        );
        assert_ne!(fs::read(&public_path).unwrap(), original_public);
        assert!(
            !dir.0.join("synthetic_sshd.pid").exists(),
            "検査でdaemonを起動してはいけません"
        );
    }

    fn set_xattr(file: &File, name: &CStr, value: &[u8]) {
        let result = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        assert_eq!(
            result,
            0,
            "この実FS検証にはxattr/ACL対応の一時ファイルシステムが必要です: {}",
            std::io::Error::last_os_error()
        );
    }

    fn default_acl() -> Vec<u8> {
        // Linux UAPI: posix_acl_xattr_header(version=2) + tag/perm/idのlittle endian entries。
        let uid = unsafe { libc::geteuid() };
        let named_uid = if uid == 65534 { 65533u32 } else { 65534u32 };
        let mut value = 2u32.to_le_bytes().to_vec();
        for (tag, permission, id) in [
            (1u16, 7u16, u32::MAX),
            (2, 7, named_uid),
            (4, 0, u32::MAX),
            (16, 7, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            value.extend_from_slice(&tag.to_le_bytes());
            value.extend_from_slice(&permission.to_le_bytes());
            value.extend_from_slice(&id.to_le_bytes());
        }
        value
    }

    #[test]
    fn ssh_xattrs_and_default_acls_refuse_without_replacing_original() {
        for kind in ["xattr", "default-acl", "inherited-acl-race"] {
            let dir = TestDirectory::new();
            let path = dir.0.join("sshd_config");
            fs::write(&path, b"private original\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let original = fs::metadata(&path).unwrap();
            match kind {
                "xattr" => set_xattr(
                    &File::open(&path).unwrap(),
                    c"user.plumivox-test",
                    b"original attribute",
                ),
                "default-acl" => set_xattr(
                    &File::open(&dir.0).unwrap(),
                    c"system.posix_acl_default",
                    &default_acl(),
                ),
                "inherited-acl-race" => BEFORE_SSH_CANDIDATE_CREATE.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(|parent| {
                        set_xattr(
                            &File::open(parent).unwrap(),
                            c"system.posix_acl_default",
                            &default_acl(),
                        );
                    }));
                }),
                _ => unreachable!(),
            }
            assert!(
                atomic_replace_ssh(
                    &path,
                    b"private original\n",
                    b"candidate must not be published\n",
                    &original
                )
                .is_err(),
                "{kind}"
            );
            assert_eq!(fs::read(&path).unwrap(), b"private original\n", "{kind}");
            assert_eq!(fs::metadata(&path).unwrap().ino(), original.ino(), "{kind}");
            assert_eq!(
                fs::metadata(&path).unwrap().mode(),
                original.mode(),
                "{kind}"
            );
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1, "{kind}");
        }
    }

    #[test]
    fn exchange_retains_noncooperative_update_instead_of_losing_or_rolling_back_it() {
        for change in ["in-place", "new-inode"] {
            let dir = TestDirectory::new();
            let path = dir.0.join("sshd_config");
            fs::write(&path, b"original\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let original = fs::metadata(&path).unwrap();
            BEFORE_SSH_EXCHANGE.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |path| {
                    if change == "in-place" {
                        fs::write(path, b"concurrent administrator data\n").unwrap();
                    } else {
                        let alternate = path.with_extension("alternate");
                        fs::write(&alternate, b"concurrent administrator data\n").unwrap();
                        fs::rename(alternate, path).unwrap();
                    }
                }));
            });
            let error =
                atomic_replace_ssh(&path, b"original\n", b"complete candidate\n", &original)
                    .unwrap_err();
            assert!(
                error.contains("置換状態不明")
                    && error.contains("退避先:")
                    && error.contains("reloadは実行していません")
            );
            assert_eq!(fs::read(&path).unwrap(), b"complete candidate\n");
            let displaced: Vec<_> = fs::read_dir(&dir.0)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p != &path)
                .collect();
            assert_eq!(displaced.len(), 1);
            assert_eq!(
                fs::read(displaced[0].join("sshd_config.before")).unwrap(),
                b"concurrent administrator data\n"
            );
        }
    }

    #[test]
    fn unsupported_exchange_preserves_original_and_backup_sync_failure_reports_installed_state() {
        for failure in ["exchange", "sync"] {
            let dir = TestDirectory::new();
            let path = dir.0.join("sshd_config");
            fs::write(&path, b"original\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let original = fs::metadata(&path).unwrap();
            if failure == "exchange" {
                SSH_EXCHANGE_UNSUPPORTED.with(|flag| flag.set(true));
            } else {
                SSH_BACKUP_SYNC_FAILURE.with(|flag| flag.set(true));
            }
            let error =
                atomic_replace_ssh(&path, b"original\n", b"complete candidate\n", &original)
                    .unwrap_err();
            assert!(error.contains("reloadは実行していません"));
            if failure == "exchange" {
                assert_eq!(fs::read(&path).unwrap(), b"original\n");
                assert_eq!(fs::metadata(&path).unwrap().ino(), original.ino());
                assert!(error.contains("元の設定が維持されていることを確認"));
                assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
            } else {
                assert_eq!(fs::read(&path).unwrap(), b"complete candidate\n");
                assert!(error.contains("置換済み") && error.contains("退避先:"));
                assert!(!error.contains("置換状態不明"));
                let backup = fs::read_dir(&dir.0)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|p| p != &path)
                    .unwrap();
                assert_eq!(
                    fs::read(backup.join("sshd_config.before")).unwrap(),
                    b"original\n"
                );
            }
        }
    }

    #[test]
    fn atomic_ssh_replace_preserves_mode_owner_and_complete_old_inode() {
        for mode in [0o600, 0o644] {
            let dir = TestDirectory::new();
            let path = dir.0.join("sshd_config");
            fs::write(&path, b"Port 22\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let original = fs::metadata(&path).unwrap();
            let mut old = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            let backup = atomic_replace_ssh(
                &path,
                b"Port 22\n",
                b"Port 22\n# complete candidate\n",
                &original,
            )
            .unwrap();
            let replaced = fs::metadata(&path).unwrap();
            assert_eq!(replaced.mode(), original.mode());
            assert_eq!(
                (replaced.uid(), replaced.gid()),
                (original.uid(), original.gid())
            );
            assert_ne!(replaced.ino(), original.ino());
            assert_eq!(fs::read(&path).unwrap(), b"Port 22\n# complete candidate\n");
            let mut old_bytes = Vec::new();
            old.read_to_end(&mut old_bytes).unwrap();
            assert_eq!(old_bytes, b"Port 22\n");
            assert_eq!(fs::read(&backup).unwrap(), b"Port 22\n");
            assert_eq!(fs::metadata(&backup).unwrap().ino(), original.ino());
            // 交換後・照合後に、別管理者が交換前から保持していたFDへ書いても失わせない。
            old.write_all(b"# late administrator write\n").unwrap();
            old.sync_all().unwrap();
            assert_eq!(
                fs::read(&backup).unwrap(),
                b"Port 22\n# late administrator write\n"
            );
            assert_eq!(fs::read(&path).unwrap(), b"Port 22\n# complete candidate\n");
            let backup_directory = Path::new(&backup).parent().unwrap();
            assert_eq!(
                fs::metadata(backup_directory).unwrap().mode() & 0o7777,
                0o700
            );
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
        }
    }

    #[test]
    fn exchange_api_error_after_commit_retains_both_inodes_and_reports_uncertainty() {
        let dir = TestDirectory::new();
        let path = dir.0.join("sshd_config");
        fs::write(&path, b"original\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let original = fs::metadata(&path).unwrap();
        SSH_EXCHANGE_ERROR_AFTER_COMMIT.with(|flag| flag.set(true));
        let error = atomic_replace_ssh(&path, b"original\n", b"complete candidate\n", &original)
            .unwrap_err();
        assert!(
            error.contains("置換状態不明")
                && error.contains("退避先:")
                && error.contains("reloadは実行していません")
        );
        assert!(!error.contains("元の設定が維持"));
        assert_eq!(fs::read(&path).unwrap(), b"complete candidate\n");
        let directory = fs::read_dir(&dir.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|p| p != &path)
            .unwrap();
        let backup = directory.join("sshd_config.before");
        assert_eq!(fs::read(&backup).unwrap(), b"original\n");
        assert_eq!(fs::metadata(&backup).unwrap().ino(), original.ino());
    }

    #[test]
    fn isolated_exchange_probe_checks_filesystem_and_cleans_only_its_candidates() {
        for unsupported in [false, true] {
            let dir = TestDirectory::new();
            let untouched = dir.0.join("unrelated");
            fs::write(&untouched, b"unrelated file\n").unwrap();
            if unsupported {
                SSH_EXCHANGE_UNSUPPORTED.with(|flag| flag.set(true));
            }
            let result = probe_exchange_in_directory(&dir.0, unsafe { libc::geteuid() });
            assert_eq!(result.is_err(), unsupported);
            if unsupported {
                assert!(result.unwrap_err().contains("RENAME_EXCHANGE"));
            }
            assert_eq!(fs::read(&untouched).unwrap(), b"unrelated file\n");
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn mount_namespace_and_inode_flag_checks_reject_unsupported_states() {
        assert!(same_mount_ids("mnt_id:\t42\n", "mnt_id:\t42\n").is_ok());
        assert!(same_mount_ids("mnt_id:\t42\n", "mnt_id:\t43\n").is_err());
        for info in [
            "",
            "mnt_id:\twrong\n",
            "mnt_id:\t0\n",
            "mnt_id:\t42\nmnt_id:\t42\n",
            "mnt_id:\t42 extra\n",
        ] {
            assert!(parse_mount_id(info).is_err());
        }
        validate_initial_user_namespace(0x6e736673, 0xeffffffd, libc::CLONE_NEWUSER).unwrap();
        for (fs_type, inode, ns_type) in [
            (0, 0xeffffffd, libc::CLONE_NEWUSER),
            (0x6e736673, 12345, libc::CLONE_NEWUSER),
            (0x6e736673, 0xeffffffd, -1),
            (0x6e736673, 0xeffffffd, libc::CLONE_NEWNS),
        ] {
            assert!(validate_initial_user_namespace(fs_type, inode, ns_type).is_err());
        }
        for flags in [0, 0x80000] {
            validate_inode_flags(flags).unwrap();
        }
        for flags in [0x10, 0x40, 0x80010, -1, 0x40000000] {
            assert!(validate_inode_flags(flags).is_err());
        }
        let unsupported = File::open("/proc/version").unwrap();
        assert!(
            safe_inode_flags(&unsupported).is_err(),
            "取得不能なprocfs flagsを成功扱いにしてはいけません"
        );
    }

    #[test]
    fn native_open_descriptors_confirm_mount_identity_without_system_changes() {
        let dir = TestDirectory::new();
        let path = dir.0.join("synthetic");
        fs::write(&path, b"synthetic\n").unwrap();
        let file = File::open(&path).unwrap();
        let parent = File::open(&dir.0).unwrap();
        same_mount(&file, &parent).unwrap();
        assert!(same_mount(&File::open("/proc/version").unwrap(), &parent).is_err());
        let namespace = File::open("/proc/self/ns/user").unwrap();
        assert_eq!(
            initial_user_namespace().is_ok(),
            namespace.metadata().unwrap().ino() == 0xeffffffd
        );
        let bash = executable("bash").unwrap();
        let helpers = [
            "bash_ssh_metadata",
            "bash_mount_id_from_info",
            "bash_same_mount",
            "bash_validate_initial_user_namespace",
            "bash_initial_user_namespace",
        ]
        .map(installer_function)
        .join("\n");
        let script = format!(
            "set -euo pipefail\nexport LC_ALL=C\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{helpers}\nbash_same_mount \"$1\" \"$2\"\n"
        );
        let proc_path = PathBuf::from("/proc/version");
        for (target, success) in [(&path, true), (&proc_path, false)] {
            let result = Command::new(&bash)
                .args(["--noprofile", "--norc", "-c", &script, "mount-fixture"])
                .arg(target)
                .arg(&dir.0)
                .output()
                .unwrap();
            assert_eq!(
                result.status.success(),
                success,
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let result = Command::new(&bash)
            .args([
                "--noprofile",
                "--norc",
                "-c",
                &format!("{helpers}\nfail() {{ exit 1; }}\nbash_initial_user_namespace"),
            ])
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            namespace.metadata().unwrap().ino() == 0xeffffffd
        );
    }

    #[test]
    fn bash_pure_namespace_mount_and_flag_validation_fails_closed() {
        let bash = executable("bash").unwrap();
        let helpers = [
            "bash_validate_initial_user_namespace",
            "bash_mount_id_from_info",
            "bash_allowed_inode_flags",
            "bash_no_special_inode_flags",
        ]
        .map(installer_function)
        .join("\n");
        let script = format!(
            "set -euo pipefail\nfail() {{ exit 1; }}\n{helpers}\nbash_validate_initial_user_namespace 6e736673 4026531837 'user:[4026531837]'\nif bash_validate_initial_user_namespace 6e736673 1234 'user:[1234]'; then exit 2; fi\nif bash_validate_initial_user_namespace unknown 4026531837 'user:[4026531837]'; then exit 3; fi\n[[ $(bash_mount_id_from_info $'mnt_id:\\t42\\n') == 42 ]]\nfor info in '' $'mnt_id:\\tbad\\n' $'mnt_id:\\t0\\n' $'mnt_id:\\t42\\nmnt_id:\\t42\\n'; do if bash_mount_id_from_info \"$info\"; then exit 4; fi; done\nbash_allowed_inode_flags --------------e-------\nfor flags in ----i----------------- ------d--------------- unknown ''; do if bash_allowed_inode_flags \"$flags\"; then exit 5; fi; done\nlsattr() {{ return 9; }}\nif (bash_no_special_inode_flags /synthetic); then exit 6; fi\n"
        );
        let result = Command::new(bash)
            .args(["--noprofile", "--norc", "-c", &script])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn bash_cleanup_failure_always_reports_credentials_backup_and_original_exit_status() {
        let bash = executable("bash").unwrap();
        for (completed, exchange, original_status, expected_status, label) in [
            (1, "exchanged", 0, 1, "導入済み"),
            (0, "not-started", 17, 17, "導入未完了"),
            (0, "unknown", 23, 23, "置換状態不明"),
        ] {
            for failure in ["error", "false-success"] {
                let dir = TestDirectory::new();
                let stage = dir.0.join("stage");
                fs::create_dir(&stage).unwrap();
                fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
                let secret = stage.join("passwd");
                fs::write(&secret, b"synthetic credential\n").unwrap();
                let backup = dir.0.join("backup");
                fs::write(&backup, b"old inode stand-in\n").unwrap();
                let script = format!(
                    "set -euo pipefail\nexport LC_ALL=C\nstage=$1\nssh_backup_path=$2\nstage_identity=$(stat -c '%d:%i:%u:%g:%a' -- \"$stage\")\ninstallation_completed={completed}\nssh_exchange_state={exchange}\nrm() {{ return {}; }}\n{}\ntrap cleanup EXIT\nexit {original_status}\n",
                    if failure == "error" { 41 } else { 0 },
                    installer_function("cleanup")
                );
                let result = Command::new(&bash)
                    .args(["--noprofile", "--norc", "-c", &script, "cleanup-fixture"])
                    .arg(&stage)
                    .arg(&backup)
                    .output()
                    .unwrap();
                assert_eq!(result.status.code(), Some(expected_status));
                let error = String::from_utf8_lossy(&result.stderr);
                assert!(
                    error.contains(label)
                        && error.contains("資格情報")
                        && error.contains(stage.to_str().unwrap())
                        && error.contains(backup.to_str().unwrap()),
                    "{error}"
                );
                assert_eq!(fs::read(&secret).unwrap(), b"synthetic credential\n");
                assert_eq!(fs::read(&backup).unwrap(), b"old inode stand-in\n");
            }
        }
    }

    #[test]
    fn bash_password_variables_are_not_exported_and_readonly_failure_precedes_input() {
        let bash = executable("bash").unwrap();
        let function = installer_function("bash_prepare_password_variables");
        let script = format!(
            "set -euo pipefail\nset -a\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{function}\nbash_prepare_password_variables\npassword=synthetic-new-secret\nconfirmation=synthetic-new-secret\n\"$1\" --noprofile --norc -c '[[ -z ${{password+x}} && -z ${{confirmation+x}} ]]'\n"
        );
        let result = Command::new(&bash)
            .args(["--noprofile", "--norc", "-c", &script, "password-fixture"])
            .arg(&bash)
            .env("password", "inherited-export")
            .env("confirmation", "inherited-export")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let script = format!(
            "set -euo pipefail\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{function}\nreadonly password=inherited-readonly\nbash_prepare_password_variables\nprintf INPUT_REACHED\n"
        );
        let result = Command::new(&bash)
            .args(["--noprofile", "--norc", "-c", &script])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stdout).contains("INPUT_REACHED"));
        assert!(String::from_utf8_lossy(&result.stderr).contains("安全に初期化できません"));
    }

    #[test]
    fn bash_cleanup_does_not_delete_replaced_stage_and_success_keeps_backup() {
        let bash = executable("bash").unwrap();
        for replaced in [false, true] {
            let dir = TestDirectory::new();
            let stage = dir.0.join("stage");
            fs::create_dir(&stage).unwrap();
            fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(stage.join("passwd"), b"synthetic credential\n").unwrap();
            let backup = dir.0.join("backup");
            fs::write(&backup, b"old inode\n").unwrap();
            let mutation = if replaced {
                "mv -- \"$stage\" \"$stage.original\"\nmkdir -- \"$stage\"\nprintf 'other data' > \"$stage/foreign\"\n"
            } else {
                ""
            };
            let script = format!(
                "set -euo pipefail\nexport LC_ALL=C\nstage=$1\nssh_backup_path=$2\nstage_identity=$(stat -c '%d:%i:%u:%g:%a' -- \"$stage\")\ninstallation_completed=1\nssh_exchange_state=exchanged\n{}\ntrap cleanup EXIT\n{mutation}\nexit 0\n",
                installer_function("cleanup")
            );
            let result = Command::new(&bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &script,
                    "cleanup-identity-fixture",
                ])
                .arg(&stage)
                .arg(&backup)
                .output()
                .unwrap();
            assert_eq!(result.status.success(), !replaced);
            assert_eq!(fs::read(&backup).unwrap(), b"old inode\n");
            if replaced {
                assert_eq!(fs::read(stage.join("foreign")).unwrap(), b"other data");
                assert!(String::from_utf8_lossy(&result.stderr).contains("清掃に失敗"));
            } else {
                assert!(!stage.exists());
            }
        }
    }

    #[test]
    fn bash_no_replace_publication_succeeds_for_an_owned_new_path() {
        let bash = executable("bash").unwrap();
        let dir = TestDirectory::new();
        let candidate = dir.0.join("candidate");
        let target = dir.0.join("published");
        fs::write(&candidate, b"complete candidate\n").unwrap();
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o600)).unwrap();
        let functions = [
            "bash_ssh_metadata",
            "bash_ssh_hash",
            "bash_ssh_matches",
            "bash_publish_new",
        ]
        .map(installer_function)
        .join("\n");
        let script = format!(
            "set -euo pipefail\nfail() {{ exit 1; }}\n{functions}\nbash_publish_new \"$1\" \"$2\" \"$1\"\n"
        );
        let result = Command::new(&bash)
            .args([
                "--noprofile",
                "--norc",
                "-c",
                &script,
                "new-publication-fixture",
            ])
            .arg(&candidate)
            .arg(&target)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(fs::read(&target).unwrap(), b"complete candidate\n");
        assert_eq!(
            fs::metadata(&target).unwrap().ino(),
            fs::metadata(&candidate).unwrap().ino()
        );
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o7777, 0o600);
    }

    #[test]
    fn bash_no_replace_publication_preserves_racing_files_directories_symlinks_and_fifos() {
        let bash = executable("bash").unwrap();
        let functions = [
            "bash_ssh_metadata",
            "bash_ssh_hash",
            "bash_ssh_matches",
            "bash_publish_new",
            "bash_mkdir_new",
        ]
        .map(installer_function)
        .join("\n");
        for kind in ["file", "directory", "symlink", "fifo"] {
            let dir = TestDirectory::new();
            let target = dir.0.join("target");
            let candidate = dir.0.join("candidate");
            fs::write(&candidate, b"new candidate\n").unwrap();
            let other = dir.0.join("other");
            fs::write(&other, b"other existing data\n").unwrap();
            assert!(!target.exists());
            match kind {
                "file" => fs::write(&target, b"racing data\n").unwrap(),
                "directory" => fs::create_dir(&target).unwrap(),
                "symlink" => std::os::unix::fs::symlink(&other, &target).unwrap(),
                "fifo" => {
                    let name = CString::new(target.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
                _ => unreachable!(),
            }
            let before = fs::symlink_metadata(&target).unwrap();
            let action = if kind == "directory" {
                "bash_mkdir_new \"$2\" 755"
            } else {
                "bash_publish_new \"$1\" \"$2\" \"$1\""
            };
            let script = format!(
                "set -euo pipefail\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{functions}\n{action}\n"
            );
            let result = Command::new(&bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &script,
                    "publication-fixture",
                ])
                .arg(&candidate)
                .arg(&target)
                .output()
                .unwrap();
            assert!(!result.status.success(), "{kind}");
            let after = fs::symlink_metadata(&target).unwrap();
            assert_eq!(
                (after.dev(), after.ino(), after.mode()),
                (before.dev(), before.ino(), before.mode())
            );
            assert_eq!(fs::read(&candidate).unwrap(), b"new candidate\n");
            assert_eq!(fs::read(&other).unwrap(), b"other existing data\n");
            if kind == "file" {
                assert_eq!(fs::read(&target).unwrap(), b"racing data\n");
            }
        }
    }

    #[test]
    fn directory_ancestor_policy_accepts_root_sticky_tmp_and_rejects_untrusted_ancestors() {
        directory_permissions(0, 0, 0o755, false).unwrap();
        directory_permissions(0, 0, 0o1777, true).unwrap();
        for (uid, gid, mode, tmp) in [
            (0, 0, 0o777, true),
            (0, 0, 0o775, false),
            (1234, 0, 0o755, false),
            (0, 1234, 0o755, false),
            (0, 0, 0o2775, true),
        ] {
            assert!(directory_permissions(uid, gid, mode, tmp).is_err());
        }
        trusted_directory_chain(Path::new("/tmp")).unwrap();
        trusted_directory_chain(Path::new("/run")).unwrap();
        let dir = TestDirectory::new();
        let stage = dir.0.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("secret"), b"synthetic secret\n").unwrap();
        let kept = File::open(&stage).unwrap();
        let original = kept.metadata().unwrap();
        let moved = dir.0.join("old-stage");
        fs::rename(&stage, &moved).unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("other"), b"unrelated data\n").unwrap();
        assert!(same_directory_identity(&original, &fs::metadata(&stage).unwrap()).is_err());
        assert_eq!(
            fs::read(format!("/proc/self/fd/{}/secret", kept.as_raw_fd())).unwrap(),
            b"synthetic secret\n"
        );
        assert_eq!(fs::read(stage.join("other")).unwrap(), b"unrelated data\n");
        let functions = ["bash_directory_permissions"]
            .map(installer_function)
            .join("\n");
        let script = format!(
            "set -euo pipefail\n{functions}\nbash_directory_permissions 0 0 755 0\nbash_directory_permissions 0 0 1777 1\nif bash_directory_permissions 0 0 777 1; then exit 2; fi\nif bash_directory_permissions 0 0 775 0; then exit 3; fi\nif bash_directory_permissions 1000 0 755 0; then exit 4; fi\n"
        );
        assert!(
            Command::new(executable("bash").unwrap())
                .args(["--noprofile", "--norc", "-c", &script])
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn input_snapshot_accepts_caller_owned_files_and_rejects_third_party_write_access() {
        let caller = unsafe { libc::getuid() };
        input_permissions(caller, 0o644, caller, false).unwrap();
        input_permissions(0, 0o1777, caller, true).unwrap();
        assert!(input_permissions(1234, 0o644, 5678, false).is_err());
        assert!(input_permissions(caller, 0o664, caller, false).is_err());
        assert!(input_permissions(0, 0o777, caller, true).is_err());
        let dir = TestDirectory::new();
        let key = dir.0.join("key.pub");
        fs::write(&key, b"ssh-ed25519 original-snapshot\n").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        let snapshot = trusted_input_snapshot(&key, caller).unwrap();
        fs::write(&key, b"changed key\n").unwrap();
        assert_eq!(snapshot, b"ssh-ed25519 original-snapshot\n");
        fs::set_permissions(&key, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(trusted_input_snapshot(&key, caller).is_err());
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(trusted_input_snapshot(&key, caller).is_err());
        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o700)).unwrap();
        let template = dir.0.join("squid.conf.in");
        let service = dir.0.join("unit.service");
        let output = dir.0.join("fixed.service");
        fs::write(&template, crate::SQUID_TEMPLATE).unwrap();
        fs::write(&service, crate::SERVICE).unwrap();
        fs::set_permissions(&template, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&service, fs::Permissions::from_mode(0o644)).unwrap();
        let functions = [
            "bash_input_permissions",
            "bash_input_snapshot_file",
            "render_squid",
        ]
        .map(installer_function)
        .join("\n");
        for source in [&template, &service] {
            fs::set_permissions(source, fs::Permissions::from_mode(0o664)).unwrap();
            let reject = format!(
                "set -euo pipefail\ninput_caller_uid=$1\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{functions}\nbash_input_snapshot_file \"$2\"\n"
            );
            let result = Command::new(executable("bash").unwrap())
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &reject,
                    "unsafe-source-fixture",
                ])
                .arg(caller.to_string())
                .arg(source)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("第三者が書込み可能"));
            fs::set_permissions(source, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let script = format!(
            "set -euo pipefail\ninput_caller_uid=$1\npublic_port=3128\nremote_port=43129\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{functions}\nbash_input_snapshot_file \"$2\"\ntemplate_snapshot=$input_snapshot\nbash_input_snapshot_file \"$3\"\nservice_snapshot=$input_snapshot\nprintf 'changed template' > \"$2\"\nprintf 'changed service' > \"$3\"\nrender_squid\nprintf '%s' \"$service_snapshot\" | base64 --decode > \"$4\"\n"
        );
        let result = Command::new(executable("bash").unwrap())
            .args([
                "--noprofile",
                "--norc",
                "-c",
                &script,
                "source-snapshot-fixture",
            ])
            .arg(caller.to_string())
            .arg(&template)
            .arg(&service)
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            crate::Options::parse(["--public-key-file".into(), "unused".into()])
                .unwrap()
                .unwrap()
                .squid()
        );
        assert_eq!(fs::read(output).unwrap(), crate::SERVICE.as_bytes());
    }

    #[test]
    fn bash_install_lock_never_truncates_existing_data_or_followed_race_target() {
        let bash = executable("bash").unwrap();
        let stat = executable("stat").unwrap();
        for race in [false, true] {
            let dir = TestDirectory::new();
            let lock = dir.0.join("lock");
            let data = dir.0.join("other");
            fs::write(&data, b"unrelated existing data\n").unwrap();
            fs::write(
                &lock,
                if race {
                    b"".as_slice()
                } else {
                    b"existing lock data\n".as_slice()
                },
            )
            .unwrap();
            fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
            let shim = if race {
                "stat() { if [[ $* == *'%u:%g:%a:%h:%s'* && $* != *-L* ]]; then mv -- \"$lock\" \"$lock.original\"; ln -s -- \"$data\" \"$lock\"; printf '0:0:600:1:0\\n'; else \"$real_stat\" \"$@\"; fi; }\n"
            } else {
                ""
            };
            let script = format!(
                "set -euo pipefail\numask 077\nlock=$1\ndata=$2\nreal_stat=$3\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{shim}\n{}\nbash_acquire_install_lock \"$lock\"\n",
                installer_function("bash_acquire_install_lock")
            );
            let result = Command::new(&bash)
                .args(["--noprofile", "--norc", "-c", &script, "lock-fixture"])
                .arg(&lock)
                .arg(&data)
                .arg(&stat)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert_eq!(fs::read(&data).unwrap(), b"unrelated existing data\n");
            if !race {
                assert_eq!(fs::read(&lock).unwrap(), b"existing lock data\n");
            }
        }
    }

    #[test]
    fn bash_creation_uses_pinned_parent_when_name_is_replaced_after_validation() {
        let bash = executable("bash").unwrap();
        let functions = [
            "bash_parent_reference_matches",
            "bash_retained_path",
            "bash_mkdir_new",
            "bash_write_new",
        ]
        .map(installer_function)
        .join("\n");
        for action in ["mkdir", "write"] {
            let dir = TestDirectory::new();
            let parent = dir.0.join("parent");
            fs::create_dir(&parent).unwrap();
            let source = dir.0.join("payload");
            fs::write(&source, b"synthetic payload\n").unwrap();
            // 所有権の前提は別試験済み。ここでは本人所有の合成木でroot同時管理のrenameを再現する。
            let injected = if action == "mkdir" {
                "mkdir() { command mv -- \"$test_parent\" \"$test_parent.moved\"; command mkdir -- \"$test_parent\"; printf 'unrelated' > \"$test_parent/other\"; command mkdir \"$@\"; }"
            } else {
                "mktemp() { command mv -- \"$test_parent\" \"$test_parent.moved\"; command mkdir -- \"$test_parent\"; printf 'unrelated' > \"$test_parent/other\"; command mktemp \"$@\"; }"
            };
            let call = if action == "mkdir" {
                "bash_mkdir_new \"$test_parent/created\" 755"
            } else {
                "bash_write_new \"$source\" \"$test_parent/created\" 600"
            };
            let script = format!(
                "set -euo pipefail\numask 077\ntest_parent=$1\nsource=$2\nbash_pin_holder=$BASHPID\nbash_pins_ready=1\ndeclare -A bash_dir_fds=()\nexec {{held_parent}}< \"$test_parent\"\nbash_dir_fds[$test_parent]=$held_parent\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\nbash_verify_creation_parent() {{ :; }}\n{functions}\n{injected}\n{call}\n"
            );
            let result = Command::new(&bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &script,
                    "creation-pin-fixture",
                ])
                .arg(&parent)
                .arg(&source)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("親参照が変わりました"));
            assert_eq!(fs::read(parent.join("other")).unwrap(), b"unrelated");
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
            let moved = dir.0.join("parent.moved");
            if action == "mkdir" {
                assert!(moved.join("created").is_dir());
            } else {
                assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
            }
            assert_eq!(fs::read(&source).unwrap(), b"synthetic payload\n");
        }
    }

    #[test]
    fn ssh_exchange_keeps_creation_and_exchange_in_the_open_parent_after_rename() {
        for phase in ["candidate", "exchange"] {
            let dir = TestDirectory::new();
            let parent = dir.0.join("parent");
            let moved = dir.0.join("parent.moved");
            fs::create_dir(&parent).unwrap();
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
            let target = parent.join("sshd_config");
            fs::write(&target, b"original\n").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            let original = fs::metadata(&target).unwrap();
            let destination = moved.clone();
            if phase == "candidate" {
                BEFORE_SSH_CANDIDATE_CREATE.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move |parent| {
                        fs::rename(parent, &destination).unwrap();
                        fs::create_dir(parent).unwrap();
                        fs::write(
                            parent.join("sshd_config"),
                            b"unrelated current configuration\n",
                        )
                        .unwrap();
                    }));
                });
            } else {
                BEFORE_SSH_EXCHANGE.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move |target| {
                        let parent = target.parent().unwrap();
                        fs::rename(parent, &destination).unwrap();
                        fs::create_dir(parent).unwrap();
                        fs::write(
                            parent.join("sshd_config"),
                            b"unrelated current configuration\n",
                        )
                        .unwrap();
                    }));
                });
            }
            let error =
                atomic_replace_ssh(&target, b"original\n", b"complete candidate\n", &original)
                    .unwrap_err();
            assert!(error.contains(moved.to_str().unwrap()), "{error}");
            assert_eq!(
                fs::read(&target).unwrap(),
                b"unrelated current configuration\n"
            );
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
            let backup_dir = fs::read_dir(&moved)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.is_dir())
                .unwrap();
            if phase == "candidate" {
                assert_eq!(fs::read(moved.join("sshd_config")).unwrap(), b"original\n");
                assert_eq!(fs::read_dir(backup_dir).unwrap().count(), 0);
            } else {
                assert_eq!(
                    fs::read(moved.join("sshd_config")).unwrap(),
                    b"complete candidate\n"
                );
                assert_eq!(
                    fs::read(backup_dir.join("sshd_config.before")).unwrap(),
                    b"original\n"
                );
                assert!(error.contains("置換状態不明"));
            }
        }
    }

    #[test]
    fn creation_parent_default_acl_is_rejected_before_any_new_files() {
        let dir = TestDirectory::new();
        let original = dir.0.join("existing");
        fs::write(&original, b"existing parent data\n").unwrap();
        let directory = File::open(&dir.0).unwrap();
        creation_parent_attributes(&directory).unwrap();
        set_xattr(&directory, c"system.posix_acl_default", &default_acl());
        let error = creation_parent_attributes(&directory).unwrap_err();
        assert!(error.contains("default ACL"));
        assert_eq!(fs::read(&original).unwrap(), b"existing parent data\n");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        let functions = [
            "bash_no_default_acl",
            "bash_allowed_inode_flags",
            "bash_no_special_inode_flags",
            "bash_creation_parent_attributes",
        ]
        .map(installer_function)
        .join("\n");
        let script = format!(
            "set -euo pipefail\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\ngetfacl() {{ printf 'default:user::rwx\\n'; }}\nlsattr() {{ printf '%s %s\\n' '--------------e-------' \"$1\"; }}\n{functions}\nbash_creation_parent_attributes \"$1\"\nprintf MUTATION_REACHED\n"
        );
        let result = Command::new(executable("bash").unwrap())
            .args(["--noprofile", "--norc", "-c", &script, "parent-acl-fixture"])
            .arg(&dir.0)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("default ACL"));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("MUTATION_REACHED"));
        assert_eq!(fs::read(&original).unwrap(), b"existing parent data\n");
    }

    #[test]
    fn bash_exchange_preflight_refuses_missing_tools_and_old_mv_before_writing() {
        let bash = executable("bash").expect("Bash互換経路の検証にはBashが必要です");
        let tools = [
            "mv", "getfattr", "getfacl", "sync", "readlink", "lsattr", "ln", "mkdir", "chmod",
            "dirname", "rm", "rmdir", "cat", "cp", "sleep",
        ];
        for failure in tools.into_iter().chain(std::iter::once("old-mv")) {
            let dir = TestDirectory::new();
            let untouched = dir.0.join("unrelated");
            fs::write(&untouched, b"unrelated file\n").unwrap();
            // 交換APIそのものは偽装せず、ここでは実helperの前提判定だけを分離検証する。
            let mut tool_stubs = String::new();
            for tool in tools {
                if tool == failure {
                    continue;
                }
                if tool == "mv" {
                    tool_stubs.push_str(if failure == "old-mv" {
                        "mv() { if [[ $1 == --version ]]; then printf 'mv (GNU coreutils) 9.4\\n'; else printf 'Usage: mv SOURCE DEST\\n'; fi; }\n"
                    } else {
                        "mv() { if [[ $1 == --version ]]; then printf 'mv (GNU coreutils) fixture\\n'; else printf '%s\\n' '--exchange --no-copy'; fi; }\n"
                    });
                } else if tool == "ln" {
                    tool_stubs.push_str("ln() { printf '%s\\n' '--no-target-directory'; }\n");
                } else {
                    tool_stubs.push_str(&format!("{tool}() {{ :; }}\n"));
                }
            }
            let script = format!(
                "set -euo pipefail\nPATH=/nonexistent-plumivox-tools\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\n{tool_stubs}\n{}\nbash_require_exchange_tools\n",
                installer_function("bash_require_exchange_tools")
            );
            let result = Command::new(&bash)
                .args(["--noprofile", "--norc", "-c", &script])
                .current_dir(&dir.0)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(!result.status.success(), "{failure}");
            let error = String::from_utf8_lossy(&result.stderr);
            let expected = if failure == "old-mv" {
                "--exchange/--no-copyに未対応".to_string()
            } else {
                format!("必要なコマンドがありません: {failure}")
            };
            assert!(error.contains(&expected), "{failure}: {error}");
            assert_eq!(fs::read(&untouched).unwrap(), b"unrelated file\n");
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        }
    }

    #[test]
    #[ignore = "requires GNU mv --exchange/--no-copy plus getfattr/getfacl; uses only synthetic files, no root installation"]
    fn bash_atomic_exchange_preserves_private_backup_and_reports_uncertain_results() {
        let bash = executable("bash").expect("Bashが必要です");
        let mv = match std::env::var_os("PLUMIVOX_TEST_GNU_MV") {
            Some(path) => {
                let path = PathBuf::from(path);
                assert!(
                    path.is_absolute(),
                    "PLUMIVOX_TEST_GNU_MVには実GNU mvの絶対パスが必要です"
                );
                assert!(
                    fs::metadata(&path).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0),
                    "指定した実GNU mvを実行できません"
                );
                path
            }
            None => executable("mv").expect("GNU mvが必要です"),
        };
        let functions = [
            "bash_ssh_metadata",
            "bash_ssh_hash",
            "bash_ssh_matches",
            "bash_require_exchange_tools",
            "bash_no_xattrs",
            "bash_no_default_acl",
            "bash_mount_id_from_info",
            "bash_same_mount",
            "bash_allowed_inode_flags",
            "bash_no_special_inode_flags",
            "bash_parent_reference_matches",
            "bash_retained_path",
            "bash_atomic_replace_ssh",
        ]
        .map(installer_function)
        .join("\n");
        for condition in [
            "normal",
            "error-after-exchange",
            "concurrent-update",
            "flags-race",
        ] {
            let dir = TestDirectory::new();
            let target = dir.0.join("sshd_config");
            let before = dir.0.join("before");
            let candidate = dir.0.join("candidate");
            fs::write(&target, b"original\n").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&before, b"original\n").unwrap();
            fs::write(&candidate, b"complete candidate\n").unwrap();
            let original = fs::metadata(&target).unwrap();
            let script = format!(
                "set -euo pipefail\numask 077\nexport LC_ALL=C\ntarget=$1\nbefore=$2\ncandidate=$3\ncondition=$4\nreal_mv=$5\nreal_lsattr=$6\nexchange_done=0\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\nmv() {{ local status=0; if [[ ${{1:-}} == --exchange && $condition == concurrent-update ]]; then printf 'concurrent administrator data\\n' > \"$target\"; fi; \"$real_mv\" \"$@\" || status=$?; if [[ ${{1:-}} == --exchange && $status == 0 ]]; then exchange_done=1; if [[ $condition == error-after-exchange ]]; then return 1; fi; fi; return \"$status\"; }}\nlsattr() {{ if [[ $condition == flags-race && $exchange_done == 1 ]]; then printf '%s %s\\n' '------d---------------' \"$target\"; else \"$real_lsattr\" \"$@\"; fi; }}\n{functions}\nbash_require_exchange_tools\nexpected=$(bash_ssh_metadata \"$target\")\nexec {{old_fd}}>> \"$target\"\nbash_atomic_replace_ssh \"$target\" \"$before\" \"$candidate\" \"$expected\"\nprintf '# late administrator write\\n' >&\"$old_fd\"\nexec {{old_fd}}>&-\nprintf '%s\\n' \"$ssh_backup_path\"\n"
            );
            let result = Command::new(&bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &script,
                    "plumivox-bash-atomic-regression",
                ])
                .arg(&target)
                .arg(&before)
                .arg(&candidate)
                .arg(condition)
                .arg(&mv)
                .arg(executable("lsattr").expect("実lsattrが必要です"))
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(
                result.status.success(),
                condition == "normal",
                "{condition}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let backups: Vec<_> = fs::read_dir(&dir.0)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.is_dir())
                .collect();
            assert_eq!(backups.len(), 1, "{condition}");
            let backup = backups[0].join("sshd_config.before");
            assert_eq!(fs::metadata(&backups[0]).unwrap().mode() & 0o7777, 0o700);
            assert_eq!(
                fs::read(&target).unwrap(),
                b"complete candidate\n",
                "{condition}"
            );
            if condition == "normal" {
                assert_eq!(
                    fs::read(&backup).unwrap(),
                    b"original\n# late administrator write\n"
                );
                assert_eq!(fs::metadata(&backup).unwrap().ino(), original.ino());
                assert_eq!(fs::metadata(&target).unwrap().mode() & 0o7777, 0o600);
            } else {
                assert!(String::from_utf8_lossy(&result.stderr).contains("置換状態不明"));
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains("reloadは実行していません")
                );
                assert_eq!(
                    fs::read(&backup).unwrap(),
                    if condition == "concurrent-update" {
                        b"concurrent administrator data\n".as_slice()
                    } else {
                        b"original\n".as_slice()
                    }
                );
            }
        }
        for phase in ["candidate-parent", "exchange-parent"] {
            let dir = TestDirectory::new();
            let parent = dir.0.join("parent");
            fs::create_dir(&parent).unwrap();
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
            let target = parent.join("sshd_config");
            let before = dir.0.join("before");
            let candidate = dir.0.join("candidate");
            fs::write(&target, b"original\n").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&before, b"original\n").unwrap();
            fs::write(&candidate, b"complete candidate\n").unwrap();
            let injected = if phase == "candidate-parent" {
                "mktemp() { \"$real_mv\" -- \"$test_parent\" \"$test_parent.moved\"; command mkdir -- \"$test_parent\"; printf 'unrelated current configuration\\n' > \"$test_parent/sshd_config\"; \"$real_mktemp\" \"$@\"; }"
            } else {
                "mv() { if [[ ${1:-} == --exchange ]]; then \"$real_mv\" -- \"$test_parent\" \"$test_parent.moved\"; command mkdir -- \"$test_parent\"; printf 'unrelated current configuration\\n' > \"$test_parent/sshd_config\"; fi; \"$real_mv\" \"$@\"; }"
            };
            let script = format!(
                "set -euo pipefail\numask 077\nexport LC_ALL=C\ntest_parent=$1\ntarget=$1/sshd_config\nbefore=$2\ncandidate=$3\nreal_mv=$4\nreal_mktemp=$5\nfail() {{ printf '%s\\n' \"$*\" >&2; exit 1; }}\nmv() {{ \"$real_mv\" \"$@\"; }}\n{functions}\n{injected}\nbash_require_exchange_tools\nexpected=$(bash_ssh_metadata \"$target\")\nbash_atomic_replace_ssh \"$target\" \"$before\" \"$candidate\" \"$expected\"\n"
            );
            let result = Command::new(&bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &script,
                    "ssh-parent-pin-fixture",
                ])
                .arg(&parent)
                .arg(&before)
                .arg(&candidate)
                .arg(&mv)
                .arg(executable("mktemp").unwrap())
                .output()
                .unwrap();
            assert!(!result.status.success());
            let error = String::from_utf8_lossy(&result.stderr);
            assert!(
                error.contains("親参照") && error.contains("reloadは実行していません"),
                "{phase}: {error}"
            );
            assert_eq!(
                fs::read(&target).unwrap(),
                b"unrelated current configuration\n"
            );
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
            let moved = dir.0.join("parent.moved");
            assert!(error.contains(moved.to_str().unwrap()), "{error}");
            let backup_dir = fs::read_dir(&moved)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.is_dir())
                .unwrap();
            if phase == "candidate-parent" {
                assert_eq!(fs::read(moved.join("sshd_config")).unwrap(), b"original\n");
                assert_eq!(fs::read_dir(backup_dir).unwrap().count(), 0);
            } else {
                assert_eq!(
                    fs::read(moved.join("sshd_config")).unwrap(),
                    b"complete candidate\n"
                );
                assert_eq!(
                    fs::read(backup_dir.join("sshd_config.before")).unwrap(),
                    b"original\n"
                );
            }
        }
    }

    #[test]
    fn atomic_ssh_replace_refuses_concurrent_content_mode_and_inode_changes() {
        for change in ["contents", "permissions", "inode"] {
            let dir = TestDirectory::new();
            let path = dir.0.join("sshd_config");
            fs::write(&path, b"Port 22\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let original = fs::metadata(&path).unwrap();
            match change {
                "contents" => fs::write(&path, b"Port 23\n").unwrap(),
                "permissions" => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap()
                }
                "inode" => {
                    let alternate = dir.0.join("alternate");
                    fs::write(&alternate, b"Port 24\n").unwrap();
                    fs::rename(&alternate, &path).unwrap();
                }
                _ => unreachable!(),
            }
            let expected = fs::read(&path).unwrap();
            let mode = fs::metadata(&path).unwrap().mode();
            assert!(
                atomic_replace_ssh(&path, b"Port 22\n", b"replacement\n", &original).is_err(),
                "{change}"
            );
            assert_eq!(fs::read(&path).unwrap(), expected, "{change}");
            assert_eq!(fs::metadata(&path).unwrap().mode(), mode, "{change}");
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1, "{change}");
        }
    }
}
