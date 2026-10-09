// System primitives for Doctor probes and the context banner: disk, memory,
// CPU, uptime, battery, OS info, startup Run keys, and a PATH tool scan.
// All read-only Win32 calls — nothing shells out, nothing wakes a service.

use std::collections::HashSet;
use std::path::PathBuf;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE,
};
use windows::Win32::System::SystemInformation::{
    ComputerNameDnsHostname, GetComputerNameExW, GetSystemInfo, GetTickCount64,
    GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows::Win32::System::Threading::GetSystemTimes;

/// Free bytes on the volume containing `path` (C:\ by default).
pub fn disk_free_bytes(path: Option<&str>) -> Option<u64> {
    let wide: Vec<u16> = path
        .unwrap_or("C:\\")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut free_available = 0u64;
    let mut total_bytes = 0u64;
    let mut total_free = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free_available),
            Some(&mut total_bytes),
            Some(&mut total_free),
        )
        .ok()?;
    }
    Some(free_available)
}

fn memory_status() -> Option<MEMORYSTATUSEX> {
    let mut stat = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut stat).ok()? };
    Some(stat)
}

/// Total + available physical memory.
pub fn memory_bytes() -> Option<(u64, u64)> {
    let stat = memory_status()?;
    Some((stat.ullTotalPhys, stat.ullAvailPhys))
}

/// Committed swap in use — the honest "is memory tight" signal.
pub fn swap_used_bytes() -> Option<(u64, u64)> {
    let stat = memory_status()?;
    let used = stat.ullTotalPageFile.saturating_sub(stat.ullAvailPageFile);
    Some((used, stat.ullTotalPageFile))
}

/// System uptime in seconds (GetTickCount64 — milliseconds since boot).
pub fn uptime_secs() -> u64 {
    unsafe { GetTickCount64() / 1000 }
}

/// Cumulative system CPU time (kernel+user, idle) — callers sample twice to
/// derive a usage percentage. Units: 100ns FILETIME ticks.
fn system_times() -> Option<(u64, u64, u64)> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetSystemTimes(
            Some(&mut idle as *mut FILETIME),
            Some(&mut kernel as *mut FILETIME),
            Some(&mut user as *mut FILETIME),
        )
        .ok()?;
    }
    let to_u64 = |ft: FILETIME| ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    Some((to_u64(idle), to_u64(kernel), to_u64(user)))
}

/// One CPU-time snapshot for percentage math (see `cpu_percent`).
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

pub fn cpu_times() -> Option<CpuTimes> {
    let (idle, kernel, user) = system_times()?;
    Some(CpuTimes {
        idle,
        total: kernel + user,
    })
}

/// CPU usage % between two snapshots (0–100, one decimal).
pub fn cpu_percent(a: &CpuTimes, b: &CpuTimes) -> Option<f64> {
    let d_total = b.total.checked_sub(a.total)?;
    let d_idle = b.idle.checked_sub(a.idle)?;
    if d_total == 0 {
        return None;
    }
    Some((1.0 - (d_idle as f64 / d_total as f64)) * 100.0)
}

/// Physical core count.
pub fn core_count() -> usize {
    unsafe {
        let mut info = std::mem::zeroed::<SYSTEM_INFO>();
        GetSystemInfo(&mut info);
        info.dwNumberOfProcessors as usize
    }
}

/// (on_ac, battery_pct, charging). None when no battery exists (desktops) —
/// probes render nothing rather than a fake reading.
pub fn battery() -> Option<(bool, u8, bool)> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe {
        GetSystemPowerStatus(&mut status).ok()?;
    }
    // ACLineStatus: 0 = offline, 1 = online, 255 = unknown (usually no battery).
    if status.ACLineStatus == 255 || status.BatteryFlag == 255 {
        return None;
    }
    // BatteryFlag bit 7 (0x80) = charging; BatteryLifePercent 255 = unknown.
    let charging = status.BatteryFlag & 0x80 != 0;
    let pct = if status.BatteryLifePercent == 255 {
        return None;
    } else {
        status.BatteryLifePercent
    };
    Some((status.ACLineStatus == 1, pct, charging))
}

/// Windows build + display name via the registry (VerifyVersionInfoW is
/// manifest-sensitive — skip it; the registry is not). The build is what
/// matters for support: Windows 11 = build ≥ 22000 (both report major 10).
pub fn os_version() -> Option<(u32, String)> {
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build = read_registry_sz(HKEY_LOCAL_MACHINE, KEY, "CurrentBuild")?
        .parse::<u32>()
        .ok()?;
    let display = read_registry_sz(HKEY_LOCAL_MACHINE, KEY, "ProductName")
        .unwrap_or_else(|| "Windows".into());
    Some((build, format!("{display} (build {build})")))
}

