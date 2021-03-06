//! # Aspen
//! A general-purpose, object-oriented,
//! purely immutable programming language.
//!
//! ---
//!
//! This project contains the full compiler – the parser,
//! analyzer, optimizer, and emitter.
//!
//! It can be used directly as a Rust library, but is
//! mainly written to support the `aspen-cli` project,
//! which implements the CLI used for developing software
//! in Aspen.

#![feature(async_closure)]
#![feature(try_trait)]
#![feature(asm)]

#[macro_use]
extern crate async_trait;

#[macro_use]
extern crate lazy_static;

mod context;
mod diagnostics;
pub mod generation;
pub mod semantics;
mod source;
pub mod syntax;

pub use self::context::*;
pub use self::diagnostics::*;
pub use self::source::*;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
