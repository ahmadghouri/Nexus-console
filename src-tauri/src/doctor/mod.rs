// Nexus Doctor — probe engine (docs/doctor/PLAN.md Phase 0).
// A probe is read-only and must never wake a daemon (the watchman lesson,
// SESSION-CONTEXT §2). Mutations only ever happen via explicit fix commands.

pub mod browser;
pub mod claude;
pub mod deepscan;
pub mod ollama;
pub mod paths;
pub mod probes;
pub mod scheduler;
pub mod system;
pub mod vscode;
pub mod windows;

use serde::{Deserialize, Serialize};

/// Scan scopes — mirror the Doctor page's scope chips 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    System,
    Claude,
    Vscode,
    Browser,
    Disk,
    Startup,
}

/// Declared in report order — `as u8` is the sort key (red first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Red,
    Yellow,
    Green,
}

/// Honest tag: disk cleanup does NOT make a machine faster. Every finding
/// carries what it actually affects; `Info` is for findings that are neither
/// (backup risk, battery wear).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    Speed,
    Storage,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::MacOs
        }
    }
}

/// One line in the report: severity + one-line summary + expandable explain.
/// `guide` = manual GUI steps; `fix` = id of an automated fix command the UI
/// maps to a button (each fix is its own arg-less command — the frontend never
/// supplies paths).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub probe_id: &'static str,
    pub scope: Scope,
    pub severity: Severity,
    pub tag: Tag,
    pub summary: String,
    pub explain: String,
    pub guide: Option<Vec<String>>,
    /// Fix ids are usually static ("vscode_vacuum") but can carry a validated
    /// parameter ("ollama_rm:<model>") — the backend re-validates against the
    /// real installed set at fix time, never trusting the string itself.
    pub fix: Option<String>,
}

pub struct Probe {
    pub id: &'static str,
    pub scope: Scope,
    pub platforms: &'static [Platform],
    /// Runtime capability check — a feature the device doesn't have simply
    /// never renders (works both directions, Win↔Mac).
    pub available: fn() -> bool,
    /// Read-only. Must not wake daemons or block on anything interactive.
    pub run: fn() -> Vec<Finding>,
}

pub const REGISTRY: &[Probe] = &[
    probes::DISK_FREE,
    probes::LOAD_VS_CORES,
    system::LOW_POWER_MODE,
    system::MEMORY_TRUTH,
    system::BACKUP_RISK,
    system::STARTUP_ITEMS,
    vscode::ORPHANS,
    vscode::DB_BLOAT,
    vscode::EXTENSION_AUDIT,
    ollama::OLLAMA_MODELS,
    browser::PROCESSES,
    browser::AUDIO_BLOCKER,
    browser::STORAGE,
    browser::EXTENSIONS,
    // The Windows probes live in a `#![cfg(windows)]` module — gated here too,
    // so a macOS build never sees the registry entries (mac = 14 probes,
    // Windows = the same 14 + the 12 below).
    #[cfg(windows)]
    windows::BATTERY_SAVER,
    #[cfg(windows)]
    windows::MEMORY_TRUTH,
    #[cfg(windows)]
    windows::STARTUP_ITEMS,
    #[cfg(windows)]
    windows::LOAD_VS_CORES,
    #[cfg(windows)]
    windows::OS_SUPPORT,
    #[cfg(windows)]
    windows::REBOOT_PENDING,
    #[cfg(windows)]
    windows::DEV_TOOLS,
    #[cfg(windows)]
    windows::BATTERY_HEALTH,
    #[cfg(windows)]
    windows::UPTIME,
    #[cfg(windows)]
    windows::CPU_MODEL,
    #[cfg(windows)]
    windows::DISK_HEALTH,
    #[cfg(windows)]
    windows::WU_STATUS,
];

/// A probe that can't read its source reports that honestly instead of
/// silently vanishing from the report.
pub(crate) fn probe_failed(probe: &Probe, tag: Tag, what: &str) -> Finding {
    Finding {
        probe_id: probe.id,
        scope: probe.scope,
        severity: Severity::Yellow,
        tag,
        summary: format!("Couldn't {what}"),
        explain: "The read-only probe failed on this machine — the item was not checked."
            .to_string(),
        guide: None,
        fix: None,
    }
}

/// Probes that will actually run: platform ∩ requested scopes ∩ available.
pub fn probes_for(scopes: &[Scope]) -> Vec<&'static Probe> {
    REGISTRY
        .iter()
        .filter(|p| p.platforms.contains(&Platform::current()))
        .filter(|p| scopes.contains(&p.scope))
        .filter(|p| (p.available)())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // vscode/ollama/browser probes are availability-gated; counts below assume
    // all exist on this machine. Ignored in CI (runner has none installed).
    // macOS counts — the Windows registry has its own probes, tested there.
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn registry_filters_by_scope() {
        let all = [
            Scope::System,
            Scope::Claude,
            Scope::Vscode,
            Scope::Browser,
            Scope::Disk,
            Scope::Startup,
        ];
        // macOS sees only the 14 cross/mac probes — the 11 Windows entries are
        // cfg(windows)-gated out of this list.
        assert_eq!(REGISTRY.len(), 14);
        assert_eq!(probes_for(&all).len(), 14);
        assert_eq!(probes_for(&[Scope::Browser]).len(), 4);
        assert_eq!(probes_for(&[Scope::Vscode]).len(), 3);
        assert_eq!(probes_for(&[Scope::Disk]).len(), 2);
        assert_eq!(probes_for(&[Scope::Disk])[0].id, "disk_free");
        assert_eq!(probes_for(&[Scope::System]).len(), 4);
        assert_eq!(probes_for(&[Scope::System])[0].id, "load_vs_cores");
        assert_eq!(probes_for(&[Scope::Startup])[0].id, "startup_items");
        assert!(probes_for(&[]).is_empty());
    }

    // Windows counts: the same availability assumptions as the macOS test
    // (vscode/ollama may be absent), so only platform guarantees are asserted.
    #[cfg(windows)]
    #[test]
    fn registry_filters_by_scope() {
        let all = [
            Scope::System,
            Scope::Claude,
            Scope::Vscode,
            Scope::Browser,
            Scope::Disk,
            Scope::Startup,
        ];
        assert_eq!(REGISTRY.len(), 26);
        let ids: Vec<&str> = probes_for(&all).iter().map(|p| p.id).collect();
        for want in [
            "disk_free",
            "battery_saver",
            "memory_truth",
            "startup_items",
            "load_vs_cores",
            "os_support",
            "reboot_pending",
            "dev_tools",
            "battery_health",
            "uptime",
            "cpu_model",
            "disk_health",
            "wu_status",
        ] {
            assert!(ids.contains(&want), "{want} missing from {ids:?}");
        }
        // macOS-only probes must never surface here.
        for absent in ["low_power_mode", "backup_risk"] {
            assert!(!ids.contains(&absent), "{absent} leaked into {ids:?}");
        }
        // Startup scope on Windows = the Run-key twin, never the launchctl one.
        assert_eq!(probes_for(&[Scope::Startup]).len(), 1);
        assert_eq!(probes_for(&[Scope::Startup])[0].id, "startup_items");
        assert!(probes_for(&[]).is_empty());
    }

    #[test]
    fn severity_sorts_red_first() {
        assert!((Severity::Red as u8) < (Severity::Yellow as u8));
        assert!((Severity::Yellow as u8) < (Severity::Green as u8));
    }
}
