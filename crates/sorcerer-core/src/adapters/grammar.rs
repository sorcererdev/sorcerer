//! Grammars at run time. No grammar is compiled into sorcerer: a language file names where its
//! grammar's C sources are, `{ git, rev }` or a local directory. The first time the grammar is
//! needed, its shared library is put in the user's cache and loaded into the running process.
//! The library gets into the cache in one of two ways:
//!
//! 1. **Downloaded.** For a grammar a shipped language names (`{ git, rev }`, not the
//!    project's own), the library prebuilt for this platform is downloaded from scry's GitHub
//!    release for this version ([`RELEASES`]) and kept only when its SHA-256 matches the
//!    release's `<file>.sha256`. The file name is the cache's own name for it, which already
//!    says the revision, tree-sitter's ABI, the operating system and the processor.
//! 2. **Built.** Otherwise, or when the download fails for any reason (offline, no release for
//!    this version, a sum that does not match), the sources are fetched into the cache and
//!    compiled with the first of `cc`, `gcc` or `clang` on PATH.
//!
//! Layout under the cache directory:
//!
//! ```text
//! src/<name>-<rev7>/          a git checkout at the pinned revision (the grammar in its
//!                             `src`, or in `<dir>/src` when the language names a `dir`)
//! <name>-<key>.<dylib|so|dll> the compiled grammar
//! ```
//!
//! Every write goes to a temporary name first and is renamed into place, so a reader never sees
//! half a checkout or half a library, and two processes building the same grammar at once do no
//! harm: the second rename simply replaces the first, or is dropped.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::adapters::env::SystemEnv;
use crate::adapters::platform::compiler::{
    CCompiler, c_compiler, compiler_hint, shared_library_extension,
};
use crate::adapters::platform::dirs::{NO_CACHE_DIR, user_cache_dir};
use crate::adapters::platform::paths::{ascii_scratch_parent, canonical, is_ascii_path};
use crate::domain::fnv::fnv64_hex;
use crate::domain::language::{
    GrammarObtained, GrammarOrigin, GrammarSource, Languages, Layer, Loaded, parse_toml,
};
use crate::domain::query::compile;
use crate::ports::env::Env;
use crate::{Error, Result};

/// Scry's GitHub release for this version, where the prebuilt grammar libraries are:
/// `https://github.com/sorcererdev/sorcerer/releases/download/v0.1.0` for scry 0.1.0. A build
/// whose version has no release gets a 404 there and compiles instead.
pub const RELEASES: &str = concat!(
    "https://github.com/sorcererdev/sorcerer/releases/download/v",
    env!("CARGO_PKG_VERSION")
);

/// Where grammars are downloaded, fetched and built: one per user, shared by all their
/// projects.
#[derive(Debug, Clone)]
pub struct GrammarCache {
    /// The directory, or `None` when the environment names no user cache directory (see
    /// [`GrammarCache::dir`]).
    dir: Option<PathBuf>,
    /// Where prebuilt libraries are downloaded from ([`RELEASES`] for the user's cache), or
    /// `None` to always compile.
    pub releases: Option<String>,
}

impl GrammarCache {
    /// The user's cache: `sorcerer/grammars` in the user's cache directory
    /// (`~/.cache/sorcerer/grammars` on Linux and macOS, `%LOCALAPPDATA%\sorcerer\grammars` on
    /// Windows). When the environment names no such directory (no `HOME`, say), every load
    /// fails with `NoCacheDir`, which names the variable to set. There is deliberately no
    /// fallback in the current directory: a repository could carry a library there.
    pub fn user(env: &dyn Env) -> GrammarCache {
        GrammarCache {
            dir: user_cache_dir(env).map(|base| base.join("sorcerer").join("grammars")),
            releases: Some(RELEASES.to_string()),
        }
    }

    /// A cache in any directory, for tests. It never downloads: every grammar is compiled.
    pub fn at(dir: PathBuf) -> GrammarCache {
        GrammarCache {
            dir: Some(dir),
            releases: None,
        }
    }

    /// The cache's directory, or `NoCacheDir` when there is none: `there is no place for the
    /// grammar cache: neither XDG_CACHE_HOME nor HOME is set`.
    pub fn dir(&self) -> Result<&Path> {
        self.dir.as_deref().ok_or_else(|| Error::NoCacheDir {
            why: NO_CACHE_DIR.into(),
        })
    }

