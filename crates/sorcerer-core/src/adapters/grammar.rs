//! Grammars at run time. No grammar is compiled into sorcerer: a language file names where its
//! grammar's C sources are, `{ git, rev }` or a local directory. The first time the grammar is
//! needed, the sources are fetched into the user's cache, compiled with the first of `cc`, `gcc`
//! or `clang` on PATH into a shared library, and the library is loaded into the running
//! process.
//!
//! Layout under the cache directory:
//!
//! ```text
//! src/<name>-<rev7>/          a git checkout at the pinned revision
//! <name>-<key>.<dylib|so|dll> the compiled grammar
//! ```
//!
//! Every write goes to a temporary name first and is renamed into place, so a reader never sees
//! half a checkout or half a library, and two processes building the same grammar at once do no
//! harm: the second rename simply replaces the first, or is dropped.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::adapters::env::SystemEnv;
use crate::adapters::platform::compiler::{
    CCompiler, c_compiler, compiler_hint, shared_library_extension,
};
use crate::adapters::platform::dirs::user_cache_dir;
use crate::adapters::platform::paths::canonical;
use crate::domain::fnv::fnv64_hex;
use crate::domain::language::{GrammarSource, Languages, Layer, Loaded, parse_toml};
use crate::domain::query::compile;
use crate::ports::env::Env;
use crate::{Error, Result};

/// Where grammars are fetched and built: one per user, shared by all their projects.
#[derive(Debug, Clone)]
pub struct GrammarCache {
    pub dir: PathBuf,
}

impl GrammarCache {
    /// The user's cache: `sorcerer/grammars` in the user's cache directory
    /// (`~/.cache/sorcerer/grammars` on Linux and macOS, `%LOCALAPPDATA%\sorcerer\grammars` on
    /// Windows), else `.cache/sorcerer/grammars` in the current directory.
    pub fn user(env: &dyn Env) -> GrammarCache {
        let base = user_cache_dir(env).unwrap_or_else(|| PathBuf::from(".cache"));
        GrammarCache {
            dir: base.join("sorcerer").join("grammars"),
        }
    }

    /// A cache in any directory, for tests.
    pub fn at(dir: PathBuf) -> GrammarCache {
        GrammarCache { dir }
    }
}

/// What this process got when it last tried to load each library: the grammar, or the error
/// text.
type TriedLibraries = HashMap<PathBuf, std::result::Result<tree_sitter::Language, String>>;

/// Loads a language's grammar, fetching and building it first when the cache has no library
/// for it.
///
/// Each library is tried once per process, and a failure is remembered too, so a missing
/// compiler is reported once rather than for every file. A failed fetch or a file error is not
/// remembered: it may pass (offline when sorcerer first starts), so the next call tries again.
pub fn load(
    cache: &GrammarCache,
    name: &str,
    source: &GrammarSource,
) -> Result<tree_sitter::Language> {
    static TRIED: OnceLock<Mutex<TriedLibraries>> = OnceLock::new();
    let path = library_path(cache, name, source)?;
    let mut tried = TRIED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(earlier) = tried.get(&path) {
        return earlier.clone().map_err(|why| Error::GrammarLoad {
            name: name.into(),
            why,
        });
    }
    let compiler = c_compiler(&SystemEnv);
    let result = build_if_absent(cache, name, source, compiler.as_ref())
        .and_then(|lib| open_library(name, &lib));
    if !may_pass(&result) {
        tried.insert(
            path,
            result
                .as_ref()
                .map(Clone::clone)
                .map_err(ToString::to_string),
        );
    }
    result
}

fn may_pass(result: &Result<tree_sitter::Language>) -> bool {
    matches!(result, Err(Error::GrammarFetch { .. } | Error::Io { .. }))
}

