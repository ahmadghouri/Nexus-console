// Phase 6 — the real Windows System/Startup probes, all read-only Win32 via
// win::sys (registry, memory, CPU times, power status). Nothing wakes a
// service, nothing blocks on anything interactive — the same discipline as
// the macOS probes. The one exception is disk_health, which shells out to a
// PowerShell CIM *read* (SMART wear + nominal disk status); it starts no
// daemons and is gated to `dev` tier when the Storage CIM is blocked.
//
// Probes whose macOS twins share an id (memory_truth, startup_items,
// load_vs_cores) deliberately keep it, so the report labels the same check on
// either platform; the platform filter in probes_for guarantees only one twin
// is ever registered per machine. Probes whose signal reads fine render green
// or nothing (invisible-when-fine, like macOS low_power_mode) — a report row
// must earn its place.

// The whole module only exists on Windows (win::sys is cfg-gated) — macOS
// builds see an empty module and the cfg(windows) REGISTRY entries vanish.
#![cfg(windows)]

use super::{probe_failed, Finding, Platform, Probe, Scope, Severity, Tag};
use crate::win::sys;
use std::path::PathBuf;

const WINDOWS: &[Platform] = &[Platform::Windows];

fn always() -> bool {
    true
}

pub const BATTERY_SAVER: Probe = Probe {
    id: "battery_saver",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_battery_saver,
};

pub const MEMORY_TRUTH: Probe = Probe {
    id: "memory_truth",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_memory_truth,
};

pub const STARTUP_ITEMS: Probe = Probe {
    id: "startup_items",
    scope: Scope::Startup,
    platforms: WINDOWS,
    available: always,
    run: run_startup_items,
};

pub const LOAD_VS_CORES: Probe = Probe {
    id: "load_vs_cores",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_load_vs_cores,
};

pub const OS_SUPPORT: Probe = Probe {
    id: "os_support",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_os_support,
};

pub const REBOOT_PENDING: Probe = Probe {
    id: "reboot_pending",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_reboot_pending,
};

pub const DEV_TOOLS: Probe = Probe {
    id: "dev_tools",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_dev_tools,
};

pub const BATTERY_HEALTH: Probe = Probe {
    id: "battery_health",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_battery_health,
};

pub const UPTIME: Probe = Probe {
    id: "uptime",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_uptime,
};

pub const CPU_MODEL: Probe = Probe {
    id: "cpu_model",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_cpu_model,
};

pub const DISK_HEALTH: Probe = Probe {
    id: "disk_health",
    scope: Scope::Disk,
    platforms: WINDOWS,
    available: always,
    run: run_disk_health,
};

pub const WU_STATUS: Probe = Probe {
    id: "wu_status",
    scope: Scope::System,
    platforms: WINDOWS,
    available: always,
    run: run_wu_status,
};

// --- battery_saver ------------------------------------------------------------

/// The schemes Windows ships with. GUIDs are locale-stable — the display names
/// powercfg prints are not, and registry reads need no shell-out anyway.
const SCHEME_POWER_SAVER: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";
const SCHEME_BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const SCHEME_HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const SCHEME_ULTIMATE: &str = "ded574b5-45a0-4f42-8737-46345c09c238";

fn scheme_name(guid: &str) -> Option<&'static str> {
    match guid {
        SCHEME_POWER_SAVER => Some("Power saver"),
        SCHEME_BALANCED => Some("Balanced"),
        SCHEME_HIGH_PERF => Some("High performance"),
        SCHEME_ULTIMATE => Some("Ultimate Performance"),
        _ => None,
    }
}

