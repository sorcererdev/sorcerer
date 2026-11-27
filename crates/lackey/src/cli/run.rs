//! `lackey FILE:LINE "instruction"`: one task from the terminal. Its changes are written to
//! the disk, what the agent says is printed when it ends, and its questions take their default.
//! It is how the harness is tried without an editor; the editor's server is the product.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::disk_editor::DiskEditor;
use crate::adapters::task_runner::TaskRunner;
use crate::adapters::wiring::{notes_dir, task_deps};
use crate::domain::task::TaskState;
use crate::usecases::run_task::TaskOrder;

use super::RunArgs;
use super::run_cli::Wiring;

/// Runs one task and prints what it said. Exit 0 when the task finished, 1 when it failed or
/// was refused, 2 when it could not run.
pub(super) fn run_here(
    args: &RunArgs,
    cwd: &Path,
    project: Project,
    env: &dyn Env,
    wiring: &Wiring,
    out: &mut dyn Write,
) -> Result<i32> {
    let target = args
        .target
        .as_deref()
        .ok_or_else(|| anyhow!("give FILE:LINE and an instruction"))?;
    let instruction = args
        .instruction
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| anyhow!("give an instruction after {target}"))?;
    let (file, line) = place_of(target)?;
    let relative = project_relative(&project, cwd, &file)?;
    let text = std::fs::read_to_string(project.root.join(&relative))
        .with_context(|| format!("{} cannot be read", relative.display()))?;
    let editor = Arc::new(DiskEditor::new(project.root.clone(), notes_dir(&project)?));
    let deps = task_deps(
        &project,
        GrammarCache::user(env),
        editor.clone(),
        (wiring.agent)(&project)?,
        wiring.clock.clone(),
    )?;
    let runner = TaskRunner::open(deps)?;
    let order = TaskOrder {
        instruction: instruction.to_string(),
        file: forward_slashes(&relative),
        text,
        line,
        unsaved: Vec::new(),
    };
    let ended = runner.run(&order);
    for said in editor.take_said() {
        writeln!(out, "{said}")?;
    }
    Ok(match ended {
        Ok(record) if record.state == TaskState::Done => 0,
        Ok(_) => 1,
        Err(refused) => {
            writeln!(out, "{}", refused.text())?;
            1
        }
    })
}

/// `FILE:LINE`; the colon that counts is the last one, so a Windows drive letter works.
fn place_of(text: &str) -> Result<(PathBuf, u32)> {
    let not_a_place = || anyhow!("`{text}` is not a place: write FILE:LINE");
    let (file, line) = text.rsplit_once(':').ok_or_else(not_a_place)?;
    let line: u32 = line.parse().map_err(|_| not_a_place())?;
    if file.is_empty() || line == 0 {
        return Err(not_a_place());
    }
    Ok((file.into(), line))
}

/// The file as the project knows it: relative to the root. A file outside it is an error.
fn project_relative(project: &Project, cwd: &Path, file: &Path) -> Result<PathBuf> {
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    let resolved =
        canonical(&absolute).with_context(|| format!("{} cannot be read", file.display()))?;
    match resolved.strip_prefix(&project.root) {
        Ok(relative) => Ok(relative.to_path_buf()),
        Err(_) => bail!("{} is outside the project", file.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_a_file_and_a_line() {
        assert_eq!(
            place_of("src/a.txt:12").unwrap(),
            (PathBuf::from("src/a.txt"), 12)
        );
        assert_eq!(place_of("C:\\p\\a.txt:3").unwrap().1, 3);
        for bad in ["a.txt", "a.txt:0", ":3", "a.txt:x"] {
            assert!(place_of(bad).is_err(), "{bad}");
        }
    }
}
