// Service Control Manager (SCM) — list/start/stop/configure Windows services
// through the Win32 API directly. No sc.exe for reads; control falls back to an
// elevated helper (`elevate::elevated_wait`) when the token lacks rights (UAC
// prompt), since starting/stopping services normally needs administrator.

use crate::error::{AppError, AppResult};
use windows::core::PCWSTR;
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, EnumServicesStatusExW,
    OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QueryServiceStatusEx, StartServiceW,
    ENUM_SERVICE_STATE, ENUM_SERVICE_TYPE, SC_ENUM_PROCESS_INFO, SC_MANAGER_CONNECT,
    SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START, SERVICE_BOOT_START,
    SERVICE_CHANGE_CONFIG, SERVICE_CONTINUE_PENDING, SERVICE_CONTROL_STOP, SERVICE_DEMAND_START,
    SERVICE_DISABLED, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_PAUSED, SERVICE_PAUSE_PENDING,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START,
    SERVICE_START_PENDING, SERVICE_START_TYPE, SERVICE_STATE_ALL, SERVICE_STATUS,
    SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_PROCESS, SERVICE_STOP, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SERVICE_SYSTEM_START, SERVICE_WIN32,
};

/// One Windows service as the rest of the app sees it (mapped onto ServiceDto).
#[derive(Debug, Clone)]
pub struct ScmService {
    pub name: String,
    pub display_name: String,
    /// "started" | "stopped" | "start_pending" | "stop_pending" | "paused" | "error"
    pub status: String,
    pub pid: Option<u32>,
    /// "automatic" | "manual" | "system" | "disabled"
    pub start_type: String,
}

/// SCM service states → the status strings the frontend reconciler understands.
pub fn map_state(state: SERVICE_STATUS_CURRENT_STATE, win32_exit_code: u32) -> String {
    match state {
        SERVICE_RUNNING => "started".into(),
        SERVICE_START_PENDING | SERVICE_CONTINUE_PENDING => "start_pending".into(),
        SERVICE_STOP_PENDING | SERVICE_PAUSE_PENDING => "stop_pending".into(),
        SERVICE_PAUSED => "paused".into(),
        SERVICE_STOPPED => {
            if win32_exit_code != 0 {
                "error".into()
            } else {
                "stopped".into()
            }
        }
        _ => "stopped".into(),
    }
}