/// Where the compiled library for these sources is: `<cache>/<name>-<key>.<dylib|so|dll>`.
///
/// The key is a hash of the sources' identity (the git revision, or a local directory's path
/// and its C files' sizes and times), tree-sitter's ABI version, the operating system and the
/// processor. Anything that would need a rebuild changes the key, so the new library gets a new
/// file; the old file is left in place.
pub fn library_path(cache: &GrammarCache, name: &str, source: &GrammarSource) -> Result<PathBuf> {
    let identity = sources_identity(source)?;
    let key = fnv64_hex(
        format!(
            "{identity}|{}|{}|{}",
            tree_sitter::LANGUAGE_VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH
        )
        .as_bytes(),
    );
    Ok(cache
        .dir
        .join(format!("{name}-{key}.{}", shared_library_extension())))
}

/// What names a version of the sources: a git revision, or for a local directory its full path
/// plus the size and time of `parser.c` and `scanner.c`, so editing either means a rebuild.
fn sources_identity(source: &GrammarSource) -> Result<String> {
    match source {
        GrammarSource::Git { rev, .. } => Ok(rev.clone()),
        GrammarSource::Local(dir) => {
            let src = local_src(dir);
            let canonical = canonical(&src).map_err(|e| Error::io(&src, e))?;
            let stamp = |file: &str| {
                std::fs::metadata(src.join(file))
                    .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                    .unwrap_or_default()
            };
            Ok(format!(
                "{}|{}|{}",
                canonical.display(),
                stamp("parser.c"),
                stamp("scanner.c")
            ))
        }
    }
}

/// Builds the grammar's library with the C compiler unless the cache already has it, and
/// returns the library's path. With no compiler (`None`) and no library yet it is
/// `NoCompiler`, with how to install one. Public for tests, which pass `None` to see that
/// error.
#[doc(hidden)]
pub fn build_if_absent(
    cache: &GrammarCache,
    name: &str,
    source: &GrammarSource,
    compiler: Option<&CCompiler>,
) -> Result<PathBuf> {
    let lib = library_path(cache, name, source)?;
    if lib.exists() {
        return Ok(lib);
    }
    let src = match source {
        GrammarSource::Local(dir) => local_src(dir),
        GrammarSource::Git { url, rev } => fetch(cache, name, url, rev)?.join("src"),
    };
    let compiler = compiler.ok_or_else(|| Error::NoCompiler {
        hint: compiler_hint().into(),
    })?;
    std::fs::create_dir_all(&cache.dir).map_err(|e| Error::io(&cache.dir, e))?;
    let temporary = aside(&lib);
    compile_library(name, compiler, &src, &temporary)?;
    std::fs::rename(&temporary, &lib).map_err(|e| Error::io(&lib, e))?;
    Ok(lib)
}