fn run_battery_saver() -> Vec<Finding> {
    let scheme = sys::power_scheme();
    let saver = sys::battery_saver_on_battery();
    if scheme.is_none() && saver.is_none() {
        return vec![probe_failed(
            &BATTERY_SAVER,
            Tag::Speed,
            "read power settings",
        )];
    }

    // The plan itself throttles, exactly like macOS Low Power Mode.
    if scheme.as_deref() == Some(SCHEME_POWER_SAVER) {
        return vec![Finding {
            probe_id: BATTERY_SAVER.id,
            scope: BATTERY_SAVER.scope,
            severity: Severity::Red,
            tag: Tag::Speed,
            summary: "Power saver plan is active — the CPU is being throttled".into(),
            explain: "The Power saver plan caps CPU speed and background work to stretch the \
                      battery. If the machine feels slow, switch plans first — this was the \
                      measured root cause pattern on macOS (Low Power Mode)."
                .into(),
            guide: Some(vec![
                "Open Settings → System → Power".into(),
                "Set Power mode to Best performance (or Balanced)".into(),
            ]),
            fix: None,
        }];
    }

    // Battery saver engages when unplugged — a throttle that appears later.
    if saver == Some(1) {
        return vec![Finding {
            probe_id: BATTERY_SAVER.id,
            scope: BATTERY_SAVER.scope,
            severity: Severity::Yellow,
            tag: Tag::Speed,
            summary: "Battery saver is ON for battery use — limited while unplugged".into(),
            explain: "Battery saver caps background activity and CPU speed to stretch the \
                      battery. Fine on the go; if the machine feels slow on battery, turn it \
                      off — the setting applies the moment you unplug."
                .into(),
            guide: Some(vec![
                "Open Settings → System → Power → Battery saver".into(),
                "Turn off \"Turn battery saver on automatically\"".into(),
            ]),
            fix: None,
        }];
    }

    // Known non-throttling plan (or no saver key) → nothing is capped.
    let name = scheme.as_deref().and_then(scheme_name);
    let summary = match name {
        Some(n) => format!("Power plan: {n} — CPU not throttled"),
        None => "Power settings aren't throttling the CPU".into(),
    };
    vec![Finding {
        probe_id: BATTERY_SAVER.id,
        scope: BATTERY_SAVER.scope,
        severity: Severity::Green,
        tag: Tag::Speed,
        summary,
        explain: "Windows is on a normal power plan; nothing at the OS level is capping CPU \
                  speed."
            .into(),
        guide: None,
        fix: None,
    }]
}

// --- memory_truth -------------------------------------------------------------

/// Commit charge (RAM + page-file promises) is the honest signal on Windows —
/// free RAM alone is misleading, since Windows deliberately keeps it near zero
/// for caching (same lesson as the macOS probe).
fn classify_memory(free_frac: f64, commit_frac: f64) -> Severity {
    if commit_frac >= 0.95 || free_frac <= 0.05 {
        Severity::Red
    } else if commit_frac >= 0.80 || free_frac <= 0.12 {
        Severity::Yellow
    } else {
        Severity::Green
    }
}

fn run_memory_truth() -> Vec<Finding> {
    let Some((total, avail)) = sys::memory_bytes() else {
        return vec![probe_failed(&MEMORY_TRUTH, Tag::Speed, "read memory stats")];
    };
    let Some((committed, ceiling)) = sys::swap_used_bytes() else {
        return vec![probe_failed(
            &MEMORY_TRUTH,
            Tag::Speed,
            "read commit charge",
        )];
    };
    if total == 0 || ceiling == 0 {
        return vec![probe_failed(&MEMORY_TRUTH, Tag::Speed, "read memory stats")];
    }
    let free_frac = avail as f64 / total as f64;
    let commit_frac = committed as f64 / ceiling as f64;
    let severity = classify_memory(free_frac, commit_frac);
    let label = match severity {
        Severity::Red => "critical",
        Severity::Yellow => "warning",
        Severity::Green => "normal",
    };
    let free_gb = avail as f64 / 1e9;
    let explain = match severity {
        Severity::Green => "\"Free\" near zero is NORMAL — Windows caches aggressively. The \
                            honest signals are free RAM and commit charge (memory promised to \
                            apps vs the RAM+page-file ceiling), and both look fine."
            .to_string(),
        Severity::Yellow => format!(
            "Commit charge is {:.0}% of the ceiling — apps are promised more memory than \
             exists, so Windows is paging to disk. Close the heaviest apps (Task Manager → \
             Details, sort by memory), or let Windows grow the page file.",
            commit_frac * 100.0
        ),
        Severity::Red => format!(
            "Memory is nearly exhausted ({free_gb:.1} GB free, commit {:.0}%) — new \
             allocations will fail or page hard. Task Manager → Performance → Memory shows \
            the breakdown; close the biggest consumers.",
            commit_frac * 100.0
        ),
    };
    vec![Finding {
        probe_id: MEMORY_TRUTH.id,
        scope: MEMORY_TRUTH.scope,
        severity,
        tag: Tag::Speed,
        summary: format!(
            "Memory pressure {label} · {free_gb:.1} GB free · commit {:.0}%",
            commit_frac * 100.0
        ),
        explain,
        guide: None,
        fix: None,
    }]
}

