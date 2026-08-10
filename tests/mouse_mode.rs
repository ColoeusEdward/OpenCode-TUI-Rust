//! Verifies that mouse capture setup actually arms the platform's mouse channel.
//!
//! On Windows this is the whole point of the `mouse` module: Crossterm's event
//! source reads console input records, and those only carry mouse events when the
//! console input handle has `ENABLE_MOUSE_INPUT`. Writing ANSI tracking sequences
//! leaves that bit clear, which silently disables every `Event::Mouse`.
//!
//! The test drives Crossterm's own commands rather than the binary's private
//! `mouse` module, so it asserts the platform contract the fix depends on.

#![cfg(windows)]

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

const ENABLE_MOUSE_INPUT: u32 = 0x0010;

/// Reads the current console input mode, or returns `None` when the test runner
/// has no real console attached (e.g. fully redirected CI output).
fn console_input_mode() -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    use std::ptr;

    unsafe extern "system" {
        fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut std::ffi::c_void,
            disposition: u32,
            flags: u32,
            template: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    // CONIN$ resolves to the process's console input buffer even when stdin is
    // redirected, which is how the test runner usually starts.
    let name: Vec<u16> = "CONIN$\0".encode_utf16().collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0x8000_0000 | 0x4000_0000, // GENERIC_READ | GENERIC_WRITE
            0x0000_0001 | 0x0000_0002, // FILE_SHARE_READ | FILE_SHARE_WRITE
            ptr::null_mut(),
            3, // OPEN_EXISTING
            0,
            ptr::null_mut(),
        )
    };
    if handle.is_null() || handle as isize == -1 {
        let _ = std::io::stdin().as_raw_handle();
        return None;
    }
    let mut mode = 0u32;
    let ok = unsafe { GetConsoleMode(handle, &mut mode) };
    unsafe { CloseHandle(handle) };
    (ok != 0).then_some(mode)
}

#[test]
fn enabling_mouse_capture_sets_the_console_mouse_input_bit() {
    let Some(original) = console_input_mode() else {
        eprintln!("skipping: no console input buffer available in this environment");
        return;
    };

    let mut stdout = std::io::stdout();
    execute!(stdout, EnableMouseCapture).expect("mouse capture should be enabled");
    let enabled = console_input_mode().expect("console mode should still be readable");
    execute!(stdout, DisableMouseCapture).expect("mouse capture should be disabled");
    let restored = console_input_mode().expect("console mode should still be readable");

    assert_ne!(
        enabled & ENABLE_MOUSE_INPUT,
        0,
        "enabling mouse capture must set ENABLE_MOUSE_INPUT; without it the Windows \
         event source never produces Event::Mouse and wheel routing cannot work"
    );
    assert_eq!(
        restored, original,
        "disabling mouse capture should restore the original console input mode"
    );
}
