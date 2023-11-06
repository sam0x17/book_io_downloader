//! Library exposing public functions and types that underlie the `bicd` CLI utility for
//! downloading book covers from BookIO.
#![warn(missing_docs)]

mod client;
pub use client::*;
mod cli;
pub use cli::*;
