//! Evaluation-network dispatch: Farzin's own FZN1 nets or HalfKAv2_hm nets.
//!
//! Both backends expose the same incremental API to the search. Values are
//! returned in Farzin centipawns; HalfKAv2_hm output (reference-engine
//! internal units) is rescaled by [`hka_scale`] / 1024.
use std::sync::atomic::{AtomicI32, Ordering};

use crate::board::Board;
use crate::types::Move;
use crate::{fzn2, hka, net};

/// Multiplier (/1024) from HalfKAv2_hm internal units to Farzin centipawns.
/// A least-squares fit against Farzin's g2 evaluations gives ~425; matches
/// showed 560 is +37.8 Elo stronger than 425 and 700 no better than 560
/// (see `docs/v6-report.md`).
static HKA_SCALE: AtomicI32 = AtomicI32::new(DEFAULT_HKA_SCALE);
pub const DEFAULT_HKA_SCALE: i32 = 560;

pub fn hka_scale() -> i32 {
    HKA_SCALE.load(Ordering::Relaxed)
}

pub fn set_hka_scale(v: i32) {
    HKA_SCALE.store(v, Ordering::Relaxed);
}

pub enum EvalNet {
    Fzn1(net::Network),
    Fzn2(fzn2::Network),
    Hka(hka::Network),
}

impl EvalNet {
    pub fn from_bytes(bytes: &[u8]) -> Result<EvalNet, String> {
        if hka::Network::is_hka(bytes) {
            hka::Network::from_bytes(bytes).map(EvalNet::Hka)
        } else if fzn2::Network::is_fzn2(bytes) {
            fzn2::Network::from_bytes(bytes).map(EvalNet::Fzn2)
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
            EvalNet::Fzn2(n) => format!(
                "FZN2 {}x768x{} buckets {}{}",
                n.king_buckets,
                n.hidden,
                n.out_buckets,
                if n.mirror { " mirrored" } else { "" }
            ),
            EvalNet::Hka(n) => format!("HalfKAv2_hm 22528x{} ({})", n.l1, n.description),
        }
    }

    /// Full static evaluation in Farzin centipawns (side to move).
    pub fn evaluate(&self, board: &Board) -> i32 {
        match self {
            EvalNet::Fzn1(n) => n.evaluate(board),
            EvalNet::Fzn2(n) => n.evaluate(board),
            EvalNet::Hka(n) => n.evaluate(board) * hka_scale() / 1024,
        }
    }
}

#[derive(Default)]
pub enum EvalAccs {
    #[default]
    None,
    Fzn1(net::Accumulators),
    Fzn2(Box<fzn2::Accumulators>),
    Hka(Box<hka::Accumulators>),
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
            EvalNet::Fzn2(n) => {
                if !matches!(self, EvalAccs::Fzn2(_)) {
                    *self = EvalAccs::Fzn2(Box::default());
                }
                if let EvalAccs::Fzn2(a) = self {
                    a.refresh(n, board, ply);
                }
            }
            EvalNet::Hka(n) => {
                if !matches!(self, EvalAccs::Hka(_)) {
                    *self = EvalAccs::Hka(Box::default());
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
            (EvalAccs::Fzn2(a), EvalNet::Fzn2(n)) => a.push(n, parent, child, mv, ply),
            (EvalAccs::Hka(a), EvalNet::Hka(n)) => a.push(n, parent, child, mv, ply),
            _ => unreachable!("accumulators not refreshed for this network"),
        }
    }

    #[inline]
    pub fn push_null(&mut self, net: &EvalNet, ply: usize) {
        match (self, net) {
            (EvalAccs::Fzn1(a), EvalNet::Fzn1(n)) => a.push_null(n, ply),
            (EvalAccs::Fzn2(a), EvalNet::Fzn2(_)) => a.push_null(ply),
            (EvalAccs::Hka(a), EvalNet::Hka(_)) => a.push_null(ply),
            _ => unreachable!("accumulators not refreshed for this network"),
        }
    }

    #[inline]
    pub fn evaluate(&mut self, net: &EvalNet, board: &Board, ply: usize) -> i32 {
        match (self, net) {
            (EvalAccs::Fzn1(a), EvalNet::Fzn1(n)) => a.evaluate(n, board, ply),
            (EvalAccs::Fzn2(a), EvalNet::Fzn2(n)) => a.evaluate(n, board, ply),
            (EvalAccs::Hka(a), EvalNet::Hka(n)) => a.evaluate(n, board, ply) * hka_scale() / 1024,
            _ => unreachable!("accumulators not refreshed for this network"),
        }
    }
}
