//! Grammars at run time. No grammar is compiled into a binary: a language
//! file names `{ git, rev }` (or a local directory), which is fetched once
//! into the user's cache, compiled with `cc` into a shared library, and
//! loaded with `dlopen`. Ported from olifant's `Language/Grammar.hs`.
//!
//! Layout under the cache directory:
//!
//! ```text
//! src/<name>-<rev7>/          a git checkout at the pinned revision
//! <name>-<key>.<dylib|so>     the compiled grammar
//! ```
//!
//! Every write goes to a temporary name first and is renamed into place, so
//! a reader never sees half a checkout or half a library, and two processes
//! building the same grammar at once race harmlessly.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::domain::fnv::fnv64_hex;
use crate::domain::language::{GrammarSource, Languages, Layer, Loaded};
use crate::ports::env::Env;
use crate::{Error, Result};

/// Where grammars are fetched and built: per user, not per project.
#[derive(Debug, Clone)]
pub struct GrammarCache {
    pub dir: PathBuf,
}

impl GrammarCache {
    /// `$XDG_CACHE_HOME/sorcerer/grammars`, else `~/.cache/sorcerer/grammars`.
    pub fn user(env: &dyn Env) -> GrammarCache {
        let base = env
            .var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env.var("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(|| PathBuf::from(".cache"));
        GrammarCache {
            dir: base.join("sorcerer/grammars"),
        }
    }

    pub fn at(dir: PathBuf) -> GrammarCache {
        GrammarCache { dir }
    }
}

const EXT: &str = if cfg!(target_os = "macos") {
    "dylib"
} else {
    "so"
};

/// Loads NAME's grammar from SOURCE, fetching and building it first when
/// the cache has no library for it. Once per process per library: a
/// failure is remembered too (a missing compiler is reported once), except
/// a failed fetch or a file error, which may pass (offline at first start).
pub fn load(
    cache: &GrammarCache,
    name: &str,
    source: &GrammarSource,
) -> Result<tree_sitter::Language> {
    static LOADED: OnceLock<
        Mutex<HashMap<PathBuf, std::result::Result<tree_sitter::Language, String>>>,
    > = OnceLock::new();
    let path = library_path(cache, name, source)?;
    let mut loaded = LOADED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(r) = loaded.get(&path) {
        return r.clone().map_err(|why| Error::GrammarLoad {
            name: name.into(),
            why,
        });
    }
    let r = build_if_absent(cache, name, source, "cc").and_then(|lib| open_library(name, &lib));
    // a fetch or a file error may be passing (offline at first start): try again next time
    if !matches!(r, Err(Error::GrammarFetch { .. } | Error::Io { .. })) {
        loaded.insert(
            path,
            r.as_ref().map(Clone::clone).map_err(ToString::to_string),
        );
    }
    r
}

/// The compiled library's path: `<dir>/<name>-<key>.<ext>`, where the key
/// hashes the source's identity, the runtime's ABI, the OS and the arch. A
/// changed key is a new file; the old one is left.
pub fn library_path(cache: &GrammarCache, name: &str, source: &GrammarSource) -> Result<PathBuf> {
    let identity = match source {
        GrammarSource::Git { rev, .. } => rev.clone(),
        GrammarSource::Local(dir) => {
            let src = local_src(dir);
            let canon = src.canonicalize().map_err(|e| Error::io(&src, e))?;
            let stamp = |f: &str| {
                std::fs::metadata(src.join(f))
                    .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                    .unwrap_or_default()
            };
            format!(
                "{}|{}|{}",
                canon.display(),
                stamp("parser.c"),
                stamp("scanner.c")
            )
        }
    };
    let key = fnv64_hex(
        format!(
            "{identity}|{}|{}|{}",
            tree_sitter::LANGUAGE_VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH
        )
        .as_bytes(),
    );
    Ok(cache.dir.join(format!("{name}-{key}.{EXT}")))
}

/// Builds NAME's library with the compiler CC unless the cache has it;
/// returns its path. Exposed for tests (the compiler is a parameter so a
/// missing one can be simulated).
#[doc(hidden)]
pub fn build_if_absent(
    cache: &GrammarCache,
    name: &str,
    source: &GrammarSource,
    cc: &str,
) -> Result<PathBuf> {
    let lib = library_path(cache, name, source)?;
    if lib.exists() {
        return Ok(lib);
    }
    let src = match source {
        GrammarSource::Local(dir) => local_src(dir),
        GrammarSource::Git { url, rev } => fetch(cache, name, url, rev)?.join("src"),
    };
    std::fs::create_dir_all(&cache.dir).map_err(|e| Error::io(&cache.dir, e))?;
    let tmp = aside(&lib);
    let mut cmd = Command::new(cc);
    cmd.args(["-shared", "-fPIC", "-O2", "-I"])
        .arg(&src)
        .arg(src.join("parser.c"));
    if src.join("scanner.c").exists() {
        cmd.arg(src.join("scanner.c"));
    }
    let out = cmd
        .arg("-o")
        .arg(&tmp)
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NoCompiler {
                hint: compiler_hint().into(),
            },
            _ => Error::GrammarBuild {
                name: name.into(),
                stderr: e.to_string(),
            },
        })?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::GrammarBuild {
            name: name.into(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    std::fs::rename(&tmp, &lib).map_err(|e| Error::io(&lib, e))?;
    Ok(lib)
}

fn compiler_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "xcode-select --install"
    } else {
        "apt install gcc"
    }
}