    /// The same cache, downloading prebuilt libraries from `url` instead (a test's local
    /// server, such as `http://127.0.0.1:4321`).
    pub fn with_releases(self, url: &str) -> GrammarCache {
        GrammarCache {
            releases: Some(url.trim_end_matches('/').to_string()),
            ..self
        }
    }
}

/// What this process got when it last tried to load each library: the grammar, or the error
/// text.
type TriedLibraries = HashMap<PathBuf, std::result::Result<tree_sitter::Language, String>>;

/// Loads a language's grammar, putting its library in the cache first when it is not there,
/// and says how the library got there.
///
/// A shipped grammar (`origin` is `Shipped`) is downloaded prebuilt from the cache's release
/// when it can be; anything else, or a failed download, is fetched and compiled with the first
/// C compiler on PATH. Each library is tried once per process, and a failure is remembered
/// too, so a missing compiler is reported once rather than for every file. A later call for a
/// library this process already loaded says `Cached`. A failed fetch or a file error is not
/// remembered: it may pass (offline when sorcerer first starts), so the next call tries again.
pub fn load(
    cache: &GrammarCache,
    name: &str,
    source: &GrammarSource,
    origin: GrammarOrigin,
) -> Result<(tree_sitter::Language, GrammarObtained)> {
    load_with_compiler(cache, name, source, origin, c_compiler(&SystemEnv).as_ref())
}

/// `load` with the C compiler given rather than looked up on PATH. Public for tests, which
/// pass `None` to see what a machine with no compiler gets.
#[doc(hidden)]
pub fn load_with_compiler(
    cache: &GrammarCache,
    name: &str,
    source: &GrammarSource,
    origin: GrammarOrigin,
    compiler: Option<&CCompiler>,
) -> Result<(tree_sitter::Language, GrammarObtained)> {
    static TRIED: OnceLock<Mutex<TriedLibraries>> = OnceLock::new();
    let path = library_path(cache, name, source)?;
    sweep_once_per_process(cache);
    let mut tried = TRIED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(earlier) = tried.get(&path) {
        return earlier
            .clone()
            .map(|grammar| (grammar, GrammarObtained::Cached))
            .map_err(|why| Error::GrammarLoad {
                name: name.into(),
                why,
            });
    }
    let grammar = Grammar {
        cache,
        name,
        source,
        compiler,
    };
    let result = grammar.put_in_cache_and_open(origin);
    if !may_pass(&result) {
        tried.insert(
            path,
            result
                .as_ref()
                .map(|(grammar, _)| grammar.clone())
                .map_err(ToString::to_string),
        );
    }
    result
}

fn may_pass<T>(result: &Result<T>) -> bool {
    matches!(result, Err(Error::GrammarFetch { .. } | Error::Io { .. }))
}

/// One grammar being put in the cache: where, which, from what sources, and the C compiler
/// to build it with, if there is one.
struct Grammar<'a> {
    cache: &'a GrammarCache,
    name: &'a str,
    source: &'a GrammarSource,
    compiler: Option<&'a CCompiler>,
}

