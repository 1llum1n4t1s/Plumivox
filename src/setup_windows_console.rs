//! 秘密入力中だけ制御イベントを監視し、同じ所有状態から端末モードを復元する。
use super::fail;
use crate::error::Result;
use std::{
    fs::{File, OpenOptions},
    os::windows::io::AsRawHandle,
    sync::{Mutex, MutexGuard},
};
use windows_sys::Win32::System::Console::*;

struct Active {
    input: File,
    original: u32,
    interrupted: bool,
}
struct State {
    active: Option<Active>,
    registered: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    active: None,
    registered: false,
});
static SERIAL: Mutex<()> = Mutex::new(());
fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|error| error.into_inner())
}

unsafe extern "system" fn control(event: u32) -> i32 {
    if matches!(
        event,
        CTRL_C_EVENT
            | CTRL_BREAK_EVENT
            | CTRL_CLOSE_EVENT
            | CTRL_LOGOFF_EVENT
            | CTRL_SHUTDOWN_EVENT
    ) {
        let mut state = state();
        if let Some(active) = state.active.as_mut() {
            active.interrupted = true;
            // handlerと通常復元は同じlockを使い、閉じたhandleへアクセスしない。
            unsafe {
                SetConsoleMode(active.input.as_raw_handle(), active.original);
            }
        }
    }
    // 既定のCtrl+C/Break/Close等の終了動作を維持する。
    0
}

struct Mode {
    _serial: MutexGuard<'static, ()>,
    armed: bool,
}
impl Mode {
    fn restore(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let mut state = state();
        if let Some(active) = state.active.as_ref() {
            if unsafe { SetConsoleMode(active.input.as_raw_handle(), active.original) } == 0 {
                return Err(fail("INPUT_REQUIRED"));
            }
        }
        if state.registered {
            if unsafe { SetConsoleCtrlHandler(Some(control), 0) } == 0 {
                return Err(fail("INPUT_REQUIRED"));
            }
            state.registered = false;
        }
        state.active.take();
        self.armed = false;
        Ok(())
    }
    fn interrupted(&self) -> bool {
        state()
            .active
            .as_ref()
            .is_none_or(|active| active.interrupted)
    }
}
impl Drop for Mode {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub(super) fn secret(prompt: &str) -> Result<String> {
    let serial = SERIAL.lock().map_err(|_| fail("INPUT_REQUIRED"))?;
    let input = OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")
        .map_err(|_| fail("INPUT_REQUIRED"))?;
    let output = OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONOUT$")
        .map_err(|_| fail("INPUT_REQUIRED"))?;
    let mut original = 0;
    let mut output_mode = 0;
    if unsafe { GetConsoleMode(input.as_raw_handle(), &mut original) } == 0
        || unsafe { GetConsoleMode(output.as_raw_handle(), &mut output_mode) } == 0
    {
        return Err(fail("INPUT_REQUIRED"));
    }
    let input_handle = input.as_raw_handle();
    let mut mode = Mode {
        _serial: serial,
        armed: true,
    };
    {
        let mut state = state();
        // 前回の復元が失敗した状態で新しい入力を始めない。
        if state.active.is_some() || state.registered {
            mode.armed = false;
            return Err(fail("INPUT_REQUIRED"));
        }
        state.active = Some(Active {
            input,
            original,
            interrupted: false,
        });
        if unsafe { SetConsoleCtrlHandler(Some(control), 1) } == 0 {
            state.active.take();
            mode.armed = false;
            return Err(fail("INPUT_REQUIRED"));
        }
        state.registered = true;
        if unsafe {
            SetConsoleMode(
                input_handle,
                (original | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT) & !ENABLE_ECHO_INPUT,
            )
        } == 0
        {
            drop(state);
            return Err(fail("INPUT_REQUIRED"));
        }
    }
    let prompt = format!("{prompt}: ").encode_utf16().collect::<Vec<_>>();
    let result = (|| {
        let mut offset = 0;
        while offset < prompt.len() {
            if mode.interrupted() {
                return Err(fail("INPUT_REQUIRED"));
            }
            let mut written = 0;
            if unsafe {
                WriteConsoleW(
                    output.as_raw_handle(),
                    prompt.as_ptr().add(offset),
                    u32::try_from(prompt.len() - offset).map_err(|_| fail("INPUT_REQUIRED"))?,
                    &mut written,
                    std::ptr::null(),
                )
            } == 0
                || written == 0
            {
                return Err(fail("INPUT_REQUIRED"));
            }
            offset += written as usize;
        }
        let mut text = zeroize::Zeroizing::new(Vec::<u16>::new());
        loop {
            if mode.interrupted() {
                return Err(fail("INPUT_REQUIRED"));
            }
            let mut buffer = zeroize::Zeroizing::new([0u16; 256]);
            let mut read = 0;
            if unsafe {
                ReadConsoleW(
                    input_handle,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                    std::ptr::null(),
                )
            } == 0
                || read == 0
                || mode.interrupted()
            {
                return Err(fail("INPUT_REQUIRED"));
            }
            text.extend_from_slice(&buffer[..read as usize]);
            if text
                .last()
                .is_some_and(|last| *last == b'\n' as u16 || *last == b'\r' as u16)
            {
                break;
            }
        }
        while text
            .last()
            .is_some_and(|last| *last == b'\n' as u16 || *last == b'\r' as u16)
        {
            text.pop();
        }
        let value = String::from_utf16(&text).map_err(|_| fail("INPUT_REQUIRED"))?;
        let newline = [13u16, 10u16];
        let mut written = 0;
        if unsafe {
            WriteConsoleW(
                output.as_raw_handle(),
                newline.as_ptr(),
                2,
                &mut written,
                std::ptr::null(),
            )
        } == 0
            || written != 2
        {
            return Err(fail("INPUT_REQUIRED"));
        }
        Ok(value)
    })();
    mode.restore()?;
    result
}