pub fn map_start_type(t: SERVICE_START_TYPE) -> String {
    match t {
        SERVICE_AUTO_START => "automatic".into(),
        SERVICE_DEMAND_START => "manual".into(),
        SERVICE_BOOT_START | SERVICE_SYSTEM_START => "system".into(),
        SERVICE_DISABLED => "disabled".into(),
        _ => "manual".into(),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Windows error codes are HRESULTs; the meaningful bits are the low 16.
fn err_code(e: &windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

fn os_error_message(prefix: &str, code: u32) -> AppError {
    match code {
        5 => AppError::Forbidden(format!(
            "{prefix}: access denied — this needs administrator rights"
        )),
        1060 => AppError::NotFound(format!("{prefix}: service is not installed")),
        1056 | 1061 => AppError::Shell(format!("{prefix}: the service is in an unexpected state")),
        1058 => AppError::Shell(format!(
            "{prefix}: the service is disabled — enable it in Services first"
        )),
        _ => AppError::Shell(format!("{prefix}: Windows error {code}")),
    }
}

fn open_manager(access: u32) -> AppResult<windows::Win32::System::Services::SC_HANDLE> {
    unsafe {
        OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access)
            .map_err(|e| AppError::Shell(format!("OpenSCManager: {e}")))
    }
}

/// List every WIN32 service (own-process) with state + pid. Startup type is
/// queried per service — a registry-speed call, fine inside the list cache.
pub fn list_services() -> AppResult<Vec<ScmService>> {
    let scm = open_manager(SC_MANAGER_ENUMERATE_SERVICE | SC_MANAGER_CONNECT)?;
    let mut out = Vec::new();
    let mut resume: u32 = 0;
    loop {
        let mut buf = vec![0u8; 64 * 1024];
        let mut bytes_needed = 0u32;
        let mut count = 0u32;
        let res = unsafe {
            EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                ENUM_SERVICE_STATE(SERVICE_STATE_ALL.0),
                Some(&mut buf),
                &mut bytes_needed,
                &mut count,
                Some(&mut resume),
                PCWSTR::null(),
            )
        };
        if let Err(e) = res {
            let code = err_code(&e);
            // ERROR_MORE_DATA is normal: resume continues the enumeration.
            if code != 234 {
                unsafe {
                    let _ = CloseServiceHandle(scm);
                }
                return Err(os_error_message("EnumServicesStatusEx", code));
            }
        }
        let mut offset = 0usize;
        for _ in 0..count {
            let e = unsafe { &*(buf.as_ptr().add(offset) as *const EnumEntry) };
            let (name, display) = unsafe { (read_wide(e.name), read_wide(e.display)) };
            let status = e.status;
            offset += std::mem::size_of::<EnumEntry>();

            let start_type = query_start_type(scm, &name).unwrap_or(SERVICE_DEMAND_START);
            out.push(ScmService {
                status: map_state(status.dwCurrentState, status.dwWin32ExitCode),
                pid: (status.dwProcessId != 0).then_some(status.dwProcessId),
                start_type: map_start_type(start_type),
                name,
                display_name: display,
            });
        }
        if resume == 0 {
            break;
        }
    }
    unsafe {
        let _ = CloseServiceHandle(scm);
    }
    out.sort_by_key(|s| s.name.to_lowercase());
    Ok(out)
}

/// One ENUM_SERVICE_STATUS_PROCESSW entry: two pointers into the same buffer
/// plus the status struct. All fields of SERVICE_STATUS_PROCESS are u32-based
/// transparent wrappers, so the C layout matches the Win32 struct exactly.
#[repr(C)]
struct EnumEntry {
    name: *const u16,
    display: *const u16,
    status: SERVICE_STATUS_PROCESS,
}

/// SAFETY: `p` must point at a null-terminated UTF-16 string inside a buffer
/// the caller owns (the EnumServicesStatusEx output buffer).
unsafe fn read_wide(p: *const u16) -> String {
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

fn query_start_type(
    scm: windows::Win32::System::Services::SC_HANDLE,
    name: &str,
) -> Option<SERVICE_START_TYPE> {
    unsafe {
        let svc = OpenServiceW(scm, PCWSTR(wide(name).as_ptr()), SERVICE_QUERY_CONFIG).ok()?;
        let mut needed = 0u32;
        let _ = QueryServiceConfigW(svc, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseServiceHandle(svc);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let res = QueryServiceConfigW(svc, Some(buf.as_mut_ptr() as *mut _), needed, &mut needed);
        let result = if res.is_ok() {
            Some((*(buf.as_ptr() as *const QueryCfg)).start_type)
        } else {
            None
        };
        let _ = CloseServiceHandle(svc);
        result
    }
}

/// QUERY_SERVICE_CONFIGW layout (u32 fields + 4 pointers).
#[repr(C)]
struct QueryCfg {
    service_type: ENUM_SERVICE_TYPE,
    start_type: SERVICE_START_TYPE,
    error_control: SERVICE_ERROR,
    binary_path: *const u16,
    load_order: *const u16,
    tag: u32,
    dependencies: *const u16,
    service_start_name: *const u16,
    display_name: *const u16,
}

/// Start a service. Tries the direct API first (works when already elevated);
/// the caller escalates via `elevate::elevated_wait` on `Forbidden`.
pub fn start(name: &str) -> AppResult<()> {
    let scm = open_manager(SC_MANAGER_CONNECT)?;
    let svc = unsafe {
        OpenServiceW(
            scm,
            PCWSTR(wide(name).as_ptr()),
            SERVICE_START | SERVICE_QUERY_STATUS,
        )
    };
    let svc = match svc {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = CloseServiceHandle(scm);
            }
            return Err(os_error_message(&format!("start {name}"), err_code(&e)));
        }
    };
    let res = unsafe { StartServiceW(svc, None) };
    let result = match res {
        Ok(()) => Ok(()),
        Err(e) => {
            let code = err_code(&e);
            if code == 1056 {
                Ok(()) // already running — idempotent, like brew services start
            } else {
                Err(os_error_message(&format!("start {name}"), code))
            }
        }
    };
    unsafe {
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
    }
    result
}

fn query_status(
    svc: windows::Win32::System::Services::SC_HANDLE,
) -> Option<SERVICE_STATUS_PROCESS> {
    unsafe {
        let mut buf = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0u32;
        let bytes = std::slice::from_raw_parts_mut(
            &mut buf as *mut SERVICE_STATUS_PROCESS as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
        );
        QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, Some(bytes), &mut needed).ok()?;
        Some(buf)
    }
}

