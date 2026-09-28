//! The tests of `opening`.

use std::path::{Path, PathBuf};

use samplekit::presentation::opening::{self, OpenError};

/// The environment is the process's: a test that sets it holds this, so that
/// no other reads it halfway.
static ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn nothing_opens_over_ssh_without_a_display() {
    let _held = ENVIRONMENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe {
        std::env::set_var("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 51000");
        std::env::remove_var("DISPLAY");
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    let refused = opening::refusal(Path::new("images/a.png"));
    if cfg!(target_os = "linux") {
        assert!(matches!(refused, Some(OpenError::NoDisplay { .. })));
        assert!(refused.unwrap().to_string().contains("images/a.png"));
    }
    unsafe {
        std::env::set_var("DISPLAY", ":0");
    }
    assert!(opening::refusal(Path::new("images/a.png")).is_none());
}

#[test]
fn a_file_is_navigated_to_by_its_folder() {
    assert_eq!(
        opening::folder_of(Path::new("images/a.png")),
        PathBuf::from("images")
    );
    let here = dunce::canonicalize(std::env::temp_dir()).unwrap();
    assert_eq!(opening::folder_of(&here), here);
    assert_eq!(opening::folder_of(Path::new("a.png")), PathBuf::from("."));
}

#[cfg(target_os = "linux")]
#[test]
#[cfg(target_os = "linux")]
fn an_opened_file_leaves_no_process_behind() {
    // `opener` dropped the xdg-open it started without waiting for it: each
    // file opened in a long workbench session left a zombie process.
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{Duration, Instant};
    let _held = ENVIRONMENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scratch = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-opening-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("bin")).unwrap();
    let said = scratch.join("opened");
    let opener = scratch.join("bin/xdg-open");
    std::fs::write(
        &opener,
        format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", said.display()),
    )
    .unwrap();
    std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::remove_var("SSH_CONNECTION");
        std::env::remove_var("SSH_TTY");
        std::env::set_var("PATH", format!("{}:{path}", scratch.join("bin").display()));
    }
    let opened = opening::open(Path::new("images/a.png"));
    unsafe {
        std::env::set_var("PATH", &path);
    }
    opened.unwrap();
    let started = Instant::now();
    while !said.exists() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&said).unwrap(), "images/a.png");
    // Reaped: no child of this process is left a zombie. Given a moment,
    // since the reaping thread runs beside this one.
    let zombies = || -> Vec<String> {
        let mut found = Vec::new();
        for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
            let children =
                std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
            for child in children.split_whitespace() {
                let stat =
                    std::fs::read_to_string(format!("/proc/{child}/stat")).unwrap_or_default();
                // `pid (comm) state …`: the state follows the closing parenthesis.
                if stat
                    .rsplit_once(')')
                    .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
                {
                    found.push(stat);
                }
            }
        }
        found
    };
    let started = Instant::now();
    while !zombies().is_empty() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(zombies().is_empty(), "{:?}", zombies());
    let _ = std::fs::remove_dir_all(&scratch);
}
