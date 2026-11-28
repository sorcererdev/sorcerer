//! A pack installed: its `.sorcerer/` copied into the project, and where it came from recorded.

use crate::domain::pack::{
    PACK_RECORD, PackAddress, PackRecord, PackStop, SPELLBOOK_DIR, pack_name,
};
use crate::ports::pack_source::{PackFile, PackSource, SorcererDir};

/// What installing reads from and writes to.
pub struct InstallDeps<'a> {
    pub source: &'a dyn PackSource,
    pub project: &'a dyn SorcererDir,
}

/// What installing did: the record written, the files copied, and the files the project had
/// already and kept (paths under `.sorcerer/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPack {
    pub record: PackRecord,
    pub copied: Vec<String>,
    pub kept: Vec<String>,
}

/// Installs a pack: (1) refuses a project that has `.sorcerer/spellbook/` already, before
/// fetching anything; (2) fetches the pack at its revision; (3) copies each file of its
/// `.sorcerer/` the project does not have, and keeps the project's own where it has one (its
/// `config.toml`, say); (4) writes `.sorcerer/pack.toml` with the pack's name, repository and the
/// commit taken.
///
/// Example: into a project with only `.sorcerer/config.toml`, a pack with `config.toml`,
/// `scry.toml` and one generator copies `scry.toml` and the generator, keeps `config.toml`, and
/// records `{ name = "rust-web", repo = "…", rev = "<sha>" }`.
pub fn install_pack(deps: &InstallDeps, address: &PackAddress) -> Result<InstalledPack, PackStop> {
    if deps.project.holds(SPELLBOOK_DIR) {
        return Err(PackStop::HasSpellbook);
    }
    let fetched = deps.source.fetch(address)?;
    let (kept, to_copy): (Vec<PackFile>, Vec<PackFile>) = fetched
        .files
        .into_iter()
        .filter(|file| file.path != PACK_RECORD)
        .partition(|file| deps.project.holds(&file.path));
    for file in &to_copy {
        deps.project.write(&file.path, &file.bytes)?;
    }
    let record = PackRecord {
        name: pack_name(fetched.manifest.as_deref(), &address.repo),
        repo: address.repo.clone(),
        rev: fetched.rev,
    };
    deps.project.write(PACK_RECORD, record.text().as_bytes())?;
    Ok(InstalledPack {
        record,
        copied: to_copy.into_iter().map(|file| file.path).collect(),
        kept: kept.into_iter().map(|file| file.path).collect(),
    })
}
