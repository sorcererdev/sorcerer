//! Spellbook's generated regions as a writer (conjure's, lackey's) is told about them: the
//! lines between `spellbook:region` and `spellbook:end` are made from files, so a writer that
//! types one in, or thinks it may not add one, goes wrong.

/// The words that open a region spellbook keeps, after the file's comment marker.
pub const REGION_OPENS: &str = "spellbook:region";

/// The line a writer is given when a file it may edit holds a region.
pub const GENERATED_REGIONS_LINE: &str = "Lines inside a spellbook:region are generated from files: to add one, create the file, and saving adds its line.";

/// The line for a writer, when any of the texts holds a spellbook region; `None` otherwise.
///
/// Example: a `mod.rs` holding `// spellbook:region modules (from src/*.rs; …)` gives
/// [`GENERATED_REGIONS_LINE`]; a file with no region gives `None`.
pub fn generated_regions_line<'a>(
    texts: impl IntoIterator<Item = &'a str>,
) -> Option<&'static str> {
    texts
        .into_iter()
        .any(|text| text.contains(REGION_OPENS))
        .then_some(GENERATED_REGIONS_LINE)
}
