//! Gmail provider (REST API).

mod client;
mod compose;
mod labels;
mod messages;
mod parse;
mod provider;
mod trash;
mod triage;

pub use client::AuthClient;
pub use provider::GmailProvider;
