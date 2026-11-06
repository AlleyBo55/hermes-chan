//! When the plugin starts her it passes `HERMESCHAN_PARENT_PID`. She leaves when that
//! Hermes process is gone, even after a crash, instead of lingering on the desktop and
//! doubling up with the body the next Hermes starts.

use tauri::AppHandle;

pub fn spawn(handle: AppHandle) {
    let Some(pid) = std::env::var("HERMESCHAN_PARENT_PID")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 1 && *pid != std::process::id())
    else {
        return;
    };
    std::thread::spawn(move || {
        if wait_for_exit(pid) {
            handle.exit(0);
        }
    });
}

/// Blocks until `pid` exits. False when the process cannot be watched at all.
#[cfg(unix)]
fn wait_for_exit(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    loop {
        // Signal 0 checks existence only. EPERM means it exists but belongs to someone else.
        let gone = unsafe { libc::kill(pid, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if gone {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(windows)]
fn wait_for_exit(pid: u32) -> bool {
    use windows::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE},
    };
    match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
        Ok(process) => {
            let result = unsafe { WaitForSingleObject(process, INFINITE) };
            let _ = unsafe { CloseHandle(process) };
            result == WAIT_OBJECT_0
        }
        // No such process: Hermes is already gone.
        Err(error) => error.code() == ERROR_INVALID_PARAMETER.to_hresult(),
    }
}

#[cfg(not(any(unix, windows)))]
fn wait_for_exit(_pid: u32) -> bool {
    false
}
