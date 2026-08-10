//! Regenerates the committed Restore backup fixtures.
//!
//! Run with `cargo run --example generate-restore-fixtures -p
//! weavelit-server-restore`. The generator is a development target only; it is
//! never linked into the Server binary. `tests/fixtures.rs` fails when the
//! committed bytes and this generator disagree.

#[path = "../tests/support/mod.rs"]
mod support;

fn main() -> std::io::Result<()> {
    let directory = support::fixture_directory();
    support::generate().write(&directory)?;
    println!("wrote Restore fixtures to {}", directory.display());
    Ok(())
}