/// Stop a service and wait (up to `timeout`) for it to fully reach STOPPED, so
/// a following start doesn't race a half-stopped process.
pub fn stop(name: &str, timeout: std::time::Duration) -> AppResult<()> {
    let scm = open_manager(SC_MANAGER_CONNECT)?;
    let svc = unsafe {
        OpenServiceW(
            scm,
            PCWSTR(wide(name).as_ptr()),
            SERVICE_STOP | SERVICE_QUERY_STATUS,
        )
    };
    let svc = match svc {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = CloseServiceHandle(scm);
            }
            return Err(os_error_message(&format!("stop {name}"), err_code(&e)));
        }
    };

    let already_stopped = query_status(svc)
        .map(|s| s.dwCurrentState == SERVICE_STOPPED)
        .unwrap_or(true);

    if !already_stopped {
        let mut ctrl = SERVICE_STATUS::default();
        let res = unsafe { ControlService(svc, SERVICE_CONTROL_STOP, &mut ctrl) };
        if let Err(e) = res {
            let code = err_code(&e);
            if code != 1062 {
                // 1062 = not started → already stopped
                unsafe {
                    let _ = CloseServiceHandle(svc);
                    let _ = CloseServiceHandle(scm);
                }
                return Err(os_error_message(&format!("stop {name}"), code));
            }
        }
        let deadline = std::time::Instant::now() + timeout;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            match query_status(svc) {
                None => break, // service handle went away — treat as stopped
                Some(s) if s.dwCurrentState == SERVICE_STOPPED => break,
                Some(_) if std::time::Instant::now() > deadline => {
                    unsafe {
                        let _ = CloseServiceHandle(svc);
                        let _ = CloseServiceHandle(scm);
                    }
                    return Err(AppError::Shell(format!(
                        "stop {name}: still stopping after {}s",
                        timeout.as_secs()
                    )));
                }
                Some(_) => {}
            }
        }
    }
    unsafe {
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
    }
    Ok(())
}

