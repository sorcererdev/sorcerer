//! A product's saved state: one JSON file, `{"version": 2, "data": …}`.
//!
//! Two runs may update the same file at once (two editors, an editor and a terminal), so an
//! update takes an exclusive lock on a file beside it (`findings.json.lock`) first. It is an
//! operating-system lock, released even if the process dies. The new contents are written to a
//! temporary file that is then renamed over the old one, so a reader never sees half a file.
//!
//! A file that cannot be read (damaged, or written by a newer sorcerer) is reported as
//! unreadable and never overwritten: starting over would bring back everything the user
//! dismissed.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::domain::project::Project;
pub use crate::domain::versioned::{Snapshot, Versioned};
use crate::{Error, Result};

/// One saved state file holding a `T`.
pub struct Store<T: Versioned> {
    path: PathBuf,
    lock: PathBuf,
    _t: PhantomData<fn() -> T>,
}

/// The file as read: its version number, and its data not yet read into `T`.
#[derive(serde::Deserialize)]
struct Envelope {
    version: u32,
    data: serde_json::Value,
}

/// The file as written.
#[derive(Serialize)]
struct EnvelopeOut<'a, T> {
    version: u32,
    data: &'a T,
}

impl<T: Versioned> Store<T> {
    /// A store kept in any file; its lock file is the same path with `.lock` added.
    pub fn at(path: PathBuf) -> Store<T> {
        let mut lock = path.clone().into_os_string();
        lock.push(".lock");
        Store {
            path,
            lock: lock.into(),
            _t: PhantomData,
        }
    }

    /// A store at a path under the project's state directory, such as
    /// `in_project(&project, "scry/findings.json")`. The state directory is created (with its
    /// `.gitignore`) first. Products open their stores this way.
    pub fn in_project(project: &Project, relative: &str) -> Result<Store<T>> {
        Ok(Store::at(project.state_dir()?.join(relative)))
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the saved state without taking the lock. A missing file is `Absent`; a file that
    /// is not UTF-8, not a store file, of another version, or whose data does not fit `T` is
    /// `Unreadable` with the reason. Only an error reading the disk itself is an `Err`.
    pub fn read(&self) -> Result<Snapshot<T>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(snapshot_of(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Snapshot::Absent),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                Ok(Snapshot::Unreadable("not UTF-8".into()))
            }
            Err(e) => Err(Error::io(&self.path, e)),
        }
    }

    /// Changes the saved state safely while other runs may be changing it too, and returns
    /// what the change returned.
    ///
    /// It takes the lock, reads the state (a missing file starts from `T::default()`), applies
    /// the change, writes the result, and lets go of the lock. An unreadable file is
    /// `Error::StoreUnreadable`, and the change is not applied.
    ///
    /// Example: `store.update(|counter| counter.n += 1)` from eight threads, 50 times each,
    /// leaves `n` at 400.
    pub fn update<R>(&self, change: impl FnOnce(&mut T) -> R) -> Result<R> {
        let _lock = self.take_lock()?;
        let mut value = self.read_to_change()?;
        let result = change(&mut value);
        self.write(&value)?;
        Ok(result)
    }

    /// Waits for the lock and holds it until the returned file is dropped.
    fn take_lock(&self) -> Result<File> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.lock)
            .map_err(|e| Error::io(&self.lock, e))?;
        lock.lock().map_err(|e| Error::io(&self.lock, e))?;
        Ok(lock)
    }

    fn read_to_change(&self) -> Result<T> {
        match self.read()? {
            Snapshot::Absent => Ok(T::default()),
            Snapshot::Current(value) => Ok(value),
            Snapshot::Unreadable(why) => Err(Error::StoreUnreadable {
                path: self.path.clone(),
                why,
            }),
        }
    }

    /// Writes to a temporary file beside the store, flushes it to disk, then renames it over
    /// the store, so the store is always either the old contents or the new.
    fn write(&self, value: &T) -> Result<()> {
        let temporary = self.temporary_path();
        let bytes = serde_json::to_vec_pretty(&EnvelopeOut {
            version: T::VERSION,
            data: value,
        })
        .map_err(|e| Error::Config {
            file: self.path.clone(),
            why: e.to_string(),
        })?;
        let mut file = File::create(&temporary).map_err(|e| Error::io(&temporary, e))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| Error::io(&temporary, e))?;
        std::fs::rename(&temporary, &self.path).map_err(|e| Error::io(&self.path, e))
    }

    /// `findings.json.tmp-<process id>`: one per process, so two processes never share one.
    fn temporary_path(&self) -> PathBuf {
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(format!(".tmp-{}", std::process::id()));
        PathBuf::from(temporary)
    }
}

