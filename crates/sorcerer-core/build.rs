//! The shipped languages are embedded from `library/languages/` (`include_dir!` in
//! `domain/language.rs`). The compiler notices a changed file there but not a new one, so a new
//! language or query file would be missing until something else rebuilt the crate: cargo scans
//! the whole folder instead.

fn main() {
    println!("cargo:rerun-if-changed=library/languages");
}
