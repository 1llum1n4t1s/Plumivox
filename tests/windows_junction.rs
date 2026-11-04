use anyhow::{Context, Result, ensure};
use std::{fs, os::windows::io::AsRawHandle, path::Path};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(
        handle: *mut std::ffi::c_void,
        code: u32,
        input: *const std::ffi::c_void,
        input_length: u32,
        output: *mut std::ffi::c_void,
        output_length: u32,
        returned: *mut u32,
        overlapped: *mut std::ffi::c_void,
    ) -> i32;
}

pub fn directory_junction(link: &Path, target: &Path) -> Result<()> {
    use std::os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    };
    ensure!(
        link.is_absolute() && target.is_absolute() && !link.exists(),
        "Junction fixture requires a new absolute link path"
    );
    ensure!(
        fs::symlink_metadata(target)?.is_dir()
            && fs::symlink_metadata(target)?.file_attributes() & 0x400 == 0,
        "Junction target must be a real fixture directory"
    );
    let target = fs::canonicalize(target)?;
    let display = target.to_str().context("Fixture target must be Unicode")?;
    let display = display.strip_prefix(r"\\?\").unwrap_or(display);
    ensure!(
        !display.starts_with(r"\\"),
        "Junction fixture target must be a local drive"
    );
    let substitute: Vec<u16> = format!(r"\??\{display}").encode_utf16().collect();
    let printable: Vec<u16> = std::ffi::OsStr::new(display).encode_wide().collect();
    let path_bytes = (substitute.len() + printable.len() + 2) * 2;
    ensure!(
        path_bytes + 8 <= u16::MAX as usize,
        "Junction fixture path is too long"
    );
    let mut buffer = Vec::with_capacity(path_bytes + 16);
    buffer.extend_from_slice(&0xa0000003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
    for value in [
        (path_bytes + 8) as u16,
        0,
        0,
        (substitute.len() * 2) as u16,
        ((substitute.len() + 1) * 2) as u16,
        (printable.len() * 2) as u16,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for value in substitute
        .into_iter()
        .chain([0])
        .chain(printable)
        .chain([0])
    {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    fs::create_dir(link)?;
    let directory = fs::OpenOptions::new()
        .access_mode(0x40000000)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(link)?;
    let mut returned = 0;
    ensure!(
        unsafe {
            DeviceIoControl(
                directory.as_raw_handle(),
                0x000900a4,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        } != 0,
        "Cannot create unprivileged junction fixture: {}",
        std::io::Error::last_os_error()
    );
    ensure!(
        fs::symlink_metadata(link)?.file_attributes() & 0x400 != 0
            && fs::canonicalize(link)? == target,
        "Junction fixture did not resolve to the requested target"
    );
    Ok(())
}