/// Compiles `parser.c` (and `scanner.c`, when there is one) into a shared library at `out`. A
/// missing compiler is `NoCompiler` with how to install one; a failed compile is
/// `GrammarBuild` with the compiler's output, and leaves no file behind.
fn compile_library(name: &str, compiler: &CCompiler, src: &Path, out: &Path) -> Result<()> {
    let mut command = Command::new(&compiler.program);
    command
        .args(compiler.flags)
        .arg("-I")
        .arg(src)
        .arg(src.join("parser.c"));
    if src.join("scanner.c").exists() {
        command.arg(src.join("scanner.c"));
    }
    let output = command
        .arg("-o")
        .arg(out)
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
    if !output.status.success() {
        let _ = std::fs::remove_file(out);
        return Err(Error::GrammarBuild {
            name: name.into(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// A local grammar directory holds `parser.c` itself, or is a checkout with `src/parser.c`.
fn local_src(dir: &Path) -> PathBuf {
    if dir.join("parser.c").exists() {
        dir.to_path_buf()
    } else {
        dir.join("src")
    }
}

/// A temporary name beside the path, unique to this process and this call:
/// `rust-1a2b.dylib.tmp-4242-0`.
fn aside(path: &Path) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}-{n}", std::process::id()));
    path.with_file_name(name)
}

/// Fetches the grammar's repository at one commit into `<cache>/src/<name>-<first 7 of rev>`,
/// once, and returns that directory.
///
/// Only a commit hash (at least 7 hex digits) is accepted, and the fetched commit is checked
/// against it, so the language file pins exactly what gets compiled.
fn fetch(cache: &GrammarCache, name: &str, url: &str, rev: &str) -> Result<PathBuf> {
    let failed = |why: String| Error::GrammarFetch {
        name: name.into(),
        why,
    };
    check_fetchable(url, rev).map_err(failed)?;
    let checkout = cache.dir.join("src").join(format!("{name}-{}", &rev[..7]));
    if checkout.exists() {
        return Ok(checkout);
    }
    let parent = cache.dir.join("src");
    std::fs::create_dir_all(&parent).map_err(|e| Error::io(&parent, e))?;
    let temporary = aside(&checkout);
    if let Err(why) = fetch_commit_into(&temporary, url, rev) {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(failed(format!(
            "fetching {url} @{} failed: {why}",
            &rev[..7]
        )));
    }
    if std::fs::rename(&temporary, &checkout).is_err() {
        // another process put the same checkout in place first
        let _ = std::fs::remove_dir_all(&temporary);
    }
    Ok(checkout)
}

/// Refuses a URL git would treat as a command (`ext::…`) and a revision that is not a commit
/// hash of at least 7 hex digits.
fn check_fetchable(url: &str, rev: &str) -> std::result::Result<(), String> {
    if !allowed_url(url) {
        return Err(format!("{url} is not an https, ssh, git or file URL"));
    }
    if rev.len() < 7 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("rev {rev} is not a commit hash"));
    }
    Ok(())
}

/// Git's own transports only: a URL like `ext::…` would run a command.
fn allowed_url(url: &str) -> bool {
    ["https://", "ssh://", "git://", "file://"]
        .iter()
        .any(|p| url.starts_with(p))
}

/// A fresh repository in `dir` with only that commit fetched and checked out (no history),
/// failing when what arrived is not the commit asked for.
fn fetch_commit_into(dir: &Path, url: &str, rev: &str) -> std::result::Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    git_in(dir, &["init", "-q"])?;
    git_in(
        dir,
        &["fetch", "--quiet", "--depth", "1", "--no-tags", url, rev],
    )?;
    git_in(dir, &["checkout", "--quiet", "FETCH_HEAD"])?;
    let head = git_in(dir, &["rev-parse", "HEAD"])?;
    if head.starts_with(rev) {
        Ok(())
    } else {
        Err(format!("fetched {head}, not {rev}"))
    }
}

