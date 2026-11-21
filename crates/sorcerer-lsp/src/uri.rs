//! Paths and the URIs a client names documents by. Only `file:` URIs are files; an editor's
//! other documents (`untitled:`, `fugitive://`) have no path and are ignored.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

/// The file a URI names, or `None` when it is not a `file:` URI.
///
/// Example: `file:///tmp/a%20b/c%23d/%C3%A9.txt` is `/tmp/a b/c#d/é.txt`; `untitled:Untitled-1`
/// is `None`.
pub fn path_of(uri: &Uri) -> Option<PathBuf> {
    let url = url::Url::parse(uri.as_str()).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

/// The `file:` URI of a path, with spaces, `#` and non-ASCII letters escaped. A relative path
/// is taken from the working directory first.
pub fn uri_of(path: &Path) -> Uri {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let url = url::Url::from_file_path(&absolute)
        .unwrap_or_else(|()| panic!("{} is not a path a URI can name", absolute.display()));
    Uri::from_str(url.as_str()).expect("a file URL is a URI")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip() {
        let path = Path::new("/tmp/a b/c#d/é.txt");
        let uri = uri_of(path);
        assert_eq!(uri.as_str(), "file:///tmp/a%20b/c%23d/%C3%A9.txt");
        assert_eq!(path_of(&uri).as_deref(), Some(path));
    }

    #[test]
    fn a_non_file_uri_has_no_path() {
        let untitled = Uri::from_str("untitled:Untitled-1").unwrap();
        assert_eq!(path_of(&untitled), None);
        let fugitive = Uri::from_str("fugitive:///repo/.git//0/a.txt").unwrap();
        assert_eq!(path_of(&fugitive), None);
    }
}