impl Grammar<'_> {
    /// Puts the grammar's library in the cache and opens it. A downloaded library that will
    /// not open (built for a newer system than this one, or simply wrong) is deleted and the
    /// grammar compiled here instead.
    ///
    /// On Windows a downloaded library that opens but has the wrong tree-sitter ABI stays
    /// mapped, so it cannot be deleted: the compile then finds it in place and the ABI error
    /// stands. The ABI is part of the file's name, so only a mis-built release can do that.
    fn put_in_cache_and_open(
        &self,
        origin: GrammarOrigin,
    ) -> Result<(tree_sitter::Language, GrammarObtained)> {
        let (lib, obtained) = self.put_in_cache(origin)?;
        match open_library(self.name, &lib) {
            Err(e) if obtained == GrammarObtained::Downloaded => {
                let _ = std::fs::remove_file(&lib);
                let unloadable = DownloadFailed(format!("the downloaded file would not load: {e}"));
                let (lib, built) = self.build_here(Some(unloadable))?;
                Ok((open_library(self.name, &lib)?, built))
            }
            opened => Ok((opened?, obtained)),
        }
    }

    /// Makes sure the cache holds the grammar's library and returns its path, with how it got
    /// there: it was there already, it was downloaded prebuilt (a shipped `{ git, rev }`
    /// grammar only, from a cache that names a release), or it was compiled here.
    fn put_in_cache(&self, origin: GrammarOrigin) -> Result<(PathBuf, GrammarObtained)> {
        let lib = library_path(self.cache, self.name, self.source)?;
        if lib.exists() {
            return Ok((lib, GrammarObtained::Cached));
        }
        let failed_download = match self.release_to_download_from(origin) {
            Some(releases) => match download_prebuilt(releases, &lib) {
                Ok(()) => return Ok((lib, GrammarObtained::Downloaded)),
                Err(failed) => Some(failed),
            },
            None => None,
        };
        self.build_here(failed_download)
    }

    /// The release to ask, when this grammar may be downloaded: only a grammar a shipped
    /// language names at a git revision is on the release. A project's own grammar, an
    /// override and a local directory are always compiled here, and a cache made with
    /// `GrammarCache::at` names no release.
    fn release_to_download_from(&self, origin: GrammarOrigin) -> Option<&str> {
        let on_the_release =
            origin == GrammarOrigin::Shipped && matches!(self.source, GrammarSource::Git { .. });
        self.cache.releases.as_deref().filter(|_| on_the_release)
    }

    /// Fetches and compiles the grammar with this machine's C compiler: the library's path,
    /// and `Built` with the compiler's name. A library that another process put in place
    /// meanwhile is `Cached`.
    ///
    /// With no compiler, after a download that failed, the error says why the download failed
    /// (`NoGrammar`), and the sources are not fetched, since nothing could compile them.
    fn build_here(
        &self,
        failed_download: Option<DownloadFailed>,
    ) -> Result<(PathBuf, GrammarObtained)> {
        let lib = library_path(self.cache, self.name, self.source)?;
        if lib.exists() {
            return Ok((lib, GrammarObtained::Cached));
        }
        if let (None, Some(DownloadFailed(why))) = (self.compiler, failed_download) {
            return Err(Error::NoGrammar {
                name: self.name.into(),
                why,
                hint: compiler_hint().into(),
            });
        }
        let lib = build_if_absent(self.cache, self.name, self.source, self.compiler)?;
        Ok((
            lib,
            GrammarObtained::Built {
                compiler: compiler_name(self.compiler),
            },
        ))
    }
}

/// The compiler's name without its directory or `.exe`: `cc`, `gcc`, `clang`.
fn compiler_name(compiler: Option<&CCompiler>) -> String {
    compiler
        .and_then(|c| c.program.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The largest library accepted from a release: 20 MB (the shipped grammars are under 3 MB).
const LIBRARY_CAP: u64 = 20 * 1024 * 1024;

/// The largest `.sha256` file accepted: one line of a 64-digit sum and a file name.
const SUM_CAP: u64 = 4096;

/// Downloading a prebuilt library did not work, so the caller compiles instead. It says why
/// in a few words, such as `no release for 0.1.0`, for the error a machine with no compiler
/// gets.
struct DownloadFailed(String);

/// Downloads the library prebuilt for this platform from the release and puts it at `lib`,
/// but only when its SHA-256 matches the release's checksum file.
///
/// It makes two requests, each given 10 seconds to connect and 60 to finish: the library,
/// refused past 20 MB, and its `.sha256`. For example, for scry 0.1.0 on an Apple Silicon Mac:
///
/// ```text
/// …/releases/download/v0.1.0/python-1a2b3c4d5e6f7a8b.dylib
/// …/releases/download/v0.1.0/python-1a2b3c4d5e6f7a8b.dylib.sha256   "<64 hex digits>  python-….dylib"
/// ```
///
/// The bytes are checked before anything is written, then written beside `lib` and renamed
/// into place, so the cache never holds a file whose sum did not match, nor half a file.
fn download_prebuilt(releases: &str, lib: &Path) -> std::result::Result<(), DownloadFailed> {
    let file = lib
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| DownloadFailed("the cache's file name is not text".into()))?;
    let client = release_client()?;
    let library = get_at_most(&client, &format!("{releases}/{file}"), LIBRARY_CAP)?;
    let sum_file = get_at_most(&client, &format!("{releases}/{file}.sha256"), SUM_CAP)?;
    if !sum_matches(&library, &sum_file) {
        return Err(DownloadFailed(
            "its SHA-256 did not match the release's".into(),
        ));
    }
    put_in_place(&library, lib)
}

/// An HTTP client with the download's time limits. It follows at most 10 redirects, and a
/// release reached over https may not redirect to plain http ([`follows_redirect`]).
fn release_client() -> std::result::Result<reqwest::blocking::Client, DownloadFailed> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let from = attempt.previous().last();
            if attempt.previous().len() >= MOST_REDIRECTS {
                attempt.error("too many redirects")
            } else if from.is_none_or(|from| follows_redirect(from, attempt.url())) {
                attempt.follow()
            } else {
                attempt.error("a redirect from https to plain http is refused")
            }
        }))
        .build()
        .map_err(|e| DownloadFailed(format!("could not start the download: {e}")))
}

