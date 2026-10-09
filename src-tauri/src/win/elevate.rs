// Elevation: detect an admin token, and launch a single operation through the
// UAC consent prompt (ShellExecuteW "runas"). Only ever invoked with a fixed
// program + pre-validated arguments — argument strings are quoted defensively
// and callers pass only names/pids that passed the strict validators.

use crate::error::{AppError, AppResult};
use std::os::windows::ffi::OsStrExt;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// Is this process running with an administrator token (elevated)?
/// TokenElevation — true only when the whole token is elevated (UAC).
pub fn is_elevated() -> bool {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    unsafe {
        let mut token = Default::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut needed = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut needed,
        );
        let _ = CloseHandle(token);
        ok.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Quote one argument for a CreateProcess command line. Callers guarantee no
/// embedded quotes (validators reject them); the quoting covers spaces.
fn quote(arg: &str) -> String {
    if arg.contains(' ') || arg.is_empty() {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

/// Run `program` with `args` hidden, elevated via the UAC prompt, and WAIT for
/// it to finish. Returns its exit code. The user can decline the prompt — that
/// maps to Cancelled, never a crash.
pub fn elevated_wait(program: &str, args: &[&str]) -> AppResult<i32> {
    let params = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let prog_w: Vec<u16> = program.encode_utf16().chain(std::iter::once(0)).collect();
    let params_w: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();
    let dir_w: Vec<u16> = std::env::current_dir()
        .map(|d| {
            d.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        })
        .unwrap_or_else(|_| vec![0]);
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = w!("runas");
        info.lpFile = PCWSTR(prog_w.as_ptr());
        info.lpParameters = PCWSTR(params_w.as_ptr());
        info.lpDirectory = PCWSTR(dir_w.as_ptr());
        info.nShow = SW_HIDE.0;

        if let Err(e) = ShellExecuteExW(&mut info) {
            if e.code() == ERROR_CANCELLED.to_hresult() {
                // The user declined the UAC prompt.
                return Err(AppError::Cancelled(
                    "Administrator approval was declined — nothing changed.".into(),
                ));
            }
            return Err(AppError::Shell(format!(
                "could not launch elevated helper: {e}"
            )));
        }
        if info.hProcess.is_invalid() {
            return Err(AppError::Shell("elevated helper did not start".into()));
        }
        let _ = WaitForSingleObject(info.hProcess, 0xFFFFFFFF);
        let mut exit_code = 0u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut exit_code);
        let _ = CloseHandle(info.hProcess);
        Ok(exit_code as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(quote("redis"), "redis");
        assert_eq!(
            quote("Docker Desktop Service"),
            "\"Docker Desktop Service\""
        );
        assert_eq!(quote(""), "\"\"");
    }

    #[test]
    fn elevation_detection_works() {
        // Must not panic either way; on dev boxes this is usually false.
        let _ = is_elevated();
    }
}