/// Active power scheme GUID (HKLM …\Power\User\PowerSchemes). Locale-stable —
/// powercfg's display names are not.
pub fn power_scheme() -> Option<String> {
    read_registry_sz(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes",
        "ActivePowerScheme",
    )
}

/// Battery saver set to engage on battery (1 = on). None on machines without
/// the feature (desktops/VMs) — probes render nothing rather than guess.
pub fn battery_saver_on_battery() -> Option<u32> {
    read_registry_dword(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\GlobalSettings\BatterySaver",
        "BatterySaverOnBattery",
    )
}

/// Windows finished installing updates but needs a reboot to complete them —
/// the two keys Windows Update itself writes. Either present = pending.
pub fn reboot_pending() -> bool {
    const WU: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired";
    const CBS: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending";
    key_exists(HKEY_LOCAL_MACHINE, WU) || key_exists(HKEY_LOCAL_MACHINE, CBS)
}

fn key_exists(root: HKEY, subkey: &str) -> bool {
    let Some(hkey) = open_read(root, subkey) else {
        return false;
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    true
}

/// CPU model string from the registry (HKLM …\CentralProcessor\0). Real
/// hardware and VMs alike ship this value.
pub fn cpu_model() -> Option<String> {
    read_registry_sz(
        HKEY_LOCAL_MACHINE,
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "ProcessorNameString",
    )
}

/// Last recorded successful Windows Update check (ISO-8601-ish REG_SZ written
/// by the Update UX). Read HKLM then HKCU; None when this OS writes no such
/// value (fresh installs and some SKUs legitimately don't) — the probe then
/// reports "couldn't determine", never a fake.
pub fn wu_last_success() -> Option<String> {
    const UX: &str = r"SOFTWARE\Microsoft\WindowsUpdate\UX\Settings";
    let value = read_registry_sz(HKEY_LOCAL_MACHINE, UX, "LastSuccessTime")
        .or_else(|| read_registry_sz(HKEY_CURRENT_USER, UX, "LastSuccessTime"));
    match value {
        Some(v) if v.trim().is_empty() => None,
        other => other,
    }
}

/// Computer name (for the context banner).
pub fn computer_name() -> Option<String> {
    let mut buf = [0u16; 256];
    let mut size = buf.len() as u32;
    unsafe {
        GetComputerNameExW(
            ComputerNameDnsHostname,
            Some(PWSTR(buf.as_mut_ptr())),
            &mut size,
        )
        .ok()?;
    }
    Some(String::from_utf16_lossy(&buf[..size as usize]))
}

// --- registry helpers ---------------------------------------------------------

fn open_read(root: HKEY, subkey: &str) -> Option<HKEY> {
    let sub: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey = Default::default();
    unsafe {
        // RegOpenKeyExW returns WIN32_ERROR; 0 = ERROR_SUCCESS.
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut hkey).is_err() {
            return None;
        }
    }
    Some(hkey)
}

fn read_registry_sz(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let val: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let hkey = open_read(root, subkey)?;
    unsafe {
        let mut buf = [0u16; 512];
        let mut buf_len = (buf.len() * 2) as u32;
        let mut ty = REG_VALUE_TYPE::default();
        let res = RegQueryValueExW(
            hkey,
            PCWSTR(val.as_ptr()),
            None,
            Some(&mut ty as *mut REG_VALUE_TYPE),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut buf_len),
        );
        let _ = RegCloseKey(hkey);
        if res.is_err() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn read_registry_dword(root: HKEY, subkey: &str, value: &str) -> Option<u32> {
    let val: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let hkey = open_read(root, subkey)?;
    unsafe {
        let mut out = 0u32;
        let mut len = 4u32;
        let mut ty = REG_VALUE_TYPE::default();
        let res = RegQueryValueExW(
            hkey,
            PCWSTR(val.as_ptr()),
            None,
            Some(&mut ty as *mut REG_VALUE_TYPE),
            Some(&mut out as *mut u32 as *mut u8),
            Some(&mut len),
        );
        let _ = RegCloseKey(hkey);
        if res.is_err() {
            return None;
        }
        Some(out)
    }
}

/// All value names under a Run key — the login-startup entries. The key path
/// is identical for HKCU (per-user) and HKLM (machine-wide); only the root
/// hive differs.
pub fn startup_run_entries(machine: bool) -> Vec<String> {
    let root = if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    let sub = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    let Some(hkey) = open_read(root, sub) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    unsafe {
        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 256];
            let mut name_len = name_buf.len() as u32;
            let res = RegEnumValueW(
                hkey,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            );
            if res.is_err() {
                break;
            }
            names.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
            index += 1;
            if index > 512 {
                break; // runaway guard
            }
        }
        let _ = RegCloseKey(hkey);
    }
    names
}

