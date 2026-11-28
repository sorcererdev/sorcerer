//! `spellbook init --pack <repo>[@rev]`: copies a pack's `.sorcerer/` into the project and records
//! where it came from.

use std::io::Write;
use std::path::Path;

use anyhow::Result;

use crate::adapters::disk_sorcerer_dir::DiskSorcererDir;
use crate::adapters::git_pack_source::GitPackSource;
use crate::domain::pack::{PACK_RECORD, PackAddress, pack_stop_text};
use crate::usecases::install_pack::{InstallDeps, InstalledPack, install_pack};

/// Installs the pack into the project at `root` and prints what was copied and kept. A stop (a
/// spellbook there already, a fetch that failed) prints its sentence and exits 2.
pub(super) fn init(
    root: &Path,
    pack: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let address = match PackAddress::parse(pack) {
        Ok(address) => address,
        Err(why) => {
            writeln!(err, "spellbook: {why}")?;
            return Ok(2);
        }
    };
    let deps = InstallDeps {
        source: &GitPackSource,
        project: &DiskSorcererDir {
            root: root.to_path_buf(),
        },
    };
    match install_pack(&deps, &address) {
        Ok(installed) => {
            write!(out, "{}", installed_text(&installed))?;
            Ok(0)
        }
        Err(stop) => {
            writeln!(err, "spellbook: {}", pack_stop_text(&stop))?;
            Ok(2)
        }
    }
}

/// What was installed, in a few lines.
///
/// Example:
/// ```text
/// pack rust-web at 1a2b3c4d5e6f: 24 files copied into .sorcerer/
///   = .sorcerer/config.toml  (the project's own, kept)
///   + .sorcerer/pack.toml  (where the pack came from)
/// next: spellbook list
/// ```
fn installed_text(installed: &InstalledPack) -> String {
    let record = &installed.record;
    let short: String = record.rev.chars().take(12).collect();
    let mut text = format!(
        "pack {} at {short}: {} copied into .sorcerer/\n",
        record.name,
        super::format::count(installed.copied.len(), "file", "files")
    );
    for kept in &installed.kept {
        text.push_str(&format!(
            "  = .sorcerer/{kept}  (the project's own, kept)\n"
        ));
    }
    text.push_str(&format!(
        "  + .sorcerer/{PACK_RECORD}  (where the pack came from)\nnext: spellbook list\n"
    ));
    text
}
