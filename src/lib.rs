//! Farzin chess engine library.
pub mod attacks;
pub mod board;
pub mod datagen;
pub mod eval;
pub mod movegen;
pub mod movepick;
pub mod net;
pub mod search;
pub mod tt;
pub mod types;
pub mod uci;

/// Experimental Caissa-format network reader kept for reference experiments.
pub mod caissa;
pub mod nnue;