/// Dev tools found on PATH (node, npm, git, python, docker, …). Returns
/// (tool name, full path) pairs — read-only, used by the Doctor context.
pub fn path_tools() -> Vec<(String, PathBuf)> {
    const TOOLS: &[&str] = &[
        "node",
        "npm",
        "npx",
        "pnpm",
        "yarn",
        "git",
        "python",
        "python3",
        "py",
        "pip",
        "ruby",
        "go",
        "rustc",
        "cargo",
        "java",
        "docker",
        "docker-compose",
        "psql",
        "mysql",
        "redis-cli",
        "curl",
        "wget",
        "code",
        "pwsh",
    ];
    let Some(path) = std::env::var("PATH").ok() else {
        return Vec::new();
    };
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
        .split(';')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let dirs: Vec<&str> = path.split(';').collect();
    for tool in TOOLS {
        if seen.contains(tool) {
            continue;
        }
        'outer: for dir in &dirs {
            if dir.trim().is_empty() {
                continue;
            }
            for ext in &exts {
                let candidate = PathBuf::from(dir).join(format!("{tool}{ext}"));
                if candidate.is_file() {
                    found.push((tool.to_string(), candidate));
                    seen.insert(*tool);
                    break 'outer;
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_machine_reads() {
        // All read-only, all against this machine.
        let free = disk_free_bytes(None).expect("disk readable");
        assert!(free > 0, "C: has free space");
        let (total, avail) = memory_bytes().expect("memory readable");
        assert!(total > 0 && avail <= total);
        let up = uptime_secs();
        assert!(up > 0, "machine has been up");
        let cores = core_count();
        assert!(cores >= 1);
        let name = computer_name().expect("computer name readable");
        assert!(!name.is_empty());
        println!(
            "free={} total={} up={}s cores={} name={}",
            free, total, up, cores, name
        );
    }

    #[test]
    fn cpu_percent_between_snapshots() {
        let a = cpu_times().expect("times");
        std::thread::sleep(std::time::Duration::from_millis(120));
        let b = cpu_times().expect("times");
        let pct = cpu_percent(&a, &b).expect("delta");
        assert!((0.0..=100.0).contains(&pct), "cpu % in range: {pct}");
    }

    #[test]
    fn os_version_parses() {
        let (build, label) = os_version().expect("registry readable");
        assert!(build >= 10240, "Windows 10/11 build: {label}");
        assert!(label.contains("Windows"), "product name: {label}");
    }

    #[test]
    fn power_settings_read() {
        let scheme = power_scheme();
        println!(
            "active power scheme: {scheme:?} saver-on-battery: {:?}",
            battery_saver_on_battery()
        );
        // A scheme GUID is either a known 36-char GUID or absent on exotic
        // SKUs — never panic either way.
        if let Some(s) = scheme {
            assert_eq!(s.len(), 36, "GUID-shaped: {s}");
        }
    }

    #[test]
    fn reboot_flag_read() {
        // Presence-only check against the real machine — no assertion on the
        // value, just that the two keys open cleanly.
        let pending = reboot_pending();
        println!("reboot pending: {pending}");
    }

    #[test]
    fn run_keys_enumerate() {
        let user = startup_run_entries(false);
        println!("user Run keys: {user:?}");
        // Not asserted non-empty — a clean VM can have none; the call must not
        // panic either way.
    }

    #[test]
    fn path_tools_scan() {
        let tools = path_tools();
        println!(
            "PATH tools: {:?}",
            tools.iter().map(|(t, _)| t).collect::<Vec<_>>()
        );
        // git or node should exist on a dev machine; don't fail CI images.
    }

    #[test]
    fn cpu_and_wu_read() {
        let model = cpu_model().expect("a CPU model string is present on real machines");
        assert!(!model.is_empty());
        println!("CPU: {model}");
        // wu_last_success is legitimately absent on some installs — never assert.
        println!("WU last success: {:?}", wu_last_success());
    }
}
