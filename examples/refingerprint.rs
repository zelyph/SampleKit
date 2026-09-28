//! Recompute the fingerprints a file already claims, so that a hand-written
//! fixture stops contradicting itself.
//!
//! ```console
//! cargo run --example refingerprint -- <file.md>
//! ```
//!
//! It rewrites only the digests: every value, every unit and the note are
//! untouched. A file whose `computed` names an input it does not have is left
//! alone and reported.

use samplekit::core::property::Fingerprint;
use samplekit::core::value::Value;
use samplekit::format::schema::Recorded;
use samplekit::format::{document, fingerprint};

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: refingerprint <file.md>");
        std::process::exit(2);
    };
    let path = std::path::PathBuf::from(path);
    let text = std::fs::read_to_string(&path).expect("readable");
    let mut parsed = document::parse(&text).expect("a sample");

    // Every property's own digest, over the value form the file writes.
    let digests: Vec<(String, String)> = parsed
        .schema
        .properties
        .iter()
        .map(|(name, property)| (name.to_string(), fingerprint::of(property).to_string()))
        .collect();
    let lookup = |name: &str| {
        digests
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, digest)| digest.clone())
    };

    let mut fixed = 0usize;
    let mut orphans = Vec::new();
    for (name, property) in parsed.schema.properties.iter_mut() {
        // A recorded input keeps its name and gets the digest its source
        // actually has.
        if let Some(computed) = property.computed.as_mut() {
            // Every channel's inputs, since a record may name them.
            let held = [
                computed.quantity.as_mut(),
                computed.value.as_mut(),
                computed.uncertainty.as_mut(),
            ];
            for (input, recorded) in held.into_iter().flatten().flat_map(|map| map.iter_mut()) {
                let spelled = input.to_string();
                match lookup(&spelled) {
                    Some(digest) => {
                        // An edited mark stays: only the digest is corrected.
                        let correct = match recorded {
                            Recorded::Edited(_) => Recorded::Edited(Fingerprint::edited(digest)),
                            Recorded::Value(_) => Recorded::Value(Value::text(digest)),
                        };
                        if *recorded != correct {
                            *recorded = correct;
                            fixed += 1;
                        }
                    }
                    None => orphans.push(format!("{name}: {spelled}")),
                }
            }
        }
        if property.fingerprint.is_some() {
            let own = match &property.fingerprint {
                Some(marked) if marked.is_edited() => fingerprint::of(property).marked_edited(),
                _ => fingerprint::of(property),
            };
            if property.fingerprint.as_ref() != Some(&own) {
                property.fingerprint = Some(own);
                fixed += 1;
            }
        }
    }

    let written = document::write(&parsed);
    std::fs::write(&path, &written).expect("writable");
    println!("{}: {fixed} digest(s) corrected", path.display());
    for orphan in &orphans {
        println!("  ! names an input this file does not have — {orphan}");
    }
}
