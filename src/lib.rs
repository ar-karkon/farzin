//! Farzin chess engine library.
pub mod attacks;
pub mod board;
pub mod datagen;
pub mod eval;
pub mod evalnet;
pub mod fzn2;
pub mod hka;
pub mod movegen;
pub mod movepick;
pub mod net;
pub mod search;
pub mod tt;
pub mod types;
pub mod uci;

/// Experimental Caissa-format network reader kept for reference experiments.
#[cfg(feature = "legacy")]
pub mod caissa;
#[cfg(feature = "legacy")]
pub mod nnue;