// --- startup_items ------------------------------------------------------------

const MAX_LISTED: usize = 12;

/// The two Startup folders (per-user + machine-wide) — .lnk files that run at
/// sign-in, alongside the Run keys.
fn startup_folder_entries() -> Vec<String> {
    let mut roots = Vec::new();
    for env in ["APPDATA", "ProgramData"] {
        if let Some(base) = std::env::var_os(env) {
            roots.push(
                PathBuf::from(base)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs")
                    .join("Startup"),
            );
        }
    }
    let mut names = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".lnk") {
                name.truncate(name.len() - 4);
            }
            names.push(name);
        }
    }
    names
}

fn run_startup_items() -> Vec<Finding> {
    let mut entries = sys::startup_run_entries(false);
    entries.extend(sys::startup_run_entries(true));
    entries.extend(startup_folder_entries());
    entries.sort();
    entries.dedup();

    if entries.is_empty() {
        return vec![Finding {
            probe_id: STARTUP_ITEMS.id,
            scope: STARTUP_ITEMS.scope,
            severity: Severity::Green,
            tag: Tag::Speed,
            summary: "No programs start with Windows".into(),
            explain: "Nothing extra runs at sign-in.".into(),
            guide: None,
            fix: None,
        }];
    }

    let mut listed = entries
        .iter()
        .take(MAX_LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if entries.len() > MAX_LISTED {
        listed.push_str(&format!(", … +{}", entries.len() - MAX_LISTED));
    }
    let noisy = entries.len() > MAX_LISTED;
    vec![Finding {
        probe_id: STARTUP_ITEMS.id,
        scope: STARTUP_ITEMS.scope,
        severity: if noisy {
            Severity::Yellow
        } else {
            Severity::Green
        },
        tag: Tag::Speed,
        summary: format!(
            "{} program{} start{} with Windows",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" },
            if entries.len() == 1 { "s" } else { "" }
        ),
        explain: format!(
            "Run keys plus the Startup folder — each costs boot time and background RAM: \
             {listed}. Review anything you don't recognize."
        ),
        guide: if noisy {
            Some(vec![
                "Open Task Manager → Startup apps".into(),
                "Disable anything you don't recognize".into(),
            ])
        } else {
            None
        },
        fix: None,
    }]
}

// --- load_vs_cores ------------------------------------------------------------

/// A busy spot-check is Red/Yellow; 5-minute load doesn't exist on Windows,
/// so this is explicitly a ~1.5 s sample — the explain says so honestly.
fn classify_cpu(pct: f64) -> Severity {
    if pct >= 95.0 {
        Severity::Red
    } else if pct >= 80.0 {
        Severity::Yellow
    } else {
        Severity::Green
    }
}

fn run_load_vs_cores() -> Vec<Finding> {
    let Some(a) = sys::cpu_times() else {
        return vec![probe_failed(&LOAD_VS_CORES, Tag::Speed, "read CPU times")];
    };
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let Some(b) = sys::cpu_times() else {
        return vec![probe_failed(&LOAD_VS_CORES, Tag::Speed, "read CPU times")];
    };
    let Some(pct) = sys::cpu_percent(&a, &b) else {
        return vec![probe_failed(&LOAD_VS_CORES, Tag::Speed, "sample CPU usage")];
    };
    let severity = classify_cpu(pct);
    let cores = sys::core_count();
    let explain = match severity {
        Severity::Green => format!(
            "Spot-checked over 1.5 s: the CPU is mostly idle ({pct:.0}% busy) on {cores} \
             cores."
        ),
        Severity::Yellow | Severity::Red => format!(
            "Spot-checked over 1.5 s: the CPU is busy ({pct:.0}% on {cores} cores) right \
             now. Task Manager → Performance → CPU shows what's sustaining it — a boot-time \
             re-index or build burst settles on its own; a permanent top consumer doesn't."
        ),
    };
    vec![Finding {
        probe_id: LOAD_VS_CORES.id,
        scope: LOAD_VS_CORES.scope,
        severity,
        tag: Tag::Speed,
        summary: format!("CPU {pct:.0}% busy · {cores} cores"),
        explain,
        guide: None,
        fix: None,
    }]
}

// --- os_support -----------------------------------------------------------------

/// Windows 11 = build ≥ 22000 (both 10 and 11 report major version 10, so the
/// build is the only reliable split).
const WIN11_FIRST_BUILD: u32 = 22000;

fn run_os_support() -> Vec<Finding> {
    let Some((build, display)) = sys::os_version() else {
        return vec![probe_failed(
            &OS_SUPPORT,
            Tag::Info,
            "read the Windows version",
        )];
    };
    // Supported → nothing renders (invisible-when-fine).
    if build >= WIN11_FIRST_BUILD {
        return vec![];
    }
    vec![Finding {
        probe_id: OS_SUPPORT.id,
        scope: OS_SUPPORT.scope,
        severity: Severity::Red,
        tag: Tag::Info,
        summary: format!("{display} is past end of support"),
        explain: "Microsoft ended security updates for this Windows version (Windows 10 \
                  support ended 14 October 2025), so it receives no further fixes — \
                  Extended Security Updates patch it a little longer, for a price. Windows \
                  11 needs a TPM 2.0-capable CPU; check Update → Windows Update → Check for \
                  updates."
            .into(),
        guide: Some(vec![
            "Open Settings → Windows Update".into(),
            "Check for updates / open the Windows 11 upgrade offer".into(),
        ]),
        fix: None,
    }]
}

// --- reboot_pending --------------------------------------------------------------

fn run_reboot_pending() -> Vec<Finding> {
    // Both keys written only by Windows Update itself — absent = renders nothing.
    if !sys::reboot_pending() {
        return vec![];
    }
    let days = sys::uptime_secs() / 86_400;
    let summary = if days >= 2 {
        format!("Reboot pending · up {days} days")
    } else {
        "Windows updates need a reboot to finish".to_string()
    };
    let explain = if days >= 1 {
        format!(
            "Windows Update finished installing but the changes only take effect after a \
             restart. The machine has been up {days} day{} — restart when convenient.",
            if days == 1 { "" } else { "s" }
        )
    } else {
        "Windows Update finished installing but the changes only take effect after a \
         restart."
            .to_string()
    };
    vec![Finding {
        probe_id: REBOOT_PENDING.id,
        scope: REBOOT_PENDING.scope,
        severity: Severity::Yellow,
        tag: Tag::Info,
        summary,
        explain,
        guide: Some(vec!["Save your work, then restart Windows".into()]),
        fix: None,
    }]
}

// --- dev_tools ---------------------------------------------------------------------

fn run_dev_tools() -> Vec<Finding> {
    let tools = sys::path_tools();
    if tools.is_empty() {
        return vec![Finding {
            probe_id: DEV_TOOLS.id,
            scope: DEV_TOOLS.scope,
            severity: Severity::Yellow,
            tag: Tag::Info,
            summary: "No dev tools found on PATH".into(),
            explain: "node, git, python, … none are reachable from a terminal here. If \
                      they're installed, their bin folders are missing from PATH — \
                      terminals (including this app's) won't see them."
                .into(),
            guide: Some(vec![
                "Settings → System → About → Advanced system settings → Environment Variables"
                    .into(),
                "Check that PATH contains your toolchain's bin folder".into(),
            ]),
            fix: None,
        }];
    }
    let names: Vec<&str> = tools.iter().map(|(t, _)| t.as_str()).collect();
    let mut listed = names.iter().take(8).copied().collect::<Vec<_>>().join(", ");
    if names.len() > 8 {
        listed.push_str(&format!(", … +{}", names.len() - 8));
    }
    vec![Finding {
        probe_id: DEV_TOOLS.id,
        scope: DEV_TOOLS.scope,
        severity: Severity::Green,
        tag: Tag::Info,
        summary: format!("{} dev tools on PATH · {listed}", names.len()),
        explain: "Reachable from terminals launched here, including this app's PTY.".into(),
        guide: None,
        fix: None,
    }]
}

// --- battery_health ----------------------------------------------------------------

fn run_battery_health() -> Vec<Finding> {
    let Some((on_ac, pct, charging)) = sys::battery() else {
        return vec![]; // no battery (desktop/VM) → never renders
    };
    // Plugged in, charge stopped below 80%: wear, or a vendor charge limit.
    if on_ac && !charging && pct < 80 {
        return vec![Finding {
            probe_id: BATTERY_HEALTH.id,
            scope: BATTERY_HEALTH.scope,
            severity: Severity::Yellow,
            tag: Tag::Info,
            summary: format!("Plugged in but not charging · {pct}%"),
            explain: "The charger is connected but charging stopped below 80%. That's \
                      usually battery wear — or a vendor charge-limit feature that \
                      deliberately stops at 80% to protect the battery (many laptop \
                      utilities do this on purpose). Check the manufacturer's utility for \
                      the wear level before assuming a fault."
                .into(),
            guide: Some(vec![
                "Windows Settings → System → Power → battery".into(),
                "Open your laptop vendor's utility (Vantage, MyASUS, …) for battery health".into(),
            ]),
            fix: None,
        }];
    }
    vec![]
}

// --- uptime --------------------------------------------------------------------

fn run_uptime() -> Vec<Finding> {
    let secs = sys::uptime_secs();
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let mins = (secs % 3600) / 60;
    let when = if days >= 2 {
        format!("{days} days, {hours} h")
    } else if days == 1 {
        format!("1 day, {hours} h")
    } else if hours >= 1 {
        format!("{hours} h, {mins} min")
    } else {
        format!("{mins} min")
    };
    vec![Finding {
        probe_id: UPTIME.id,
        scope: UPTIME.scope,
        severity: Severity::Green,
        tag: Tag::Info,
        summary: format!("Up {when}"),
        explain: "Time since boot (GetTickCount64). Useful context for the memory and CPU \
                  rows — a long uptime with a busy commit charge points at a leak or a \
                  forgotten build, not a machine problem."
            .into(),
        guide: None,
        fix: None,
    }]
}

// --- cpu_model -------------------------------------------------------------------

fn run_cpu_model() -> Vec<Finding> {
    let Some(model) = sys::cpu_model() else {
        return vec![probe_failed(&CPU_MODEL, Tag::Info, "read the CPU model")];
    };
    let cores = sys::core_count();
    vec![Finding {
        probe_id: CPU_MODEL.id,
        scope: CPU_MODEL.scope,
        severity: Severity::Green,
        tag: Tag::Info,
        summary: format!("{model} · {cores} logical cores"),
        explain: "Read from the registry — silicon identity plus logical core count, no load \
                  implied. The CPU row above shows actual busy%."
            .into(),
        guide: None,
        fix: None,
    }]
}

// --- wu_status ---------------------------------------------------------------------

/// Windows Update's LastSuccessTime is a REG_SZ like `2026-10-01T00:00:00Z`.
/// Parse just the leading 19 chars — enough for a staleness check.
fn parse_timestamp(s: &str) -> Option<(i64, i64, i64, i64, i64, i64)> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let num = |i: usize, len: usize| s.get(i..i + len)?.parse::<i64>().ok();
    Some((
        num(0, 4)?,
        num(5, 2)?,
        num(8, 2)?,
        num(11, 2)?,
        num(14, 2)?,
        num(17, 2)?,
    ))
}

