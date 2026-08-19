//! A bounded, byte-oriented row scanner for PostgreSQL custom-format dumps.
//!
//! Archive-format behavior is introduced incrementally by the v0.1 implementation issues.

// Issue #7 wires these crate-private primitives into the production header parser.
#[allow(dead_code)]
mod custom;
mod error;
#[allow(dead_code)]
mod io;
#[allow(dead_code)]
mod limits;

pub use error::PgDumpError;

#[cfg(test)]
mod issue6_tests;