/// Runs git in a directory: its trimmed output, or what it printed to stderr.
fn git_in(dir: &Path, args: &[&str]) -> std::result::Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Loads a compiled grammar into the process and checks that this sorcerer's tree-sitter can
/// read it.
fn open_library(name: &str, lib: &Path) -> Result<tree_sitter::Language> {
    let language = load_language_symbol(name, lib)?;
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

/// Calls the library's `tree_sitter_<name>` function (a `-` in the name becomes `_`), which
/// hands back the grammar. The library stays loaded for the rest of the process.
fn load_language_symbol(name: &str, lib: &Path) -> Result<tree_sitter::Language> {
    let fail = |why: String| Error::GrammarLoad {
        name: name.into(),
        why,
    };
    let symbol = format!("tree_sitter_{}", name.replace('-', "_"));
    // SAFETY: the library is a tree-sitter grammar we compiled; its one
    // exported function takes nothing and returns a `TSLanguage` pointer.
    unsafe {
        let library = libloading::Library::new(lib).map_err(|e| fail(e.to_string()))?;
        let f: libloading::Symbol<unsafe extern "C" fn() -> *const ()> = library
            .get(symbol.as_bytes())
            .map_err(|e| fail(format!("{symbol}: {e}")))?;
        let language = tree_sitter::Language::new(tree_sitter_language::LanguageFn::from_raw(*f));
        // the language points into the library: it must outlive every parser
        std::mem::forget(library);
        Ok(language)
    }
}

/// The languages' disk side: the project's layers and loading a grammar.
impl Languages {
    /// The languages sorcerer ships, with the project's own under
    /// `<root>/.sorcerer/languages/<name>/` laid on top. Every name in `[languages] names` must
    /// have a language file somewhere.
    pub fn assemble(root: &Path, named_in_config: &[String]) -> Result<Languages> {
        let mut layers = Languages::shipped_layers()?;
        for (name, layer) in project_layers(root)? {
            layers.entry(name).or_default().push(("project", layer));
        }
        Languages::from_layers(layers, named_in_config)
    }

    /// A language ready to parse: its grammar fetched, built and loaded through the cache, and
    /// its query files compiled. Done once per language per `Languages`; later calls get the
    /// same one.
    pub fn load(&self, name: &str, cache: &GrammarCache) -> Result<Arc<Loaded>> {
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(earlier) = loaded.get(name) {
            return Ok(earlier.clone());
        }
        let language = self
            .get(name)
            .ok_or_else(|| Error::Language {
                name: name.into(),
                why: "no such language".into(),
            })?
            .clone();
        let grammar = load(cache, name, &language.grammar)?;
        let queries = compile_queries(&grammar, &language.queries)?;
        let ready = Arc::new(Loaded {
            language,
            grammar,
            queries,
        });
        loaded.insert(name.to_string(), ready.clone());
        Ok(ready)
    }
}

/// Each query file compiled for the grammar, in file-name order; the first that fails stops it.
fn compile_queries(
    grammar: &tree_sitter::Language,
    queries: &BTreeMap<String, String>,
) -> Result<Vec<(String, tree_sitter::Query)>> {
    queries
        .iter()
        .map(|(file, text)| Ok((file.clone(), compile(grammar, file, text)?)))
        .collect()
}

/// The project's language layers, one per directory under `.sorcerer/languages/`, by
/// language name. A project without that directory has none.
fn project_layers(root: &Path) -> Result<Vec<(String, Layer)>> {
    let Ok(entries) = std::fs::read_dir(root.join(".sorcerer/languages")) else {
        return Ok(Vec::new());
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let layer = project_layer(&name, &entry.path())?;
            Ok((name, layer))
        })
        .collect()
}

/// One project layer: its `language.toml` (either file may be missing) and its query files.
fn project_layer(name: &str, dir: &Path) -> Result<Layer> {
    let table = with_absolute_grammar_path(project_language_table(name, dir)?, dir);
    Ok(Layer {
        table,
        queries: project_queries(dir)?,
    })
}

fn project_language_table(name: &str, dir: &Path) -> Result<toml::Table> {
    let file = dir.join("language.toml");
    match std::fs::read_to_string(&file) {
        Ok(text) => parse_toml(name, &text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(Error::io(file, e)),
    }
}

/// A local grammar's `path` is written relative to its layer's directory; this makes it
/// absolute, so it still means the same place once the layers are laid together.
fn with_absolute_grammar_path(mut table: toml::Table, dir: &Path) -> toml::Table {
    if let Some(toml::Value::Table(grammar)) = table.get_mut("grammar")
        && let Some(toml::Value::String(relative)) = grammar.get("path")
    {
        let absolute = dir.join(relative).to_string_lossy().into_owned();
        grammar.insert("path".into(), toml::Value::String(absolute));
    }
    table
}

/// The `.scm` files in the layer's `queries/` directory, by file name.
fn project_queries(dir: &Path) -> Result<BTreeMap<String, String>> {
    let Ok(entries) = std::fs::read_dir(dir.join("queries")) else {
        return Ok(BTreeMap::new());
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|x| x == "scm"))
        .map(|entry| {
            let path = entry.path();
            let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            Ok((entry.file_name().to_string_lossy().into_owned(), text))
        })
        .collect()
}
