//! pdfq library: fast, parallel PDF search with a pdfgrep-compatible CLI (backs the binary).
pub mod cache;
pub mod cli;
pub mod engine;
pub mod output;
pub mod pagerange;
pub mod pdf;
pub mod search;
pub mod walk;
