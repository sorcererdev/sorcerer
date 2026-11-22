//! The web layer: requests as forms, answers as responses, one area a directory.

pub mod app;
pub mod bookings;

use std::collections::BTreeMap;

/// A request's fields, as the browser sent them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form(pub BTreeMap<String, String>);

impl Form {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

impl<const N: usize> From<[(&str, &str); N]> for Form {
    fn from(pairs: [(&str, &str); N]) -> Form {
        Form(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn new(status: u16, body: impl Into<String>) -> Response {
        Response {
            status,
            body: body.into(),
        }
    }
}
