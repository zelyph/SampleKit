//! Six writers, one file, one winner.
//!
//! ```console
//! cargo build --example race
//! for i in 1 2 3 4 5 6; do ./target/debug/examples/race <file> $i.0 & done; wait
//! ```
//!
//! Each process loads the same file, waits so that they all hold the same
//! `Origin`, then writes a different value. Exactly one save must succeed and
//! the rest must be refused — by the lock or by the digest — with no lock left
//! behind. It is `document`'s concurrency invariant, which no single-process
//! test can observe.

fn main() {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().unwrap());
    let mine: f64 = args.next().unwrap().parse().unwrap();
    let (mut sample, origin) = samplekit::format::document::load_sample(&path).unwrap();
    // Everyone reads the same state, then each writes a different value.
    std::thread::sleep(std::time::Duration::from_millis(300));
    sample
        .set_property(
            samplekit::core::identifier::Identifier::new("mass").unwrap(),
            samplekit::core::property::Property::stored(
                samplekit::core::value::Value::number(mine).unwrap(),
            ),
        )
        .unwrap();
    match samplekit::format::document::save_sample(
        &mut sample,
        &samplekit::format::document::Destination::Origin(origin),
    ) {
        Ok(_) => println!("saved {mine}"),
        Err(error) => println!(
            "refused {mine}: {}",
            error.to_string().split('.').next().unwrap()
        ),
    }
}
