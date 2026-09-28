//! Embeds the home-brewing demo, `examples/brewing/`, for the start page to
//! write into a folder the user names: the demo of the version installed, with
//! neither Python nor the network.
//!
//! A table of `include_bytes!` rather than a crate: it is the one thing a
//! build script does here, and `include_dir` would not rebuild the binary
//! when a file of the demo changes, on stable Rust. A content shared by
//! several steps — most brews and models are — is included once.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What running a step leaves beside it, never part of the demo; the same as
/// `examples/.gitignore` and `tools/check-docs.py` leave out.
const LEFT_BY_RUNS: [&str; 4] = [".samplekit", "__pycache__", ".venv", "out"];

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let demo = root.join("examples").join("brewing");
    // A folder: cargo reruns this when anything under it changes.
    println!("cargo:rerun-if-changed={}", demo.display());
    // No demo, no binary: one without it would offer a demo it cannot write.
    // tools/regenerate-demo.sh cannot answer this, since it builds the binary.
    let missing = format!(
        "{} is missing or empty: it is committed, and `git checkout -- examples/brewing` \
         brings it back",
        demo.display()
    );
    if !demo.is_dir() {
        panic!("{missing}");
    }
    let mut files = Vec::new();
    collect(&demo, &demo, &mut files);
    assert!(!files.is_empty(), "{missing}");
    files.sort();
    let mut contents: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut blobs = String::new();
    let mut table = String::new();
    for (name, path) in &files {
        let bytes = std::fs::read(path).expect("a file of the demo reads");
        let next = contents.len();
        let index = *contents.entry(bytes).or_insert_with(|| {
            let _ = writeln!(
                blobs,
                "const DEMO_{next}: &[u8] = include_bytes!({:?});",
                path.display().to_string()
            );
            next
        });
        let _ = writeln!(table, "    ({name:?}, DEMO_{index}),");
    }
    let generated = format!(
        "{blobs}\n/// The demo, `examples/brewing/`, as the binary was built with it: each\n\
         /// file's path under the demo's folder, `/`-separated, and its bytes.\n\
         pub static DEMO_FILES: &[(&str, &[u8])] = &[\n{table}];\n"
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by cargo"));
    std::fs::write(out.join("demo.rs"), generated).expect("OUT_DIR is writable");
}

/// The demo's files under `folder`, by their path from `demo`: links and
/// what running the steps leaves aside.
fn collect(demo: &Path, folder: &Path, files: &mut Vec<(String, PathBuf)>) {
    let entries =
        std::fs::read_dir(folder).unwrap_or_else(|error| panic!("{}: {error}", folder.display()));
    for entry in entries {
        let entry = entry.expect("a folder of the demo lists");
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().expect("a file of the demo has a type");
        if LEFT_BY_RUNS.contains(&name.as_str()) || name.ends_with(".pyc") || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect(demo, &path, files);
        } else {
            let relative: Vec<String> = path
                .strip_prefix(demo)
                .expect("under the demo")
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            files.push((relative.join("/"), path));
        }
    }
}