/// How many redirects a download follows before giving up.
const MOST_REDIRECTS: usize = 10;

/// Whether a download may follow a redirect from one address to another. A page fetched over
/// https is never sent on to a plain http address, since that would drop the encryption; every
/// other hop is allowed.
///
/// For example `https://github.com/…` to `https://objects.example/…` is followed, and
/// `https://github.com/…` to `http://objects.example/…` is not. A release served over plain
/// http (a test's local server) may redirect anywhere.
fn follows_redirect(from: &reqwest::Url, to: &reqwest::Url) -> bool {
    from.scheme() != "https" || to.scheme() == "https"
}

/// The body of a successful GET, refused when the status is not a success or the body is
/// longer than `cap` bytes.
fn get_at_most(
    client: &reqwest::blocking::Client,
    url: &str,
    cap: u64,
) -> std::result::Result<Vec<u8>, DownloadFailed> {
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| DownloadFailed(request_failed_text(&e)))?;
    let mut body = Vec::new();
    response
        .take(cap + 1)
        .read_to_end(&mut body)
        .map_err(|_| DownloadFailed("the download was cut short".into()))?;
    if body.len() as u64 > cap {
        return Err(DownloadFailed(format!(
            "the file is over {} MB",
            cap / (1024 * 1024)
        )));
    }
    Ok(body)
}

/// Why a request failed, in a few words: `no release for 0.1.0` for a 404 (the release, or
/// this platform's file in it, is not there), `the release answered 503 Service Unavailable`,
/// `the download timed out`, `could not connect (offline?)`.
fn request_failed_text(e: &reqwest::Error) -> String {
    match e.status() {
        Some(reqwest::StatusCode::NOT_FOUND) => {
            format!("no release for {}", env!("CARGO_PKG_VERSION"))
        }
        Some(status) => format!("the release answered {status}"),
        None if e.is_timeout() => "the download timed out".into(),
        None if e.is_connect() => "could not connect (offline?)".into(),
        None => format!("the download failed: {e}"),
    }
}

/// Whether the bytes' SHA-256 is the sum a `.sha256` file starts with (`<hex>  <file name>`;
/// upper or lower case).
fn sum_matches(bytes: &[u8], sum_file: &[u8]) -> bool {
    let text = String::from_utf8_lossy(sum_file);
    let expected = text.split_whitespace().next().unwrap_or_default();
    expected.len() == 64 && expected.eq_ignore_ascii_case(&sha256_hex(bytes))
}

/// The SHA-256 of the bytes as 64 lower-case hex digits.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes the checked bytes beside `lib` and renames them into place. An error leaves no
/// temporary file behind (a process killed between the two steps can leave one; it is never
/// loaded, since it does not have the library's name, and the next process to use the cache
/// sweeps it once it is a day old).
fn put_in_place(bytes: &[u8], lib: &Path) -> std::result::Result<(), DownloadFailed> {
    let unwritable =
        |e: std::io::Error| DownloadFailed(format!("could not write it to the cache: {e}"));
    let dir = lib.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(unwritable)?;
    let temporary = aside(lib);
    let written = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, lib));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(unwritable(e));
    }
    Ok(())
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
        .dir()?
        .join(format!("{name}-{key}.{}", shared_library_extension())))
}

