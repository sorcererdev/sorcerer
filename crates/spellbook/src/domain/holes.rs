//! Holes: the places a generation leaves for someone to fill, one hole per name with every place
//! it appears.

use serde::{Deserialize, Serialize};

/// How a hole is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoleOrigin {
    /// From the inputs, by `[holes.X]`: nothing left to do.
    Derived,
    /// By a person, an agent or conjure: a placeholder stands there until then.
    Placeholder,
}

/// One place a hole appears: a file (relative to the root, with `/`), a line from 1, and the
/// code written there (the placeholder, or the derived value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolePlace {
    pub file: String,
    pub line: u32,
    pub text: String,
    /// The signs beside the code on its line: the last non-blank character before it and the
    /// first after it (`\n` for the line's edge). Tells a short placeholder from the same text
    /// elsewhere. Empty in records written before it existed.
    #[serde(default)]
    pub edges: String,
    /// The rest of the placeholder's line, as the pieces around the holes on it, spaces left
    /// out (`async fn todo_method(&self) -> Result<(), E>;` with its two holes is
    /// `["asyncfn", "(&self)->Result<", ",E>;"]`). Tells the hole's own line from another line
    /// that happens to hold the same short placeholder. Empty in records written before it
    /// existed.
    #[serde(default)]
    pub frame: Vec<String>,
}

/// A hole of one generation, with every place it appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hole {
    pub name: String,
    pub kind: String,
    pub instruction: String,
    pub source: HoleOrigin,
    pub places: Vec<HolePlace>,
}

/// A hole as one rendered template holds it: where it landed in that file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoleAt {
    pub name: String,
    pub kind: String,
    pub instruction: String,
    pub source: HoleOrigin,
    pub line: u32,
    /// The code written in the hole's place.
    pub text: String,
    /// See [`HolePlace::edges`].
    pub edges: String,
    /// See [`HolePlace::frame`].
    pub frame: Vec<String>,
}

/// The holes of a generation, gathered file by file in the order the files are written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HoleBook {
    holes: Vec<Hole>,
}

impl HoleBook {
    /// A book holding these holes already, to add more files' holes to.
    pub fn of(holes: Vec<Hole>) -> HoleBook {
        HoleBook { holes }
    }

    /// This book with one file's holes added: a name seen before gains a place, a new name is a
    /// new hole. One name written with two kinds or two instructions is an error.
    pub fn with_file(mut self, file: &str, found: &[HoleAt]) -> Result<HoleBook, String> {
        for hole in found {
            let place = HolePlace {
                file: file.to_string(),
                line: hole.line,
                text: hole.text.clone(),
                edges: hole.edges.clone(),
                frame: hole.frame.clone(),
            };
            match self.holes.iter_mut().find(|known| known.name == hole.name) {
                Some(known)
                    if (&known.kind, &known.instruction) != (&hole.kind, &hole.instruction) =>
                {
                    return Err(format!(
                        "the hole {} has another kind or instruction in {file} than in {}",
                        hole.name, known.places[0].file
                    ));
                }
                Some(known) => known.places.push(place),
                None => self.holes.push(Hole {
                    name: hole.name.clone(),
                    kind: hole.kind.clone(),
                    instruction: hole.instruction.clone(),
                    source: hole.source,
                    places: vec![place],
                }),
            }
        }
        Ok(self)
    }

    /// The holes, in the order they first appear.
    pub fn holes(self) -> Vec<Hole> {
        self.holes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(name: &str, kind: &str, line: u32) -> HoleAt {
        HoleAt {
            name: name.into(),
            kind: kind.into(),
            instruction: "its first input".into(),
            source: HoleOrigin::Placeholder,
            line,
            text: format!("todo_{name}"),
            edges: String::new(),
            frame: Vec::new(),
        }
    }

    #[test]
    fn one_hole_in_two_files_is_one_hole_with_two_places() {
        let holes = HoleBook::default()
            .with_file(
                "a.x",
                &[at("field", "ident", 10), at("handle", "block", 28)],
            )
            .unwrap()
            .with_file("a_test.x", &[at("field", "ident", 11)])
            .unwrap()
            .holes();
        assert_eq!(holes.len(), 2);
        assert_eq!(
            holes[0].places,
            [
                HolePlace {
                    file: "a.x".into(),
                    line: 10,
                    text: "todo_field".into(),
                    edges: String::new(),
                    frame: Vec::new()
                },
                HolePlace {
                    file: "a_test.x".into(),
                    line: 11,
                    text: "todo_field".into(),
                    edges: String::new(),
                    frame: Vec::new()
                }
            ]
        );
    }

    #[test]
    fn one_name_with_two_kinds_is_refused() {
        let error = HoleBook::default()
            .with_file("a.x", &[at("field", "ident", 1)])
            .unwrap()
            .with_file("b.x", &[at("field", "block", 1)])
            .unwrap_err();
        assert!(error.contains("field") && error.contains("b.x"), "{error}");
    }
}
