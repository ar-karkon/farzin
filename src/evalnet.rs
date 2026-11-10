//! Evaluation-network dispatch: Farzin's own FZN1 nets or HalfKAv2_hm nets.
//!
//! Both backends expose the same incremental API to the search. Values are
//! returned in Farzin centipawns; HalfKAv2_hm output (reference-engine
//! internal units) is rescaled by [`hka_scale`] / 1024.
use std::sync::atomic::{AtomicI32, Ordering};

use crate::board::Board;
use crate::types::Move;
use crate::{hka, net};

/// Multiplier (/1024) from HalfKAv2_hm internal units to Farzin centipawns.
/// Default fitted by least squares against Farzin's own search scores
/// (see `docs/v6-report.md`).
static HKA_SCALE: AtomicI32 = AtomicI32::new(DEFAULT_HKA_SCALE);
pub const DEFAULT_HKA_SCALE: i32 = 425;

pub fn hka_scale() -> i32 {
    HKA_SCALE.load(Ordering::Relaxed)
}

pub fn set_hka_scale(v: i32) {
    HKA_SCALE.store(v, Ordering::Relaxed);
}

pub enum EvalNet {
    Fzn1(net::Network),
    Hka(hka::Network),
}

impl EvalNet {
    pub fn from_bytes(bytes: &[u8]) -> Result<EvalNet, String> {
        if hka::Network::is_hka(bytes) {
            hka::Network::from_bytes(bytes).map(EvalNet::Hka)
        } else {
            net::Network::from_bytes(bytes).map(EvalNet::Fzn1)
        }
    }

    pub fn load(path: &str) -> Result<EvalNet, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        EvalNet::from_bytes(&bytes)
    }

    pub fn describe(&self) -> String {
        match self {
            EvalNet::Fzn1(n) => format!("FZN1 768x{} buckets {}", n.hidden, n.buckets),
            EvalNet::Hka(n) => format!("HalfKAv2_hm 22528x{} ({})", n.l1, n.description),
        }
    }

    /// Full static evaluation in Farzin centipawns (side to move).
    pub fn evaluate(&self, board: &Board) -> i32 {
        match self {
            EvalNet::Fzn1(n) => n.evaluate(board),
            EvalNet::Hka(n) => n.evaluate(board) * hka_scale() / 1024,
        }
    }
}

pub enum EvalAccs {
    None,
    Fzn1(net::Accumulators),
    Hka(Box<hka::Accumulators>),
}

impl Default for EvalAccs {
    fn default() -> Self {
        EvalAccs::None
    }
}

impl EvalAccs {
    pub fn refresh(&mut self, net: &EvalNet, board: &Board, ply: usize) {
        match net {
            EvalNet::Fzn1(n) => {
                if !matches!(self, EvalAccs::Fzn1(_)) {
                    *self = EvalAccs::Fzn1(net::Accumulators::new());
                }
                if let EvalAccs::Fzn1(a) = self {
                    a.refresh(n, board, ply);
                }
            }
            EvalNet::Hka(n) => {
                if !matches!(self, EvalAccs::Hka(_)) {
                    *self = EvalAccs::Hka(Box::new(hka::Accumulators::new()));
                }
                if let EvalAccs::Hka(a) = self {
                    a.refresh(n, board, ply);
                }
            }
        }
    }

    #[inline]
    pub fn push(&mut self, net: &EvalNet, parent: &Board, child: &Board, mv: Move, ply: usize) {
        match (self, net) {
            (EvalAccs::Fzn1(a), EvalNet::Fzn1(n)) => a.push(n, parent, child, mv, ply),
            (EvalAccs::Hka(a), EvalNet::Hka(n)) => a.push(n, parent, child, mv, ply),
            _ => unreachable!("accumulators not refreshed for this network"),
        }
    }

    #[inline]
    pub fn push_null(&mut self, net: &EvalNet, ply: usize) {
        match (self, net) {
            (EvalAccs::Fzn1(a), EvalNet::Fzn1(n)) => a.push_null(n, ply),
            (EvalAccs::Hka(a), EvalNet::Hka(_)) => a.push_null(ply),
            _ => unreachable!("accumulators not refreshed for this network"),
        }
    }

    #[inline]
    pub fn evaluate(&self, net: &EvalNet, board: &Board, ply: usize) -> i32 {
        match (self, net) {
            (EvalAccs::Fzn1(a), EvalNet::Fzn1(n)) => a.evaluate(n, board, ply),
            (EvalAccs::Hka(a), EvalNet::Hka(n)) => a.evaluate(n, board, ply) * hka_scale() / 1024,
            _ => unreachable!("accumulators not refreshed for this network"),
        }
    }
}
