//! `scry trust`: puts the project on the user's trust list, so the editor's server reviews it
//! although its config sets something that runs code or moves the API key; `--revoke` takes it
//! off. The command line itself never asks: typing `scry` in a project is the consent.

use std::io::Write;

use anyhow::{Context, Result};
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::{TrustReason, needs_trust};
use sorcerer_core::ports::env::Env;

/// Trusts the project's root (always, also when nothing needs trust today) and prints what in
/// its config needed it; with `revoke`, takes the root off the list.
pub(super) fn trust(
    project: &Project,
    env: &dyn Env,
    revoke: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let list = TrustList::user(env)
        .context("there is no place for the trust list: neither XDG_CONFIG_HOME nor HOME is set")?;
    let root = &project.root;
    if revoke {
        let was_there = list.revoke(root)?;
        write!(out, "{}", revoked_text(root, was_there))?;
    } else {
        list.trust(root)?;
        write!(out, "{}", trusted_text(root, &needs_trust(project)))?;
    }
    Ok(0)
}

// The command's text.

/// `trusted: <root>`, then each setting that needs trust, or a line saying none does.
fn trusted_text(root: &std::path::Path, reasons: &[TrustReason]) -> String {
    let settings: String = if reasons.is_empty() {
        "(nothing in its config needs trust today)\n".to_string()
    } else {
        reasons
            .iter()
            .map(|r| format!("{} = {}\n", r.key, r.value))
            .collect()
    };
    format!("trusted: {}\n{settings}", root.display())
}

fn revoked_text(root: &std::path::Path, was_there: bool) -> String {
    if was_there {
        format!("no longer trusted: {}\n", root.display())
    } else {
        format!("was not trusted: {}\n", root.display())
    }
}
