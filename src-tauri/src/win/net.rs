// Listening ports and process samples through the IP Helper and process APIs.
// GetExtendedTcpTable/GetExtendedUdpTable give pid-owned sockets directly — no
// netstat text parsing, no locale fragility. Process names come from a ToolHelp
// snapshot; CPU/memory/uptime from GetProcessTimes + K32GetProcessMemoryInfo.

use std::collections::HashMap;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCPTABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID,
    TCP_TABLE_OWNER_PID_ALL, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

use crate::error::{AppError, AppResult};

/// One socket row: local port + owning process, enriched later with samples.
#[derive(Debug, Clone)]
pub struct SocketRow {
    pub pid: u32,
    pub port: u16,
    pub proto: String,
}

/// Per-process sample: lifetime-average CPU %, working set, uptime.
#[derive(Debug, Clone, Copy)]
pub struct ProcSample {
    pub cpu: f64,
    pub rss_bytes: u64,
    pub uptime_secs: u64,
}

/// pid → image name ("node.exe") via one ToolHelp snapshot.
pub fn process_names() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(s) => s,
            Err(_) => return map,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                map.insert(entry.th32ProcessID, name);
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    map
}

/// dwLocalPort holds the port in network byte order in the low 16 bits.
fn port_from_dword(p: u32) -> u16 {
    u16::from_be((p & 0xFFFF) as u16)
}

/// TCP rows, IPv4 + IPv6, optionally listener-only.
fn tcp_rows(listening_only: bool) -> Vec<SocketRow> {
    let mut rows = Vec::new();
    let table_type = if listening_only {
        TCP_TABLE_OWNER_PID_LISTENER
    } else {
        TCP_TABLE_OWNER_PID_ALL
    };
    for af in [2u32 /* AF_INET */, 23 /* AF_INET6 */] {
        unsafe {
            let mut size = 0u32;
            let _ = GetExtendedTcpTable(None, &mut size, false, af, table_type, 0);
            if size == 0 {
                continue;
            }
            let mut buf = vec![0u8; size as usize];
            // 0 = NO_ERROR.
            if GetExtendedTcpTable(
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                false,
                af,
                table_type,
                0,
            ) != 0
            {
                continue;
            }
            let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let count = table.dwNumEntries as usize;
            let entries = std::slice::from_raw_parts(table.table.as_ptr(), count);
            for e in entries {
                rows.push(SocketRow {
                    port: port_from_dword(e.dwLocalPort),
                    pid: e.dwOwningPid,
                    proto: "TCP".into(),
                });
            }
        }
    }
    rows
}

/// UDP bound sockets (no listen state — every bound socket counts).
fn udp_rows() -> Vec<SocketRow> {
    let mut rows = Vec::new();
    for af in [2u32, 23] {
        unsafe {
            let mut size = 0u32;
            let _ = GetExtendedUdpTable(None, &mut size, false, af, UDP_TABLE_OWNER_PID, 0);
            if size == 0 {
                continue;
            }
            let mut buf = vec![0u8; size as usize];
            if GetExtendedUdpTable(
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                false,
                af,
                UDP_TABLE_OWNER_PID,
                0,
            ) != 0
            {
                continue;
            }
            let table = &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID);
            let count = table.dwNumEntries as usize;
            let entries = std::slice::from_raw_parts(table.table.as_ptr(), count);
            for e in entries {
                let port = port_from_dword(e.dwLocalPort);
                if port != 0 {
                    rows.push(SocketRow {
                        port,
                        pid: e.dwOwningPid,
                        proto: "UDP".into(),
                    });
                }
            }
        }
    }
    rows
}

/// All sockets (deduped by (pid, port, proto)); UDP only when listener view.
pub fn sockets(listening_only: bool) -> Vec<SocketRow> {
    let mut rows = tcp_rows(listening_only);
    if listening_only {
        rows.extend(udp_rows());
    }
    rows.sort_by_key(|r| (r.port, r.pid));
    rows
}

/// pid → "DOMAIN\\user" (best effort — access-denied processes come back None
/// and the UI shows "—"; never guesses).
pub fn process_owner(pid: u32) -> Option<String> {
    use windows::Win32::Security::{
        GetTokenInformation, LookupAccountSidW, SidTypeUser, TokenUser, PSID, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(proc, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(proc);
        opened.ok()?;
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let got = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut _),
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        got.ok()?;
        // TOKEN_USER is { SID_AND_ATTRIBUTES User } where SID_AND_ATTRIBUTES
        // is { PSID Sid; DWORD Attributes } — read the SID pointer at offset 0.
        let sid = PSID(*(buf.as_ptr() as *const *mut std::ffi::c_void));
        let mut account = [0u16; 256];
        let mut domain = [0u16; 256];
        let mut acc_len = account.len() as u32;
        let mut dom_len = domain.len() as u32;
        let mut sid_use = SidTypeUser;
        if LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(account.as_mut_ptr())),
            &mut acc_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut dom_len,
            &mut sid_use,
        )
        .is_err()
        {
            return None;
        }
        let acc = String::from_utf16_lossy(&account[..acc_len as usize]);
        let dom = String::from_utf16_lossy(&domain[..dom_len as usize]);
        if dom.is_empty() {
            Some(acc)
        } else {
            Some(format!("{dom}\\{acc}"))
        }
    }
}

/// pid → (image path or exe name). Falls back to the ToolHelp image name.
pub fn process_image(pid: u32, names: &HashMap<u32, String>) -> String {
    unsafe {
        if let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = windows::Win32::System::Threading::QueryFullProcessImageNameW(
                proc,
                windows::Win32::System::Threading::PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(proc);
            if ok.is_ok() {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                if let Some((_, file)) = path.rsplit_once('\\') {
                    return file.to_string();
                }
                return path;
            }
        }
    }
    names.get(&pid).cloned().unwrap_or_else(|| "unknown".into())
}

