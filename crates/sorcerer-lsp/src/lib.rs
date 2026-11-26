//! Sorcerer's language-server host: everything about being a language server that is not
//! about one product. A product (scry first) implements [`Product`] and calls [`serve`]; the
//! host does the handshake, keeps the set of open documents (and their text, for a product that
//! asks), turns URIs into paths, routes the client's replies, and gives the product a [`Client`]
//! to speak through (and to apply edits with), [`Jobs`] to run work after a delay, and a
//! [`Reply`] for a request it answers once the editor has answered a question of its own.
//!
//! Synchronous, like the rest of the workspace: the main loop reads the connection on the
//! thread that called `serve`; delayed jobs and anything a product starts run on threads of
//! their own and speak through the `Client`, which is cheap to clone.

pub mod client;
pub mod jobs;
pub mod reply;
pub mod serve;
pub mod uri;

pub use client::{Answer, Applied, Client, WorkDone};
pub use jobs::Jobs;
pub use reply::Reply;
pub use serve::{Product, serve};
pub use uri::{path_of, uri_of};
