//! Data and rules: no I/O, nothing from `ports` or `adapters`.

// spellbook:region domain-modules (from crates/spellbook/src/domain/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod allow;
pub mod anchor;
pub mod baseline;
pub mod change;
pub mod config;
pub mod finding;
pub mod fix;
pub mod generator;
pub mod generator_checks;
pub mod generator_problems;
pub mod holes;
pub mod holes_table;
pub mod inputs;
pub mod language_syntax;
pub mod list_items;
pub mod open_hole_places;
pub mod pack;
pub mod path_pattern;
pub mod plan;
pub mod record;
pub mod region;
pub mod report;
pub mod rule;
pub mod rule_query;
pub mod rule_settings;
pub mod tables;
pub mod template;
pub mod template_checks;
// spellbook:end domain-modules
