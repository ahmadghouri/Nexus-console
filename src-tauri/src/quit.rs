#[cfg(unix)]
use crate::brew::run_brew;
#[cfg(unix)]
use crate::context::AppContext;
use crate::session::SessionServices;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// On quit: stop the services this app started ephemerally this session. Runs on a
/// detached thread joined with a hard 5s deadline so a hung `brew stop` can never
/// freeze the quit — we exit regardless once the deadline passes.
#[cfg(unix)]
pub fn stop_session_services(app: &AppHandle) {
    let names = app.state::<SessionServices>().snapshot();
    if names.is_empty() {
        return;
    }
    let Some(brew_bin) = app.state::<AppContext>().brew_bin.clone() else {
        return;
    };

    let handle = std::thread::spawn(move || {
        for name in names {
            let _ = run_brew(&brew_bin, &["services", "stop", &name]);
        }
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    while !handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Windows twin: same contract — stop what this session started, 5s hard
/// deadline, quit anyway. NO elevation here: a UAC prompt during shutdown is
/// worse than a service left running, so one that needs admin stays up (it was
/// never ours to begin with unless the user approved the prompt to start it).
#[cfg(windows)]
pub fn stop_session_services(app: &AppHandle) {
    let names = app.state::<SessionServices>().snapshot();
    if names.is_empty() {
        return;
    }

    let handle = std::thread::spawn(move || {
        for name in names {
            let _ = crate::win::scm::stop(&name, Duration::from_secs(3));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    while !handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}
