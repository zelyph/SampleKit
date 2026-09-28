//! Point the format at a real collection.
//!
//! ```console
//! cargo run --example round_trip -- <directory>
//! ```
//!
//! It copies every `.md` file to a scratch directory, reports what the format
//! makes of each one, then loads and saves each copy and reports what changed.
//! The source directory is never written to.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use samplekit::format::document::{self, Destination};
use samplekit::format::migration::{self, Action};

fn main() {
    let Some(source) = std::env::args().nth(1) else {
        eprintln!("usage: round_trip <directory>");
        std::process::exit(2);
    };
    // One directory per run. A shared one is deleted out from under whoever
    // else is reading it, which is a data-loss shape in a tool whose whole
    // claim is that it does not touch the source.
    let scratch = std::env::temp_dir().join(format!("samplekit-round-trip-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap_or_else(|e| panic!("{}: {e}", scratch.display()));

    let mut copies = Vec::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(&source)
        .unwrap_or_else(|e| panic!("{source}: {e}"))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    entries.sort();
    for path in &entries {
        let to = scratch.join(path.file_name().unwrap());
        fs::copy(path, &to).unwrap();
        copies.push(to);
    }
    println!("{} files copied to {}\n", copies.len(), scratch.display());

    // 1 · what the format makes of each file
    let plan = migration::plan(&copies);
    let mut actions: BTreeMap<&str, usize> = BTreeMap::new();
    let mut blocked = Vec::new();
    for entry in &plan.entries {
        let label = match &entry.action {
            Action::Upgrade => "Upgrade",
            Action::Canonicalize => "Canonicalize",
            Action::NoChange => "NoChange",
            Action::Blocked { reason } => {
                blocked.push((entry.origin.path.clone(), reason.clone()));
                "Blocked"
            }
        };
        *actions.entry(label).or_default() += 1;
    }
    println!("READ");
    for (label, count) in &actions {
        println!("  {label:<14} {count}");
    }
    for (path, reason) in blocked.iter().take(5) {
        let short: String = reason.chars().take(160).collect();
        println!(
            "  ! {}\n      {short}",
            path.file_name().unwrap().to_string_lossy()
        );
    }
    if let Some(first) = plan.entries.iter().find(|e| !e.alterations.is_empty()) {
        println!(
            "\n  alterations on {}:",
            first.origin.path.file_name().unwrap().to_string_lossy()
        );
        for alteration in &first.alterations {
            println!("    - {alteration}");
        }
    }

    // 2 · load, save, compare
    println!("\nROUND TRIP");
    let (mut identical, mut differing, mut failed) = (0usize, 0usize, 0usize);
    let mut first_failure = None;
    let mut first_difference = None;
    for path in &copies {
        let before = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        match document::load_sample(path) {
            Err(error) => {
                failed += 1;
                if first_failure.is_none() {
                    first_failure = Some((path.clone(), error.to_string()));
                }
            }
            Ok((mut sample, origin)) => {
                match document::save_sample(&mut sample, &Destination::Origin(origin)) {
                    Err(error) => {
                        failed += 1;
                        if first_failure.is_none() {
                            first_failure = Some((path.clone(), error.to_string()));
                        }
                    }
                    Ok(_) => {
                        let after = fs::read_to_string(path).unwrap();
                        if after == before {
                            identical += 1;
                        } else {
                            differing += 1;
                            if first_difference.is_none() {
                                first_difference = Some((path.clone(), diff(&before, &after)));
                            }
                        }
                    }
                }
            }
        }
    }
    println!("  byte-identical {identical} · differing {differing} · failed {failed}");
    if let Some((path, reason)) = first_failure {
        let short: String = reason.chars().take(300).collect();
        println!(
            "  ! {}\n      {short}",
            path.file_name().unwrap().to_string_lossy()
        );
    }
    if let Some((path, lines)) = first_difference {
        println!(
            "\n  first difference, in {}:",
            path.file_name().unwrap().to_string_lossy()
        );
        for line in lines {
            println!("    {line}");
        }
    }
}

/// The first few lines that differ, each marked with where it came from.
fn diff(before: &str, after: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let (mut b, mut a) = (before.lines(), after.lines());
    let mut number = 0;
    loop {
        number += 1;
        match (b.next(), a.next()) {
            (None, None) => break,
            (before, after) if before == after => continue,
            (before, after) => {
                lines.push(format!(
                    "{number:>5}  read    {}",
                    before.unwrap_or("<end>")
                ));
                lines.push(format!("{number:>5}  written {}", after.unwrap_or("<end>")));
                if lines.len() >= 12 {
                    break;
                }
            }
        }
    }
    lines
}