/// What a store file's text holds: the state, or why it cannot be read.
fn snapshot_of<T: Versioned>(text: &str) -> Snapshot<T> {
    let envelope: Envelope = match serde_json::from_str(text) {
        Ok(envelope) => envelope,
        Err(e) => return Snapshot::Unreadable(format!("not a store file: {e}")),
    };
    if envelope.version != T::VERSION {
        return Snapshot::Unreadable(format!(
            "version {}, and this sorcerer reads version {}",
            envelope.version,
            T::VERSION
        ));
    }
    match serde_json::from_value(envelope.data) {
        Ok(value) => Snapshot::Current(value),
        Err(e) => Snapshot::Unreadable(format!(
            "version {} data does not fit: {e}",
            envelope.version
        )),
    }
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[derive(serde::Serialize, serde::Deserialize, Default, Debug, PartialEq)]
    struct Counter {
        n: u32,
    }

    impl Versioned for Counter {
        const VERSION: u32 = 1;
    }

    fn store(d: &Path) -> Store<Counter> {
        Store::at(d.join("s/counter.json"))
    }

    #[test]
    fn absent_then_current() {
        let d = tempfile::TempDir::new().unwrap();
        let s = store(d.path());
        assert!(matches!(s.read().unwrap(), Snapshot::Absent));
        assert_eq!(s.update(|c| c.n += 1).unwrap(), ());
        assert!(matches!(
            s.read().unwrap(),
            Snapshot::Current(Counter { n: 1 })
        ));
        let text = std::fs::read_to_string(d.path().join("s/counter.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v, serde_json::json!({"version": 1, "data": {"n": 1}}));
    }

    #[test]
    fn a_newer_version_is_unreadable_and_never_overwritten() {
        let d = tempfile::TempDir::new().unwrap();
        let s = store(d.path());
        std::fs::create_dir_all(d.path().join("s")).unwrap();
        let newer = "{\"version\": 2, \"data\": {}}";
        std::fs::write(d.path().join("s/counter.json"), newer).unwrap();
        match s.read().unwrap() {
            Snapshot::Unreadable(why) => assert!(why.contains("version 2"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            s.update(|c| c.n += 1),
            Err(crate::Error::StoreUnreadable { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(d.path().join("s/counter.json")).unwrap(),
            newer
        );
    }

    #[test]
    fn corrupt_json_is_unreadable_and_untouched() {
        let d = tempfile::TempDir::new().unwrap();
        let s = store(d.path());
        std::fs::create_dir_all(d.path().join("s")).unwrap();
        std::fs::write(d.path().join("s/counter.json"), "{\"version\": 1, \"da").unwrap();
        assert!(matches!(s.read().unwrap(), Snapshot::Unreadable(_)));
        assert!(s.update(|c| c.n += 1).is_err());
        assert_eq!(
            std::fs::read_to_string(d.path().join("s/counter.json")).unwrap(),
            "{\"version\": 1, \"da"
        );
    }

    #[test]
    fn concurrent_updates_in_threads_lose_nothing() {
        let d = tempfile::TempDir::new().unwrap();
        std::thread::scope(|sc| {
            for _ in 0..8 {
                sc.spawn(|| {
                    let s = store(d.path());
                    for _ in 0..50 {
                        s.update(|c| c.n += 1).unwrap();
                    }
                });
            }
        });
        assert!(matches!(
            store(d.path()).read().unwrap(),
            Snapshot::Current(Counter { n: 400 })
        ));
    }

    /// Run as a child process by the test below: 50 updates to the store at $STORE_CHILD.
    #[test]
    fn child_increments() {
        if let Ok(p) = std::env::var("STORE_CHILD") {
            let s: Store<Counter> = Store::at(p.into());
            for _ in 0..50 {
                s.update(|c| c.n += 1).unwrap();
            }
        }
    }

    #[test]
    fn concurrent_updates_in_processes_lose_nothing() {
        let d = tempfile::TempDir::new().unwrap();
        let path = d.path().join("s/counter.json");
        let kids: Vec<_> = (0..2)
            .map(|_| {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "adapters::json_store::tests::child_increments",
                        "--test-threads",
                        "1",
                    ])
                    .env("STORE_CHILD", &path)
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut k in kids {
            assert!(k.wait().unwrap().success());
        }
        assert!(matches!(
            store(d.path()).read().unwrap(),
            Snapshot::Current(Counter { n: 100 })
        ));
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        let d = tempfile::TempDir::new().unwrap();
        let s = store(d.path());
        s.update(|c| c.n = 3).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(d.path().join("s"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec!["counter.json", "counter.json.lock"]);
    }
}