/// What names a version of the sources: a git revision (and the folder holding the grammar,
/// when the repository holds more than one), or for a local directory its full path plus the
/// size and time of `parser.c` and `scanner.c`, so editing either means a rebuild.
fn sources_identity(source: &GrammarSource) -> Result<String> {
    match source {
        GrammarSource::Git { rev, dir: None, .. } => Ok(rev.clone()),
        GrammarSource::Git {
            rev,
            dir: Some(dir),
            ..
        } => Ok(format!("{rev}|{dir}")),
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
    let sources = match source {
        GrammarSource::Local(dir) => GrammarSources::local(dir),
        GrammarSource::Git { url, rev, dir } => {
            GrammarSources::in_checkout(fetch(cache, name, url, rev)?, dir.as_deref())
        }
    };
    let compiler = compiler.ok_or_else(|| Error::NoCompiler {
        hint: compiler_hint().into(),
    })?;
    let dir = cache.dir()?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let temporary = aside(&lib);
    compile_where_the_compiler_can_read_paths(name, compiler, &sources, &temporary)?;
    std::fs::rename(&temporary, &lib).map_err(|e| Error::io(&lib, e))?;
    Ok(lib)
}

/// Where a grammar's C files are: `src`, the folder holding `parser.c`, inside `root`, the
/// folder its files may reach with `#include "../…"`. For most grammars the two are the same
/// checkout's `src` and the checkout; for a repository holding two grammars, `tsx/src` inside
/// the checkout, whose scanner includes `../../common/scanner.h`.
struct GrammarSources {
    root: PathBuf,
    src: PathBuf,
}

impl GrammarSources {
    /// A local grammar directory: it holds `parser.c` itself, or is a checkout with
    /// `src/parser.c`.
    fn local(dir: &Path) -> GrammarSources {
        GrammarSources {
            root: dir.to_path_buf(),
            src: local_src(dir),
        }
    }

    /// A fetched checkout, the grammar in its `src` or in `<dir>/src`.
    fn in_checkout(checkout: PathBuf, dir: Option<&str>) -> GrammarSources {
        let grammar = dir.map_or_else(|| checkout.clone(), |dir| checkout.join(dir));
        GrammarSources {
            src: grammar.join("src"),
            root: checkout,
        }
    }
}

/// Compiles the library at `out`, in place when the sources and `out` have plain ASCII paths.
/// When either has other characters, a Windows user whose name is outside Windows-1252,
/// MinGW's linker is handed the path with `?` for each letter and cannot open it: then it
/// compiles a copy of the sources in an ASCII-only temporary folder and moves the library to
/// `out`.
fn compile_where_the_compiler_can_read_paths(
    name: &str,
    compiler: &CCompiler,
    sources: &GrammarSources,
    out: &Path,
) -> Result<()> {
    if is_ascii_path(&sources.src) && is_ascii_path(out) {
        return compile_library(name, compiler, &sources.src, out);
    }
    let Some(parent) = ascii_scratch_parent() else {
        return compile_library(name, compiler, &sources.src, out);
    };
    let scratch = parent.join(format!(
        "sorcerer-grammar-{}-{}",
        std::process::id(),
        fnv64_hex(out.to_string_lossy().as_bytes())
    ));
    let result = compile_in_scratch(name, compiler, sources, out, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// Copies the sources' whole root into `scratch` (so an `#include "../…"` still finds its
/// file), compiles there and copies the library to `out`.
fn compile_in_scratch(
    name: &str,
    compiler: &CCompiler,
    sources: &GrammarSources,
    out: &Path,
    scratch: &Path,
) -> Result<()> {
    let _ = std::fs::remove_dir_all(scratch);
    let copied = scratch.join("root");
    copy_tree(&sources.root, &copied)?;
    let src_inside = sources
        .src
        .strip_prefix(&sources.root)
        .unwrap_or(Path::new(""));
    let built = scratch.join("grammar.lib");
    compile_library(name, compiler, &copied.join(src_inside), &built)?;
    std::fs::copy(&built, out).map_err(|e| Error::io(out, e))?;
    Ok(())
}

/// Copies a folder and everything in it but git's own `.git` (the headers a grammar includes
/// sit beside its C files, or a folder or two above them).
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| Error::io(to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| Error::io(from, e))? {
        let entry = entry.map_err(|e| Error::io(from, e))?;
        if entry.file_name() == ".git" {
            continue;
        }
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| Error::io(from, e))?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| Error::io(&target, e))?;
        }
    }
    Ok(())
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

