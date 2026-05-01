//! Gmail provider (REST API).

mod client;
mod labels;
mod messages;
mod parse;
mod triage;

pub use client::AuthClient;
