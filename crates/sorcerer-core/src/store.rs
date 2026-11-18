//! A product's versioned state: one JSON file, `{"version": N, "data": …}`,
//! updated under an exclusive lock on `<file>.lock` (an OS advisory lock,
//! released if the process dies) and written to a temporary file that is
//! renamed over the old one, so a reader never sees half a file.
//!
//! A file that cannot be read (corrupt, or written by a newer sorcerer) is
//! `Unreadable` and is never overwritten: starting over would bring back
//! everything the user dismissed.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Project, Result};

pub trait Versioned: Serialize + DeserializeOwned + Default {
    const VERSION: u32;
}

#[derive(Debug)]
pub enum Snapshot<T> {
    Absent,
    Current(T),
    Unreadable(String),
}

pub struct Store<T: Versioned> {
    path: PathBuf,
    lock: PathBuf,
    _t: PhantomData<fn() -> T>,
}

#[derive(serde::Deserialize)]
struct Envelope {
    version: u32,
    data: serde_json::Value,
}

#[derive(Serialize)]
struct EnvelopeOut<'a, T> {
    version: u32,
    data: &'a T,
}

impl<T: Versioned> Store<T> {
    pub fn at(path: PathBuf) -> Store<T> {
        let mut lock = path.clone().into_os_string();
        lock.push(".lock");
        Store {
            path,
            lock: lock.into(),
            _t: PhantomData,
        }
    }

    /// A store at REL under the project's state directory, which is
    /// created (with its `.gitignore`) first. Products open stores this way.
    pub fn in_project(project: &Project, rel: &str) -> Result<Store<T>> {
        Ok(Store::at(project.state_dir()?.join(rel)))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self) -> Result<Snapshot<T>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::Absent),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Ok(Snapshot::Unreadable("not UTF-8".into()));
            }
            Err(e) => return Err(Error::io(&self.path, e)),
        };
        let env: Envelope = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(e) => return Ok(Snapshot::Unreadable(format!("not a store file: {e}"))),
        };
        if env.version != T::VERSION {
            return Ok(Snapshot::Unreadable(format!(
                "version {}, and this sorcerer reads version {}",
                env.version,
                T::VERSION
            )));
        }
        Ok(match serde_json::from_value(env.data) {
            Ok(t) => Snapshot::Current(t),
            Err(e) => {
                Snapshot::Unreadable(format!("version {} data does not fit: {e}", env.version))
            }
        })
    }

    /// Lock, read (absent is `T::default()`), apply F, write aside, rename.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R> {
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
        let mut value = match self.read()? {
            Snapshot::Absent => T::default(),
            Snapshot::Current(t) => t,
            Snapshot::Unreadable(why) => {
                return Err(Error::StoreUnreadable {
                    path: self.path.clone(),
                    why,
                });
            }
        };
        let r = f(&mut value);
        self.write(&value)?;
        drop(lock);
        Ok(r)
    }

    fn write(&self, value: &T) -> Result<()> {
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(format!(".tmp-{}", std::process::id()));
        let tmp = PathBuf::from(tmp);
        let bytes = serde_json::to_vec_pretty(&EnvelopeOut {
            version: T::VERSION,
            data: value,
        })
        .map_err(|e| Error::Config {
            file: self.path.clone(),
            why: e.to_string(),
        })?;
        let mut f = File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| Error::io(&tmp, e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| Error::io(&self.path, e))
    }
}

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
                        "store::tests::child_increments",
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
