// Windows implementations of the service & package commands. Each fn is the
// target of the `#[cfg(windows)]` dispatch at the top of the matching
// `#[tauri::command]` in commands/{services,packages}.rs — command names,
// argument shapes and DTOs stay identical across platforms, so the IPC
// contract (and the frontend) never forks. Everything blocking runs inside
// spawn_blocking; mutations serialize on WINGET_LOCK (brew's BrewLock twin).

use crate::commands::packages::{OutdatedDto, PackageDto, PackageInfo};
use crate::commands::services::{record_intent, ServiceDto};
use crate::error::{AppError, AppResult};
use crate::session::SessionServices;
use crate::util::validate;
use crate::win::{elevate, net, scm, winget};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Serializes winget mutations — concurrent winget processes fight over its
/// source cache, exactly like BrewLock for brew on macOS. Reads (list/search/
/// show) don't take it: a table read next to an install is harmless.
static WINGET_LOCK: Mutex<()> = Mutex::new(());

const STOP_TIMEOUT: Duration = Duration::from_secs(15);

fn winget_lock() -> AppResult<std::sync::MutexGuard<'static, ()>> {
    WINGET_LOCK
        .lock()
        .map_err(|_| AppError::Io("winget lock poisoned".into()))
}

// ---- Services (SCM) --------------------------------------------------------

fn sc_exe() -> AppResult<String> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    Ok(format!("{root}\\System32\\sc.exe"))
}

/// Map an elevated `sc.exe` exit code back to our error kinds. 1056/1061/1062
/// are idempotent no-ops (already running / already stopped) — the same
/// tolerance `brew services start` gives a running service.
fn sc_result(code: i32, what: &str) -> AppResult<()> {
    match code {
        0 | 1056 | 1061 | 1062 => Ok(()),
        5 => Err(AppError::Forbidden(format!(
            "{what}: administrator rights are required"
        ))),
        1060 => Err(AppError::NotFound(format!(
            "{what}: service is not installed"
        ))),
        1072 => Err(AppError::Shell(format!(
            "{what}: marked for deletion — restart Windows to finish"
        ))),
        _ => Err(AppError::Shell(format!(
            "{what}: sc.exe exited with code {code}"
        ))),
    }
}

/// Direct API first (works when already elevated, or when the service's ACL
/// allows ordinary users); on access-denied, retry once through the UAC prompt
/// via `sc.exe` — but never when we're already elevated, since the prompt could
/// only re-run the call that just failed. Declining surfaces as Cancelled.
fn start_blocking(name: &str, linked: bool) -> AppResult<()> {
    match scm::start(name) {
        Ok(()) => {}
        Err(AppError::Forbidden(_)) if !elevate::is_elevated() => {
            sc_result(
                elevate::elevated_wait(&sc_exe()?, &["start", name])?,
                &format!("start {name}"),
            )?;
        }
        Err(e) => return Err(e),
    }
    if linked {
        set_start_type_blocking(name, true)?;
    }
    Ok(())
}

fn set_start_type_blocking(name: &str, auto: bool) -> AppResult<()> {
    use windows::Win32::System::Services::{SERVICE_AUTO_START, SERVICE_DEMAND_START};
    // A no-op re-config still goes through SCM (and possibly UAC) — check first
    // so linking an already-linked service is free.
    let want = if auto {
        SERVICE_AUTO_START
    } else {
        SERVICE_DEMAND_START
    };
    if scm::current_start_type(name) == Some(want) {
        return Ok(());
    }
    match scm::set_start_type(name, auto) {
        Ok(()) => Ok(()),
        Err(AppError::Forbidden(_)) if !elevate::is_elevated() => {
            // sc.exe wants `start=` and the value as separate tokens.
            let mode = if auto { "auto" } else { "demand" };
            sc_result(
                elevate::elevated_wait(&sc_exe()?, &["config", name, "start=", mode])?,
                &format!("configure {name}"),
            )
        }
        Err(e) => Err(e),
    }
}

