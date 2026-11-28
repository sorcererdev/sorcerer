//! A copy of bistro with a `listing` generator: a list input `fields` read by the pattern
//! `{name}:{kind}:{target?}`, its kinds checked against `tables.toml`, and a template that writes
//! one attribute per field.

use tempfile::TempDir;

use super::bistro_fixtures::{bistro_project, write_file, write_generator};

pub const LISTING_TOML: &str = r#"name     = "Listing"
summary  = "A class with one attribute per field"
language = "python"

[inputs.model]
type   = "ident"
prompt = "The model"

[inputs.fields]
type    = "list"
item    = "{name}:{kind}:{target?}"
keys_of = { kind = "kinds" }
prompt  = "The fields, name:kind (price:int,notes:text?)"

[[edits]]
create   = "src/bistro/{= model|snake =}.py"
template = "listing.py.dt"
"#;

pub const LISTING_PY: &str = "class {= model|pascal =}:\n    {= for f in fields =}\n    {= f.name =}: {= kinds[f.kind].py =}{= if f.optional =} | None{= end =}\n    {= end =}\n";

pub const TABLES_TOML: &str =
    "[kinds.text]\npy = \"str\"\n\n[kinds.int]\npy = \"int\"\n\n[kinds.ref]\npy = \"int\"\n";

/// Where a project keeps its lookup tables.
pub const TABLES_PATH: &str = ".sorcerer/spellbook/tables.toml";

/// bistro with the `listing` generator and its tables.
pub fn bistro_with_listing() -> TempDir {
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "listing",
        LISTING_TOML,
        &[("listing.py.dt", LISTING_PY)],
    );
    write_file(dir.path(), TABLES_PATH, TABLES_TOML);
    dir
}