/// A local grammar directory holds `parser.c` itself, or is a checkout with `src/parser.c`.
fn local_src(dir: &Path) -> PathBuf {
    if dir.join("parser.c").exists() {
        dir.to_path_buf()
    } else {
        dir.join("src")
    }
}

/// A unique temporary name beside PATH.
fn aside(path: &Path) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}-{n}", std::process::id()));
    path.with_file_name(name)
}

/// Git's own transports only: a URL like `ext::…` would run a command.
fn allowed_url(url: &str) -> bool {
    ["https://", "ssh://", "git://", "file://"]
        .iter()
        .any(|p| url.starts_with(p))
}

/// Fetches URL at REV into `<dir>/src/<name>-<rev7>` (once) and returns it.
fn fetch(cache: &GrammarCache, name: &str, url: &str, rev: &str) -> Result<PathBuf> {
    let failed = |why: String| Error::GrammarFetch {
        name: name.into(),
        why,
    };
    if !allowed_url(url) {
        return Err(failed(format!(
            "{url} is not an https, ssh, git or file URL"
        )));
    }
    if rev.len() < 7 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(failed(format!("rev {rev} is not a commit hash")));
    }
    let checkout = cache.dir.join("src").join(format!("{name}-{}", &rev[..7]));
    if checkout.exists() {
        return Ok(checkout);
    }
    let parent = cache.dir.join("src");
    std::fs::create_dir_all(&parent).map_err(|e| Error::io(&parent, e))?;
    let tmp = aside(&checkout);
    let git = |args: &[&str]| -> std::result::Result<String, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    let result = std::fs::create_dir_all(&tmp)
        .map_err(|e| e.to_string())
        .and_then(|_| git(&["init", "-q"]))
        .and_then(|_| git(&["fetch", "--quiet", "--depth", "1", "--no-tags", url, rev]))
        .and_then(|_| git(&["checkout", "--quiet", "FETCH_HEAD"]))
        .and_then(|_| git(&["rev-parse", "HEAD"]))
        .and_then(|head| {
            if head.starts_with(rev) {
                Ok(())
            } else {
                Err(format!("fetched {head}, not {rev}"))
            }
        });
    if let Err(why) = result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(failed(format!(
            "fetching {url} @{} failed: {why}",
            &rev[..7]
        )));
    }
    if std::fs::rename(&tmp, &checkout).is_err() {
        // another process put the same checkout in place first
        let _ = std::fs::remove_dir_all(&tmp);
    }
    Ok(checkout)
}

