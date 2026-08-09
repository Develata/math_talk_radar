//! Composition root + thin command layer for `math_talk_radar`.
#![forbid(unsafe_code)]

pub mod cli;
pub mod commands;
pub mod config_loader;
pub mod output;
pub mod runtime;
pub mod scan_engine;