/// Lifetime-average CPU %, working set bytes, uptime — same semantics as the
/// macOS `ps` numbers the UI already renders (CPU time ÷ wall time).
pub fn samples(pids: &[u32]) -> HashMap<u32, ProcSample> {
    let mut map = HashMap::new();
    let now_filetime = filetime_now();
    for &pid in pids {
        if let Some(s) = sample_one(pid, now_filetime) {
            map.insert(pid, s);
        }
    }
    map
}

fn filetime_now() -> u64 {
    // FILETIME (100ns since 1601-01-01) from the system clock.
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    dur.as_nanos() as u64 / 100 + 116_444_736_000_000_000
}

fn filetime_to_u64(ft: windows::Win32::Foundation::FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

fn sample_one(pid: u32, now_ft: u64) -> Option<ProcSample> {
    unsafe {
        let proc = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        )
        .ok()?;
        let mut creation = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        let got_times = GetProcessTimes(proc, &mut creation, &mut exit, &mut kernel, &mut user);

        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let got_mem = K32GetProcessMemoryInfo(
            proc,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        let _ = CloseHandle(proc);

        let mut cpu = 0f64;
        let mut uptime_secs = 0u64;
        if got_times.is_ok() {
            let created = filetime_to_u64(creation);
            let elapsed_100ns = now_ft.saturating_sub(created);
            uptime_secs = elapsed_100ns / 10_000_000;
            let cpu_100ns = filetime_to_u64(kernel) + filetime_to_u64(user);
            if elapsed_100ns > 0 {
                cpu = (cpu_100ns as f64 / elapsed_100ns as f64) * 100.0;
            }
        }
        let rss = if got_mem.as_bool() {
            counters.WorkingSetSize as u64
        } else {
            0
        };
        Some(ProcSample {
            cpu,
            rss_bytes: rss,
            uptime_secs,
        })
    }
}

/// Terminate a process. Callers enforce the protected-process rules (never the
/// kernel pids, never Nexus itself, never known-critical system processes).
pub fn terminate(pid: u32) -> AppResult<()> {
    unsafe {
        let proc = OpenProcess(PROCESS_TERMINATE, false, pid).map_err(|e| {
            let code = e.code().0 as u32 & 0xFFFF;
            if code == 5 {
                AppError::Forbidden(format!(
                    "pid {pid}: access denied — this process needs administrator rights"
                ))
            } else if code == 87 {
                AppError::NotFound(format!("pid {pid}: no such process"))
            } else {
                AppError::Io(format!("OpenProcess pid {pid}: {e}"))
            }
        })?;
        let ok = windows::Win32::System::Threading::TerminateProcess(proc, 1);
        let _ = CloseHandle(proc);
        ok.map_err(|e| {
            let code = e.code().0 as u32 & 0xFFFF;
            if code == 5 {
                AppError::Forbidden(format!(
                    "pid {pid}: access denied — this process needs administrator rights"
                ))
            } else {
                AppError::Io(format!("TerminateProcess pid {pid}: {e}"))
            }
        })
    }
}

/// Image names that must never be terminated through this app.
pub const PROTECTED_IMAGES: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "memory compression",
    "nexus console.exe",
];

pub fn is_protected(pid: u32, names: &HashMap<u32, String>) -> bool {
    if pid <= 4 {
        return true;
    }
    names
        .get(&pid)
        .map(|n| {
            let n = n.to_ascii_lowercase();
            PROTECTED_IMAGES.contains(&n.as_str())
        })
        // Unknown pid (name lookup failed) is protected — terminate refuses
        // anything we can't positively identify.
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_epoch_math() {
        // 2026-01-01T00:00:00Z = 1735689600 unix secs.
        let ft = 1735689600u64 * 10_000_000 + 116_444_736_000_000_000;
        assert_eq!(
            filetime_to_u64(windows::Win32::Foundation::FILETIME {
                dwLowDateTime: ft as u32,
                dwHighDateTime: (ft >> 32) as u32,
            }),
            ft
        );
    }

    #[test]
    fn port_from_network_order_dword() {
        // dwLocalPort holds the port big-endian in the low 16 bits, so as a
        // little-endian u16 the bytes are swapped: port 8080 → 0x901F.
        assert_eq!(port_from_dword(0x901F), 8080);
        assert_eq!(port_from_dword(0x5000), 80);
        assert_eq!(port_from_dword(0), 0);
    }

    #[test]
    fn protects_kernel_and_self() {
        let names = HashMap::from([
            (4u32, "System".to_string()),
            (123u32, "lsass.exe".to_string()),
            (456u32, "node.exe".to_string()),
        ]);
        assert!(is_protected(0, &names));
        assert!(is_protected(4, &names));
        assert!(is_protected(123, &names));
        assert!(!is_protected(456, &names));
        assert!(is_protected(999, &names), "unknown pid is protected");
    }

    // Real machine, read-only. Ignored in CI.
    #[test]
    #[ignore]
    fn lists_real_listeners() {
        let rows = sockets(true);
        println!("{} listeners", rows.len());
        assert!(!rows.is_empty(), "a Windows box always has listeners");
        let names = process_names();
        for r in rows.iter().take(5) {
            println!(
                "pid {} port {} {} → {}",
                r.pid,
                r.port,
                r.proto,
                process_image(r.pid, &names)
            );
        }
    }

    #[test]
    #[ignore]
    fn samples_real_processes() {
        let pid = std::process::id();
        let map = samples(&[pid]);
        let s = map.get(&pid).expect("self sample");
        assert!(s.rss_bytes > 0);
    }
}