fn stop_blocking(name: &str) -> AppResult<()> {
    match scm::stop(name, STOP_TIMEOUT) {
        Ok(()) => Ok(()),
        Err(AppError::Forbidden(_)) if !elevate::is_elevated() => {
            sc_result(
                elevate::elevated_wait(&sc_exe()?, &["stop", name])?,
                &format!("stop {name}"),
            )?;
            // sc.exe returns before STOPPED — poll for it so a restart can't race.
            scm::wait_stopped(name, STOP_TIMEOUT)
        }
        Err(e) => Err(e),
    }
}

/// `with_stats` adds cpu/mem/uptime per running service plus the listening
/// port (the health signal): services that should bind a port report healthy
/// only when one is open; the rest report healthy while their process is
/// measurable. No ServicesCache here — SCM enumeration is milliseconds, not
/// brew's 1.6s.
pub async fn list_services(with_stats: bool) -> AppResult<Vec<ServiceDto>> {
    tauri::async_runtime::spawn_blocking(move || {
        let services = scm::list_services()?;
        let (samples, listen) = if with_stats {
            let pids: Vec<u32> = services.iter().filter_map(|s| s.pid).collect();
            let samples = net::samples(&pids);
            let mut listen: HashMap<u32, u16> = HashMap::new();
            for row in net::sockets(true) {
                listen.entry(row.pid).or_insert(row.port);
            }
            (samples, listen)
        } else {
            (HashMap::new(), HashMap::new())
        };

        Ok(services
            .into_iter()
            .map(|s| {
                let noteworthy = crate::win::noteworthy(&s.name, &s.display_name);
                let expects_port = crate::win::expects_port(&s.name, &s.display_name);
                let sample = s.pid.and_then(|p| samples.get(&p));
                let port = s.pid.and_then(|p| listen.get(&p).copied());
                let healthy = if with_stats && s.status == "started" {
                    Some(if expects_port {
                        port.is_some()
                    } else {
                        sample.is_some()
                    })
                } else {
                    None
                };
                ServiceDto {
                    name: s.name,
                    status: s.status,
                    pid: s.pid,
                    cpu: sample.map(|s| s.cpu),
                    memory_bytes: sample.map(|s| s.rss_bytes),
                    uptime_secs: sample.map(|s| s.uptime_secs),
                    port,
                    healthy,
                    plist: None,
                    display_name: Some(s.display_name),
                    start_type: Some(s.start_type),
                    noteworthy: Some(noteworthy),
                }
            })
            .collect())
    })
    .await
    .map_err(|e| AppError::Shell(format!("services task failed: {e}")))?
}

/// linked → also flip the start type to Automatic (survives reboot); unlinked
/// starts without demoting — we never change persistence behind the user's
/// back, we only record the session intent.
pub async fn start_service(name: String, linked: bool, app: AppHandle) -> AppResult<()> {
    validate::validate_service_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        start_blocking(&name, linked)?;
        record_intent(&app.state::<SessionServices>(), &name, linked);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Shell(format!("service task failed: {e}")))?
}

pub async fn stop_service(name: String, app: AppHandle) -> AppResult<()> {
    validate::validate_service_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        stop_blocking(&name)?;
        app.state::<SessionServices>().unmark(&name);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Shell(format!("service task failed: {e}")))?
}

pub async fn restart_service(name: String, linked: bool, app: AppHandle) -> AppResult<()> {
    validate::validate_service_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        stop_blocking(&name)?;
        start_blocking(&name, linked)?;
        record_intent(&app.state::<SessionServices>(), &name, linked);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Shell(format!("service task failed: {e}")))?
}

/// Windows "linked" IS the start type — flipping it needs no restart, unlike
/// the macOS stop/start dance.
pub async fn set_link_intent(name: String, linked: bool, app: AppHandle) -> AppResult<()> {
    validate::validate_service_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        set_start_type_blocking(&name, linked)?;
        record_intent(&app.state::<SessionServices>(), &name, linked);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Shell(format!("service task failed: {e}")))?
}