/// How old a leftover temporary file must be before the sweep removes it. A younger one may be
/// another process's download in progress.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// The first time this process uses a cache, removes the temporary files that killed processes
/// left in it ([`remove_stale_temporary_files`]). Later calls do nothing.
fn sweep_once_per_process(cache: &GrammarCache) {
    static SWEPT: Once = Once::new();
    if let Ok(dir) = cache.dir() {
        SWEPT.call_once(|| remove_stale_temporary_files(dir, SystemTime::now()));
    }
}

/// Removes the files in `dir` whose names hold `.tmp-` (the names [`aside`] makes) and that were
/// last changed more than a day before `now`. A file without `.tmp-` in its name is never
/// touched, and neither is a younger one. Any error is ignored: a sweep must never fail a load.
///
/// For example `python-1a2b.dylib.tmp-4242-0`, left two days ago, goes;
/// `python-1a2b.dylib` stays.
fn remove_stale_temporary_files(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_temporary = entry.file_name().to_string_lossy().contains(".tmp-");
        let age = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok());
        if is_temporary && age.is_some_and(|age| age > STALE_AFTER) {
            let _ = std::fs::remove_file(entry.path());
        }
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

/// Fetches a `{ git, rev }` grammar's repository into the cache's `src/` without compiling it,
/// and returns the checkout, as a build would fetch it (the same checks on the URL and the
/// revision). `scripts/test-grammars` fetches the shipped grammars for the tests this way.
#[doc(hidden)]
pub fn fetch_checkout(cache: &GrammarCache, name: &str, url: &str, rev: &str) -> Result<PathBuf> {
    fetch(cache, name, url, rev)
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
    let checkout = cache
        .dir()?
        .join("src")
        .join(format!("{name}-{}", &rev[..7]));
    if checkout.exists() {
        return Ok(checkout);
    }
    let parent = cache.dir()?.join("src");
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
    // SAFETY: the library is a tree-sitter grammar this machine compiled, or one downloaded
    // from scry's release whose SHA-256 matched (or a file already in the user's cache under
    // its name); its one exported function takes nothing and returns a `TSLanguage` pointer.
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

    /// A language ready to parse: its grammar put in the cache (downloaded or built) and
    /// loaded, and its query files compiled. Done once per language per `Languages`; later calls get the
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
        let (grammar, obtained) = load(cache, name, &language.grammar, language.grammar_origin())?;
        let queries = compile_queries(&grammar, &language.queries)?;
        let ready = Arc::new(Loaded {
            language,
            grammar,
            obtained,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> reqwest::Url {
        reqwest::Url::parse(text).unwrap()
    }

    #[test]
    fn https_may_redirect_to_https() {
        assert!(follows_redirect(
            &url("https://a.example/x"),
            &url("https://b.example/y")
        ));
    }

    #[test]
    fn https_may_not_redirect_to_http() {
        assert!(!follows_redirect(
            &url("https://a.example/x"),
            &url("http://b.example/y")
        ));
    }

    #[test]
    fn http_may_redirect_to_http() {
        assert!(follows_redirect(
            &url("http://a.example/x"),
            &url("http://b.example/y")
        ));
    }

    fn make_file(dir: &Path, name: &str, age: Duration) -> PathBuf {
        let path = dir.join(name);
        let file = std::fs::File::create(&path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
        path
    }

    #[test]
    fn the_sweep_removes_only_temporary_files_older_than_a_day() {
        let d = tempfile::TempDir::new().unwrap();
        let two_days = Duration::from_secs(2 * 24 * 60 * 60);
        let stale = make_file(d.path(), "py-1.so.tmp-4242-0", two_days);
        let fresh = make_file(d.path(), "py-1.so.tmp-4243-0", Duration::from_secs(60));
        let library = make_file(d.path(), "py-1.so", two_days);

        remove_stale_temporary_files(d.path(), SystemTime::now());

        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(library.exists());
    }

    #[test]
    fn the_sweep_of_a_missing_directory_is_quiet() {
        let d = tempfile::TempDir::new().unwrap();
        remove_stale_temporary_files(&d.path().join("absent"), SystemTime::now());
    }
}