fn open_library(name: &str, lib: &Path) -> Result<tree_sitter::Language> {
    let fail = |why: String| Error::GrammarLoad {
        name: name.into(),
        why,
    };
    let symbol = format!("tree_sitter_{}", name.replace('-', "_"));
    // SAFETY: the library is a tree-sitter grammar we compiled; its one
    // exported function takes nothing and returns a `TSLanguage` pointer.
    let language = unsafe {
        let library = libloading::Library::new(lib).map_err(|e| fail(e.to_string()))?;
        let f: libloading::Symbol<unsafe extern "C" fn() -> *const ()> = library
            .get(symbol.as_bytes())
            .map_err(|e| fail(format!("{symbol}: {e}")))?;
        let language = tree_sitter::Language::new(tree_sitter_language::LanguageFn::from_raw(*f));
        // the language points into the library: it must outlive every parser
        std::mem::forget(library);
        language
    };
    let abi = language.abi_version();
    let (min, max) = (
        tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION,
        tree_sitter::LANGUAGE_VERSION,
    );
    if !(min..=max).contains(&abi) {
        return Err(Error::GrammarAbi {
            name: name.into(),
            abi,
            min,
            max,
        });
    }
    Ok(language)
}

/// The languages' disk side: the project's layers and loading a grammar.
impl Languages {
    /// The shipped languages, layered with the project's under
    /// `<root>/.sorcerer/languages/`. Every name in EXTRA (`[languages]
    /// names`) must have a layer somewhere.
    pub fn assemble(root: &Path, extra: &[String]) -> Result<Languages> {
        let mut layers = Languages::shipped_layers()?;
        let project = root.join(".sorcerer/languages");
        if let Ok(entries) = std::fs::read_dir(&project) {
            for e in entries.flatten().filter(|e| e.path().is_dir()) {
                let name = e.file_name().to_string_lossy().into_owned();
                layers
                    .entry(name.clone())
                    .or_default()
                    .push(("project", project_layer(&name, &e.path())?));
            }
        }
        Languages::from_layers(layers, extra)
    }

    /// NAME loaded: its grammar fetched, built and loaded through CACHE, and
    /// its queries compiled. Once per name per `Languages`.
    pub fn load(&self, name: &str, cache: &GrammarCache) -> Result<Arc<Loaded>> {
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = loaded.get(name) {
            return Ok(l.clone());
        }
        let language = self
            .get(name)
            .ok_or_else(|| Error::Language {
                name: name.into(),
                why: "no such language".into(),
            })?
            .clone();
        let grammar = load(cache, name, &language.grammar)?;
        let queries = language
            .queries
            .iter()
            .map(|(file, text)| {
                Ok((
                    file.clone(),
                    crate::domain::query::compile(&grammar, file, text)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let l = Arc::new(Loaded {
            language,
            grammar,
            queries,
        });
        loaded.insert(name.to_string(), l.clone());
        Ok(l)
    }
}

fn project_layer(name: &str, dir: &Path) -> Result<Layer> {
    let file = dir.join("language.toml");
    let mut table = match std::fs::read_to_string(&file) {
        Ok(text) => crate::domain::language::parse_toml(name, &text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(Error::io(file, e)),
    };
    // A local grammar's path is relative to this layer's directory.
    if let Some(toml::Value::Table(g)) = table.get_mut("grammar")
        && let Some(toml::Value::String(p)) = g.get("path")
    {
        let abs = dir.join(p).to_string_lossy().into_owned();
        g.insert("path".into(), toml::Value::String(abs));
    }
    let mut queries = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("queries")) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "scm") {
                let text = std::fs::read_to_string(&p).map_err(|err| Error::io(&p, err))?;
                queries.insert(e.file_name().to_string_lossy().into_owned(), text);
            }
        }
    }
    Ok(Layer { table, queries })
}