// ---- Packages (WinGet) -----------------------------------------------------

fn winget_bin() -> AppResult<String> {
    winget::find_winget().ok_or_else(|| {
        AppError::NotFound("WinGet not found — install it from the Microsoft Store".into())
    })
}

fn winget_run(args: &[&str]) -> AppResult<String> {
    let bin = winget_bin()?;
    winget::run_winget(&bin, args)
}

/// Direct first; on access-denied, the same operation through the UAC prompt
/// (skipped when already elevated — the direct run already had the token).
/// run_winget's stable flags are re-added by hand because the elevated path
/// gets only an exit code back — no stdout to classify.
fn winget_mutate(args: &[&str]) -> AppResult<()> {
    match winget_run(args) {
        Ok(_) => Ok(()),
        Err(AppError::Forbidden(_)) if !elevate::is_elevated() => {
            let bin = winget_bin()?;
            let mut full = args.to_vec();
            full.extend(["--disable-interactivity", "--accept-source-agreements"]);
            let code = elevate::elevated_wait(&bin, &full)?;
            match code {
                0 => Ok(()),
                // UPDATE_NOT_APPLICABLE — the package is already present/current:
                // an idempotent success, matched by documented code (the elevated
                // path has no stdout to classify and the locale is unknown).
                c if c == winget::UPDATE_NOT_APPLICABLE => Ok(()),
                _ => Err(AppError::Shell(format!("WinGet failed (exit code {code})"))),
            }
        }
        Err(e) => Err(e),
    }
}