/// Days between the civil date and the Unix epoch (Hinnant's algorithm) —
/// avoids adding a datetime dependency for one staleness check.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Whole days since the timestamp (0 = today), or None if unparseable.
fn days_ago(s: &str) -> Option<i64> {
    let (y, m, d, h, mi, sec) = parse_timestamp(s)?;
    let then = days_from_civil(y, m, d) * 86400 + h * 3600 + mi * 60 + sec;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(((now - then).max(0)) / 86_400)
}

fn run_wu_status() -> Vec<Finding> {
    let guide: Option<Vec<String>> = Some(vec![
        "Open Settings → Windows Update".into(),
        "Check for updates".into(),
    ]);
    let Some(last) = sys::wu_last_success() else {
        return vec![Finding {
            probe_id: WU_STATUS.id,
            scope: WU_STATUS.scope,
            severity: Severity::Yellow,
            tag: Tag::Info,
            summary: "Windows Update last-check time isn't recorded here".into(),
            explain: "Windows wrote no update check timestamp this install preserves (some \
                      SKUs and freshly-deployed machines populate nothing). That means a \
                      stale-update warning can't be given OR cleared honestly, so check \
                      manually if you expect updates pending."
                .into(),
            guide: guide.clone(),
            fix: None,
        }];
    };
    let Some(ago) = days_ago(&last) else {
        return vec![Finding {
            probe_id: WU_STATUS.id,
            scope: WU_STATUS.scope,
            severity: Severity::Yellow,
            tag: Tag::Info,
            summary: "Windows Update last-check time couldn't be interpreted".into(),
            explain: "A value was present but unreadable ({last:?}) — the OS record is not \
                      in the expected ISO format on this machine. Nothing was assumed."
                .into(),
            guide: guide.clone(),
            fix: None,
        }];
    };
    // Recent check → invisible-when-fine.
    if ago <= 30 {
        return vec![];
    }
    vec![Finding {
        probe_id: WU_STATUS.id,
        scope: WU_STATUS.scope,
        severity: Severity::Yellow,
        tag: Tag::Info,
        summary: format!(
            "Last successful update check {ago} day{} ago",
            if ago == 1 { "" } else { "s" }
        ),
        explain: format!(
            "Windows recorded the last completed update check {ago} day{} ago. Updates that \
             old are the top cause of missing patches — a quick manual check catches up.",
            if ago == 1 { "" } else { "s" }
        ),
        guide: guide.clone(),
        fix: None,
    }]
}