/// Poll until the service reaches STOPPED. `sc.exe stop` (the elevated path)
/// returns as soon as the control code is accepted, so the caller waits here
/// before a following start — same guarantee `stop()` gives on the direct path.
pub fn wait_stopped(name: &str, timeout: std::time::Duration) -> AppResult<()> {
    let scm = open_manager(SC_MANAGER_CONNECT)?;
    let svc = unsafe { OpenServiceW(scm, PCWSTR(wide(name).as_ptr()), SERVICE_QUERY_STATUS) };
    let svc = match svc {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = CloseServiceHandle(scm);
            }
            return Err(os_error_message(&format!("stop {name}"), err_code(&e)));
        }
    };
    let deadline = std::time::Instant::now() + timeout;
    let mut result = Ok(());
    loop {
        match query_status(svc) {
            None => break, // handle went away — nothing left to wait for
            Some(s) if s.dwCurrentState == SERVICE_STOPPED => break,
            Some(_) if std::time::Instant::now() > deadline => {
                result = Err(AppError::Shell(format!(
                    "stop {name}: still stopping after {}s",
                    timeout.as_secs()
                )));
                break;
            }
            Some(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    unsafe {
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
    }
    result
}

/// Change the startup type: "automatic" (linked/persistent) vs "demand"
/// (manual/ephemeral). This is the Windows meaning of the app's "linked"
/// concept — a service that starts with Windows survives a reboot.
pub fn set_start_type(name: &str, auto: bool) -> AppResult<()> {
    let scm = open_manager(SC_MANAGER_CONNECT)?;
    let svc = unsafe {
        OpenServiceW(
            scm,
            PCWSTR(wide(name).as_ptr()),
            SERVICE_CHANGE_CONFIG | SERVICE_QUERY_STATUS,
        )
    };
    let svc = match svc {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = CloseServiceHandle(scm);
            }
            return Err(os_error_message(&format!("configure {name}"), err_code(&e)));
        }
    };
    let target = if auto {
        SERVICE_AUTO_START
    } else {
        SERVICE_DEMAND_START
    };
    let res = unsafe {
        ChangeServiceConfigW(
            svc,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            target,
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    };
    let result = match res {
        Ok(()) => Ok(()),
        Err(e) => Err(os_error_message(&format!("configure {name}"), err_code(&e))),
    };
    unsafe {
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
    }
    result
}

/// Current startup type of a service, or None when unreadable.
pub fn current_start_type(name: &str) -> Option<SERVICE_START_TYPE> {
    let scm = open_manager(SC_MANAGER_CONNECT).ok()?;
    let out = query_start_type(scm, name);
    unsafe {
        let _ = CloseServiceHandle(scm);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_mapping() {
        assert_eq!(map_state(SERVICE_RUNNING, 0), "started");
        assert_eq!(map_state(SERVICE_START_PENDING, 0), "start_pending");
        assert_eq!(map_state(SERVICE_STOP_PENDING, 0), "stop_pending");
        assert_eq!(map_state(SERVICE_PAUSED, 0), "paused");
        assert_eq!(map_state(SERVICE_STOPPED, 0), "stopped");
        assert_eq!(map_state(SERVICE_STOPPED, 1067), "error");
    }

    #[test]
    fn start_type_mapping() {
        assert_eq!(map_start_type(SERVICE_AUTO_START), "automatic");
        assert_eq!(map_start_type(SERVICE_DEMAND_START), "manual");
        assert_eq!(map_start_type(SERVICE_BOOT_START), "system");
        assert_eq!(map_start_type(SERVICE_DISABLED), "disabled");
    }

    #[test]
    fn error_codes_map_to_kinds() {
        assert!(matches!(os_error_message("x", 5), AppError::Forbidden(_)));
        assert!(matches!(os_error_message("x", 1060), AppError::NotFound(_)));
        assert!(matches!(os_error_message("x", 1058), AppError::Shell(_)));
    }

    // Real machine, read-only. Ignored in CI.
    #[test]
    #[ignore]
    fn lists_real_services() {
        let services = list_services().expect("SCM readable");
        assert!(!services.is_empty(), "a Windows box always has services");
        for s in services.iter().take(8) {
            println!(
                "{} ({}) — {} start={}",
                s.name, s.display_name, s.status, s.start_type
            );
        }
        // Spooler is present on every normal Windows install.
        assert!(services
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case("spooler")));
    }

    #[test]
    #[ignore]
    fn reads_start_type_of_spooler() {
        let t = current_start_type("Spooler").expect("spooler config");
        assert_eq!(
            map_start_type(t),
            "automatic",
            "Print Spooler starts automatically"
        );
    }
}