/// Read-only inventory probe for one id: `winget list --id <id> --exact`.
/// `Ok(None)` when the package is not installed (winget exits non-zero with
/// "No installed package found", surfaced by `run_winget` as NotFound). The
/// returned row carries the installed version and — when an upgrade exists —
/// the newer version in its `extra` (Available) column, so a single read answers
/// "is it installed, and is it current?".
fn winget_list_exact(id: &str) -> AppResult<Option<winget::WingetRow>> {
    match winget_run(&["list", "--id", id, "--exact"]) {
        Ok(out) => Ok(winget::find_row(&winget::parse_list(&out), id).cloned()),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// What the inventory says about an id. Pure projection of a `winget list` row
/// so the "installed / upgrade available" decision is unit-testable: a present
/// row is installed; a non-empty Available column means a newer version exists.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PackagePresence {
    Missing,
    Installed { upgrade: Option<String> },
}

fn package_presence(row: Option<&winget::WingetRow>) -> PackagePresence {
    match row {
        None => PackagePresence::Missing,
        Some(r) => PackagePresence::Installed {
            upgrade: r.extra.clone().filter(|v| !v.trim().is_empty()),
        },
    }
}

fn find_package(id: &str) -> AppResult<PackagePresence> {
    Ok(package_presence(winget_list_exact(id)?.as_ref()))
}

/// After an install/upgrade winget can return a beat before the package shows up
/// in `list`. Poll briefly so a genuine success isn't misreported as failure
/// (and, symmetrically, so a still-missing package finally surfaces).
fn verify_installed(id: &str) -> AppResult<()> {
    for attempt in 0..3 {
        if matches!(find_package(id)?, PackagePresence::Installed { .. }) {
            return Ok(());
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    Err(AppError::Shell(format!(
        "WinGet reported success but “{id}” did not appear in the installed list"
    )))
}

/// Returns the id that was installed — the card renames itself to it, so the
/// winget id is what the Services view ends up managing.
pub async fn install_formula(name: String) -> AppResult<String> {
    validate::validate_package_id(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        // Availability gate BEFORE any mutation: `winget show --id --exact`
        // fails exactly when the id doesn't exist in any configured source —
        // a missing id becomes a clear "not available in WinGet" NotFound
        // instead of winget's raw arg dump mid-install. Reads don't take the lock.
        winget_run(&["show", "--id", name.as_str(), "--exact"])?;
        // Inventory check: if it is already installed there is nothing to do —
        // return success without ever invoking `winget install`, which is what
        // used to fail with the misleading "already installed" error. A newer
        // version present means an idempotent success with no re-install either;
        // upgrading is an explicit user action, not an install side effect.
        if matches!(find_package(&name)?, PackagePresence::Installed { .. }) {
            return Ok(name);
        }
        let guard = winget_lock()?;
        winget_mutate(&[
            "install",
            "--id",
            name.as_str(),
            "--exact",
            "--accept-package-agreements",
        ])?;
        drop(guard);
        // Confirm the mutation actually landed before reporting success.
        verify_installed(&name)?;
        Ok(name)
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

pub async fn uninstall_formula(name: String, ignore_dependents: bool) -> AppResult<()> {
    validate::validate_package_id(&name)?;
    // winget has no --ignore-dependencies equivalent; accepted so the IPC
    // signature matches macOS, then dropped.
    let _ = ignore_dependents;
    tauri::async_runtime::spawn_blocking(move || {
        let _g = winget_lock()?;
        winget_mutate(&["uninstall", "--id", name.as_str(), "--exact"])
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

/// `winget search` → manageable ids (ARP/MSIX display rows filtered out), in
/// relevance order, deduped across sources.
pub async fn search_formulae(query: String) -> AppResult<Vec<String>> {
    if query.is_empty() || query.len() > 64 {
        return Err(AppError::InvalidName("invalid search query".into()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let out = match winget_run(&["search", &query]) {
            Ok(out) => out,
            // A search spanning no package is a normal empty result, not an
            // error — winget exits non-zero with "No package found".
            Err(AppError::NotFound(_)) => String::new(),
            Err(e) => return Err(e),
        };
        let mut seen = HashSet::new();
        Ok(winget::parse_search(&out)
            .into_iter()
            .map(|r| r.id)
            .filter(|id| winget::is_manageable(id) && seen.insert(id.clone()))
            .collect())
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

/// winget installs by id only — there is no version-variant concept like
/// brew's `redis@7.2`, so the versions menu offers the id itself.
pub async fn formula_versions(name: String) -> AppResult<Vec<String>> {
    validate::validate_package_id(&name)?;
    Ok(vec![name])
}

pub async fn list_packages() -> AppResult<Vec<PackageDto>> {
    tauri::async_runtime::spawn_blocking(move || {
        // Tolerated to empty — a machine without winget (or with a broken
        // source) shows an empty table, same as brew's tolerate() policy.
        let out = winget_run(&["list"]).unwrap_or_default();
        let mut rows: Vec<PackageDto> = winget::parse_list(&out)
            .into_iter()
            .map(|r| {
                let kind = winget::kind_for(&r.id, r.source.as_deref()).to_string();
                // `winget list` shows the newer version in the Available column
                // exactly when an upgrade exists — surface it so installed cards
                // can offer "Upgrade" without a second probe.
                let latest_version = r.extra.clone().filter(|v| !v.trim().is_empty());
                PackageDto {
                    name: r.id,
                    version: r.version,
                    kind,
                    outdated: latest_version.is_some(),
                    latest_version,
                    pinned: false,
                    display_name: Some(r.name),
                }
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

pub async fn packages_outdated() -> AppResult<Vec<OutdatedDto>> {
    tauri::async_runtime::spawn_blocking(move || {
        let out = winget_run(&["upgrade"]).unwrap_or_default();
        Ok(winget::parse_upgrade(&out)
            .into_iter()
            .filter(|r| r.extra.is_some() && winget::is_manageable(&r.id))
            .map(|r| OutdatedDto {
                name: r.id,
                latest_version: r.extra,
                pinned: false,
            })
            .collect())
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

/// winget exposes no reverse-dependency graph — report none instead of
/// guessing; the UI then never blocks an uninstall on invented dependents.
pub async fn package_dependents(name: String) -> AppResult<Vec<String>> {
    validate::validate_package_id(&name)?;
    Ok(Vec::new())
}

pub async fn upgrade_package(name: String) -> AppResult<()> {
    validate::validate_package_id(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _g = winget_lock()?;
        winget_mutate(&[
            "upgrade",
            "--id",
            name.as_str(),
            "--exact",
            "--accept-package-agreements",
        ])
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

pub async fn upgrade_all() -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let _g = winget_lock()?;
        winget_mutate(&["upgrade", "--all", "--accept-package-agreements"])
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

/// `brew update`'s twin: refresh winget's source indexes (no package changes).
pub async fn update_homebrew() -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let _g = winget_lock()?;
        winget_run(&["source", "update"])?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

/// Read-only id shape check: this one is NOT elevated and args go as an
/// argv array, so ARP/MSIX display ids (backslashes) are fine — only reject
/// anything that could read as a flag or break a quoted parameter string.
fn validate_display_id(id: &str) -> AppResult<()> {
    if !id.is_empty()
        && id.len() <= 200
        && !id.starts_with('-')
        && !id.chars().any(|c| c.is_control() || c == '"')
    {
        Ok(())
    } else {
        Err(AppError::InvalidName(format!("invalid package id: {id}")))
    }
}

/// Heuristic `is_service`: winget carries no service metadata, so match any
/// id segment of meaningful length against noteworthy service names —
/// `MySQL.MySQL` → MySQL80, `PostgreSQL.PostgreSQL.17` → postgresql-x64-17.
/// Short segments are skipped so `Git.Git` never matches.
fn service_hint(id: &str) -> bool {
    let Ok(services) = scm::list_services() else {
        return false;
    };
    id.split('.').filter(|s| s.len() >= 4).any(|seg| {
        let seg = seg.to_ascii_lowercase();
        services.iter().any(|s| {
            crate::win::noteworthy(&s.name, &s.display_name)
                && (s.name.to_ascii_lowercase().contains(&seg)
                    || s.display_name.to_ascii_lowercase().contains(&seg))
        })
    })
}

pub async fn package_info(name: String) -> AppResult<PackageInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_display_id(&name)?;
        let out = winget_run(&["show", "--id", &name, "--exact"])?;
        let (version, description, homepage) = winget::parse_show(&out);
        let is_cask = !winget::is_manageable(&name);
        let is_service = service_hint(&name);
        Ok(PackageInfo {
            name,
            description,
            homepage,
            version,
            dependencies: Vec::new(),
            is_service,
            is_cask,
        })
    })
    .await
    .map_err(|e| AppError::Shell(format!("winget task failed: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sc_exit_codes_map_to_kinds() {
        assert!(sc_result(0, "x").is_ok());
        assert!(sc_result(1056, "x").is_ok(), "already running is a no-op");
        assert!(sc_result(1062, "x").is_ok(), "not started is a no-op");
        assert!(matches!(sc_result(5, "x"), Err(AppError::Forbidden(_))));
        assert!(matches!(sc_result(1060, "x"), Err(AppError::NotFound(_))));
        assert!(matches!(sc_result(1072, "x"), Err(AppError::Shell(_))));
    }

    fn row(id: &str, version: &str, available: Option<&str>) -> winget::WingetRow {
        winget::WingetRow {
            name: id.rsplit('.').next().unwrap_or(id).to_string(),
            id: id.to_string(),
            version: version.to_string(),
            extra: available.map(str::to_string),
            source: Some("winget".to_string()),
        }
    }

    #[test]
    fn package_presence_reads_installed_and_upgrade() {
        assert_eq!(package_presence(None), PackagePresence::Missing);
        assert_eq!(
            package_presence(Some(&row("Git.Git", "2.51.2", None))),
            PackagePresence::Installed { upgrade: None }
        );
        assert_eq!(
            package_presence(Some(&row("Git.Git", "2.51.2", Some("2.55.0.5")))),
            PackagePresence::Installed {
                upgrade: Some("2.55.0.5".to_string())
            }
        );
        // A blank Available cell (header present, no upgrade) is not an upgrade.
        assert_eq!(
            package_presence(Some(&row("Git.Git", "2.51.2", Some("   ")))),
            PackagePresence::Installed { upgrade: None }
        );
    }

    #[test]
    fn display_ids_allow_arp_shapes_but_not_flags() {
        assert!(validate_display_id(r"ARP\User\X64\Discord").is_ok());
        assert!(validate_display_id(r"MSIX\Microsoft.Git").is_ok());
        assert!(validate_display_id("--exact").is_err());
        assert!(validate_display_id("bad\"id").is_err());
        assert!(validate_display_id("").is_err());
    }

    // Real machine, read-only: the full service path against SCM. Ignored in CI.
    #[test]
    #[ignore]
    fn lists_services_as_dtos() {
        let services = tauri::async_runtime::block_on(list_services(true)).unwrap();
        assert!(!services.is_empty());
        let spooler = services
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case("spooler"))
            .expect("spooler present");
        assert_eq!(spooler.start_type.as_deref(), Some("automatic"));
        assert_eq!(spooler.noteworthy, Some(false));
        assert!(spooler.plist.is_none());
        println!(
            "{} services, first: {}",
            services.len(),
            spooler.display_name.clone().unwrap_or_default()
        );
    }

    // Real machine, read-only: the availability gate must reject an id that
    // isn't in any configured source BEFORE winget ever tries to install it.
    // Redis.Redis-Official is the id from the original bug report. Ignored in CI.
    #[test]
    #[ignore]
    fn unavailable_package_is_rejected_by_the_gate() {
        let err = tauri::async_runtime::block_on(install_formula("Redis.Redis-Official".into()))
            .expect_err("a non-existent id must not reach winget install");
        assert!(
            matches!(err, AppError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    // Real machine, read-only: the native Windows Redis-compatible server the
    // popular list now points at does pass the gate. Ignored in CI.
    #[test]
    #[ignore]
    fn available_redis_package_passes_the_gate() {
        let out = winget::run_winget(
            &winget_bin().unwrap(),
            &["show", "--id", "Redis.Redis", "--exact"],
        )
        .unwrap();
        assert!(!out.is_empty());
    }

    // Real machine, read-only: the inventory probe finds an installed package
    // (and reports no error) while a bogus id reads as Missing. Ignored in CI.
    #[test]
    #[ignore]
    fn inventory_probe_finds_installed_and_missing() {
        assert!(
            matches!(
                find_package("Git.Git").unwrap(),
                PackagePresence::Installed { .. }
            ),
            "Git.Git should be installed on the dev machine"
        );
        assert_eq!(
            find_package("Redis.Redis-Official").unwrap(),
            PackagePresence::Missing
        );
    }

    // Real machine: re-installing an already-installed package must be an
    // idempotent Ok — this is the exact scenario that used to surface winget's
    // "already installed" exit as a failure. No mutation happens (the
    // inventory pre-check returns first). Ignored in CI.
    #[test]
    #[ignore]
    fn already_installed_package_installs_idempotently() {
        tauri::async_runtime::block_on(install_formula("Git.Git".into()))
            .expect("re-installing an installed package is a success");
    }

    // Real machine, read-only: `list_packages` marks installed packages and
    // carries a latest_version only when one exists. Ignored in CI.
    #[test]
    #[ignore]
    fn list_packages_reports_installed_state() {
        let rows = tauri::async_runtime::block_on(list_packages()).unwrap();
        let git = rows
            .iter()
            .find(|p| p.name == "Git.Git")
            .expect("Git.Git listed");
        assert!(
            git.latest_version.is_none() || git.outdated,
            "outdated must track latest_version"
        );
    }
}