// --- disk_health ------------------------------------------------------------------

/// Tiered PowerShell CIM read. Preferred tier: MSFT_StorageReliabilityCounter
/// (SMART wear/temperature/errors — genuinely supported only when the storage
/// stack exposes it). When that CIM is blocked (common on client machines,
/// as it is on the machine these fixtures were captured on), fall back to the
/// nominal Win32_DiskDrive Status. Read-only — nothing here starts a service.
const DISK_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$r = @(Get-PhysicalDisk -ErrorAction SilentlyContinue | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue | Select-Object DeviceId, FriendlyName, Wear, Temperature, @{n='readErrors';e={[uint64]$_.ReadErrorsTotal}}, @{n='writeErrors';e={[uint64]$_.WriteErrorsTotal}})
if ($r.Count -gt 0) { @{ tier = 'smart'; disks = @($r) } | ConvertTo-Json -Compress -Depth 4; exit 0 }
$d = @(Get-CimInstance -ClassName Win32_DiskDrive -ErrorAction SilentlyContinue | Select-Object @{n='id';e={$_.Index}}, Model, Status)
if ($d.Count -gt 0) { @{ tier = 'dev'; disks = @($d) } | ConvertTo-Json -Compress -Depth 4; exit 0 }
"#;

fn run_disk_script() -> Option<String> {
    let mut cmd = crate::util::command("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        DISK_SCRIPT,
    ]);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

