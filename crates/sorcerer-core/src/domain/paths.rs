//! A project-relative path as text: the same text on every platform.

use std::path::{MAIN_SEPARATOR_STR, Path};

/// The path with `/` between its parts, whatever the platform writes: a prompt, the store,
/// a run log and a recording key then read the same on Windows as on Linux and macOS.
///
/// Example: the path made by `Path::new("src").join("orders.py")` is `src\orders.py` on
/// Windows and `src/orders.py` elsewhere; this gives `src/orders.py` on both. On Linux and
/// macOS it is the path's own text, unchanged.
pub fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace(MAIN_SEPARATOR_STR, "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_slashes_joins_components_with_a_slash() {
        let path = Path::new("src").join("shop").join("orders.py");
        assert_eq!(forward_slashes(&path), "src/shop/orders.py");
        assert_eq!(forward_slashes(Path::new("orders.py")), "orders.py");
    }
}
