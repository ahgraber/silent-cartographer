//! silent-cartographer (`c10r`): a persistent codebase knowledge graph that unifies a syntax tree
//! with a semantic index into one graph keyed on stable symbol identities.
//!
//! This crate stands up the structural spine for Rust: canonical symbol identity, the semantic
//! engine port, the guarded syntax/semantic join, the SQLite-core store, and the query surface
//! exposed by the `c10r` CLI.

pub mod cli;
pub mod commands;
pub mod exit;
pub mod graph;
pub mod identity;
pub mod manifest;
pub mod query;
pub mod render;
pub mod semantic;