struct DiskHealthRow {
    name: String,
    dev_status: Option<String>,
    wear: Option<u64>,
    read_errors: Option<u64>,
    write_errors: Option<u64>,
}

fn rows_from(value: &serde_json::Value) -> Vec<DiskHealthRow> {
    value
        .get("disks")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .map(|o| {
                    let name = o
                        .get("FriendlyName")
                        .or_else(|| o.get("Model"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    DiskHealthRow {
                        name,
                        dev_status: o.get("Status").and_then(|v| v.as_str()).map(String::from),
                        wear: o.get("Wear").and_then(|v| v.as_u64()),
                        read_errors: o.get("readErrors").and_then(|v| v.as_u64()),
                        write_errors: o.get("writeErrors").and_then(|v| v.as_u64()),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// SMART tier findings — wear high or any read/write errors are honest signals.
fn smart_findings(rows: &[DiskHealthRow]) -> Vec<Finding> {
    let mut out = Vec::new();
    for row in rows {
        let fatigue = row.wear.filter(|w| *w <= 100).unwrap_or(0);
        let errors = row.read_errors.unwrap_or(0) + row.write_errors.unwrap_or(0);
        let label = if row.name.is_empty() {
            "A disk".to_string()
        } else {
            row.name.clone()
        };
        if errors > 0 {
            out.push(Finding {
                probe_id: DISK_HEALTH.id,
                scope: DISK_HEALTH.scope,
                severity: Severity::Red,
                tag: Tag::Storage,
                summary: format!(
                    "{label} reports {errors} I/O error{}",
                    if errors == 1 { "" } else { "s" }
                ),
                explain: "The drive's SMART counters recorded hardware read/write errors, \
                          which precede failures. Back up anything important and check the \
                          vendor's health tool (or preferably replace the disk)."
                    .into(),
                guide: None,
                fix: None,
            });
        } else if fatigue >= 90 {
            out.push(Finding {
                probe_id: DISK_HEALTH.id,
                scope: DISK_HEALTH.scope,
                severity: Severity::Yellow,
                tag: Tag::Storage,
                summary: format!("{label} is heavily worn · {fatigue}%"),
                explain: "The drive reports its rated endurance is nearly spent. It still \
                          works, but this is the signal to replace it on a schedule rather \
                          than on failure."
                    .into(),
                guide: None,
                fix: None,
            });
        }
    }
    if out.is_empty() {
        out.push(Finding {
            probe_id: DISK_HEALTH.id,
            scope: DISK_HEALTH.scope,
            severity: Severity::Green,
            tag: Tag::Storage,
            summary: format!(
                "SMART health OK · {} disk{}",
                rows.len(),
                if rows.len() == 1 { "" } else { "s" }
            ),
            explain: "Wear and I/O-error counters are inside normal range on every drive the \
                      storage stack exposes counters for."
                .into(),
            guide: None,
            fix: None,
        });
    }
    out
}

/// Dev-tier findings — Win32_DiskDrive Status, which works on every desktop.
fn dev_findings(rows: &[DiskHealthRow]) -> Vec<Finding> {
    let mut out = Vec::new();
    for row in rows {
        match row.dev_status.as_deref() {
            Some("OK") => {}
            Some("Pred Fail") => out.push(Finding {
                probe_id: DISK_HEALTH.id,
                scope: DISK_HEALTH.scope,
                severity: Severity::Red,
                tag: Tag::Storage,
                summary: format!("{} is predicting failure", row.name),
                explain: "Windows marks the drive 'Pred Fail' — the SMART predictor has \
                          flagged a likely fault. Back up immediately and plan a replacement."
                    .into(),
                guide: None,
                fix: None,
            }),
            Some(st) if st.eq_ignore_ascii_case("Error") => out.push(Finding {
                probe_id: DISK_HEALTH.id,
                scope: DISK_HEALTH.scope,
                severity: Severity::Red,
                tag: Tag::Storage,
                summary: format!("{} reported an error status", row.name),
                explain: "The disk reports status 'Error'. Have vendor diagnostics confirm \
                          before swap — but treat data on it as at risk."
                    .into(),
                guide: None,
                fix: None,
            }),
            Some(st) => out.push(Finding {
                probe_id: DISK_HEALTH.id,
                scope: DISK_HEALTH.scope,
                severity: Severity::Yellow,
                tag: Tag::Storage,
                summary: format!("{} status is {st}", row.name),
                explain: "The disk reports a status other than OK (Unknown/Degraded). It \
                          mounts and serves, but the exact condition is unclear — back up \
                          and check vendor software."
                    .into(),
                guide: None,
                fix: None,
            }),
            None => {}
        }
    }
    if out.is_empty() {
        out.push(Finding {
            probe_id: DISK_HEALTH.id,
            scope: DISK_HEALTH.scope,
            severity: Severity::Green,
            tag: Tag::Storage,
            summary: format!(
                "{} disk{} report OK",
                rows.len(),
                if rows.len() == 1 { "" } else { "s" }
            ),
            explain: "Win32_DiskDrive Status is OK everywhere. The storage stack didn't \
                      expose richer SMART wear data on this machine, so this nominal check \
                      is what's honestly available."
                .into(),
            guide: None,
            fix: None,
        });
    }
    out
}

fn run_disk_health() -> Vec<Finding> {
    let Some(out) = run_disk_script() else {
        return vec![probe_failed(
            &DISK_HEALTH,
            Tag::Storage,
            "read disk-health status",
        )];
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) else {
        return vec![probe_failed(
            &DISK_HEALTH,
            Tag::Storage,
            "interpret disk-health status",
        )];
    };
    let rows = rows_from(&v);
    if rows.is_empty() {
        return vec![probe_failed(
            &DISK_HEALTH,
            Tag::Storage,
            "find any physical disk",
        )];
    }
    match v.get("tier").and_then(|t| t.as_str()) {
        Some("smart") => smart_findings(&rows),
        Some("dev") => dev_findings(&rows),
        _ => vec![probe_failed(
            &DISK_HEALTH,
            Tag::Storage,
            "interpret disk-health status",
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_memory() {
        assert_eq!(classify_memory(0.50, 0.30), Severity::Green);
        assert_eq!(classify_memory(0.10, 0.30), Severity::Yellow); // low free
        assert_eq!(classify_memory(0.50, 0.85), Severity::Yellow); // commit high
        assert_eq!(classify_memory(0.50, 0.97), Severity::Red); // commit critical
        assert_eq!(classify_memory(0.03, 0.20), Severity::Red); // free critical
    }

    #[test]
    fn classifies_cpu() {
        assert_eq!(classify_cpu(12.0), Severity::Green);
        assert_eq!(classify_cpu(83.0), Severity::Yellow);
        assert_eq!(classify_cpu(97.5), Severity::Red);
    }

    // Real machine, strictly read-only: every probe runs without panicking.
    // The always-on probes return exactly one finding; invisible-when-fine
    // probes may return none (supported OS, no pending reboot, healthy battery,
    // recent Windows Update check).
    #[test]
    fn probes_run_on_real_machine() {
        assert_eq!(run_battery_saver().len(), 1);
        assert_eq!(run_memory_truth().len(), 1);
        assert!(!run_startup_items().is_empty());
        assert_eq!(run_load_vs_cores().len(), 1); // samples for 1.5 s
        assert_eq!(run_dev_tools().len(), 1);
        assert!(run_os_support().len() <= 1);
        assert!(run_reboot_pending().len() <= 1);
        assert!(run_battery_health().len() <= 1);
        assert_eq!(run_uptime().len(), 1);
        assert_eq!(run_cpu_model().len(), 1);
        assert!(run_wu_status().len() <= 1);
        // Disk health can only render when a disk is readable; on a disk-less
        // CI container it honestly reports "couldn't", still one row.
        assert_eq!(run_disk_health().len(), 1);
    }

    #[test]
    fn parses_timestamps_for_staleness() {
        // Mid-2023 snapshot against a mid-2026 "now" — >30 days, so yellow.
        let s = "2023-05-01T00:00:00Z";
        let ago = days_ago(s).expect("parses");
        assert!(ago > 500, "about 3 years: {ago}");
        // Bad shapes never parse (and never panic).
        assert!(days_ago("").is_none());
        assert!(days_ago("garbage").is_none());
        assert!(days_ago("2026-01-01").is_none());
    }

    // The `dev` tier is what this machine actually produced (the reliability
    // CIM is blocked for normal users) — captured as a fixture.
    #[test]
    fn parses_dev_tier_fixture() {
        let out = include_str!("../../fixtures/disk-health-dev.json");
        let v: serde_json::Value = serde_json::from_str(out).expect("fixture parses");
        let rows = rows_from(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "WDC PC SN730 SDBQNTY-256G-1001");
        assert_eq!(rows[0].dev_status.as_deref(), Some("OK"));
        let findings = dev_findings(&rows);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Green);
    }

    #[test]
    fn smart_tier_flags_wear_and_errors() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"tier":"smart","disks":[
                {"DeviceId":0,"FriendlyName":"NVMe A","Wear":95,"readErrors":0,"writeErrors":0},
                {"DeviceId":1,"FriendlyName":"HDD B","Wear":30,"readErrors":12,"writeErrors":0}
            ]}"#,
        )
        .expect("json");
        let rows = rows_from(&v);
        assert_eq!(rows.len(), 2);
        let findings = smart_findings(&rows);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].severity, Severity::Yellow); // heavy wear
        assert_eq!(findings[1].severity, Severity::Red); // I/O errors
        assert!(findings[1].summary.contains("12 I/O errors"));
    }

    #[test]
    fn smart_tier_healthy_is_green() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"tier":"smart","disks":[{"DeviceId":0,"FriendlyName":"Disc X","Wear":4,"readErrors":0,"writeErrors":0}]}"#,
        )
        .expect("json");
        let findings = smart_findings(&rows_from(&v));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Green);
    }
}
