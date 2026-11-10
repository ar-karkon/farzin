//! UCI protocol front end.
use crate::board::Board;
use crate::evalnet::EvalNet as Network;
use crate::movegen::perft;
use crate::search::{search, Limits, Params, Shared, Thread};
use crate::tt::TranspositionTable;
use crate::types::*;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

pub const NAME: &str = concat!("Farzin ", env!("CARGO_PKG_VERSION"));

/// Network embedded at build time when `FARZIN_EVALFILE` is set or the
/// default file exists (see build instructions in the README).
pub fn default_network() -> Option<Arc<Network>> {
    static BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/default.nnue"));
    if BYTES.is_empty() {
        return None;
    }
    Network::from_bytes(BYTES).ok().map(Arc::new)
}

pub const BENCH_FENS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
    "rnbqkb1r/pp2pppp/3p1n2/8/3NP3/8/PPP2PPP/RNBQKB1R w KQkq - 1 5",
    "r1bq1rk1/ppp2ppp/2np1n2/2b1p3/2B1P3/2PP1N2/PP3PPP/RNBQ1RK1 w - - 0 7",
    "rnbqk2r/ppp1bppp/4pn2/3p4/2PP4/2N2N2/PP2PPPP/R1BQKB1R w KQkq - 4 5",
    "r2q1rk1/pp2bppp/2n1pn2/2pp4/3P1B2/2PBPN2/PP1N1PPP/R2QK2R w KQ - 0 9",
    "2rq1rk1/pb2bppp/1pn1pn2/2pp4/3P4/1PPBPN2/PB1N1PPP/R2Q1RK1 w - - 4 12",
    "r1b2rk1/2q1bppp/p2ppn2/1p6/3BPP2/2N2B2/PPP3PP/R2Q1R1K w - - 0 14",
    "3r1rk1/p4ppp/1qp1b3/2b1P3/2Bp4/1P3N2/P4PPP/R2QR1K1 w - - 0 18",
    "r4rk1/1b3ppp/p2qpn2/1p6/3N4/1BP5/PP3PPP/R2QR1K1 w - - 2 17",
    "2r2rk1/pp1q1ppp/4pn2/3p4/3P4/P1PQ1N2/5PPP/R4RK1 b - - 0 17",
    "6k1/pp3ppp/4p3/3pP3/1P1n1P2/P2B4/6PP/6K1 w - - 0 30",
    "8/5pk1/6p1/1p1P3p/1P3P1P/6P1/5K2/8 w - - 0 45",
    "4r1k1/1p3pp1/p1p4p/8/2PP4/1P3RPP/P4PK1/8 b - - 0 30",
    "8/8/4k3/3p4/3K4/8/4P3/8 w - - 0 1",
    "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1",
    "8/p7/1p6/2p5/2P5/1P6/P4K2/7k w - - 0 1",
    "5rk1/1pp2pbp/p2p2p1/3P4/2P1rP2/1P2B3/P5PP/2R2RK1 w - - 0 24",
    "r1bqr1k1/pp3pbp/2pp1np1/4n3/2PNP3/2N1BP2/PP1Q2PP/R3KB1R w KQ - 2 11",
    "r2qkb1r/1p1b1ppp/p1nppn2/8/3NP3/2N1BP2/PPPQ2PP/R3KB1R w KQkq - 0 9",
    "rn1qk2r/pbppbppp/1p2pn2/8/2PP4/5NP1/PP2PPBP/RNBQK2R w KQkq - 3 6",
    "2kr3r/ppp1qppp/2n1bn2/2b1p3/4P3/2NP1N2/PPPBBPPP/R2QK2R w KQ - 6 9",
    "8/3k4/8/2K1P3/8/8/8/8 w - - 0 1",
    "1k6/1pp5/p7/8/8/P7/1PP5/1K6 w - - 0 1",
    "r5k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
];

struct Options {
    hash_mb: usize,
    threads: usize,
    overhead: u64,
    params: Params,
    net: Option<Arc<Network>>,
}

pub struct Engine {
    shared: Arc<Shared>,
    threads: Option<Vec<Thread>>,
    worker: Option<JoinHandle<Vec<Thread>>>,
    board: Board,
    history: Vec<u64>,
    opts: Options,
}

fn new_shared(mb: usize) -> Arc<Shared> {
    Arc::new(Shared {
        tt: TranspositionTable::new(mb),
        stop: AtomicBool::new(false),
        nodes: AtomicU64::new(0),
    })
}

/// Parse the clock part of a `go` command into search limits.
pub fn limits_from_go(words: &[&str], stm: usize, overhead: u64) -> Limits {
    let num = |key: &str| {
        words
            .windows(2)
            .find(|w| w[0] == key)
            .and_then(|w| w[1].parse::<i64>().ok())
    };
    let mut limits = Limits {
        depth: num("depth").map(|d| d as i32),
        nodes: num("nodes").map(|n| n.max(1) as u64),
        infinite: words.contains(&"infinite"),
        ..Default::default()
    };
    if let Some(mt) = num("movetime") {
        let t = (mt.max(1) as u64).saturating_sub(overhead).max(1);
        limits.soft_ms = Some(t);
        limits.hard_ms = Some(t);
        // A fixed move time should use the whole budget.
        limits.soft_ms = None;
    }
    let (time, inc) = if stm == WHITE {
        (num("wtime"), num("winc"))
    } else {
        (num("btime"), num("binc"))
    };
    if let Some(time) = time {
        let time = time.max(1) as u64;
        let inc = inc.unwrap_or(0).max(0) as u64;
        let avail = time.saturating_sub(overhead).max(1);
        let soft = match num("movestogo") {
            Some(mtg) => {
                let mtg = (mtg.clamp(1, 50)) as u64;
                (avail / mtg + inc * 3 / 4).min(avail * 8 / 10)
            }
            None => (avail / 25 + inc * 3 / 4).min(avail * 4 / 10),
        };
        let hard = (soft * 3).min(avail * 7 / 10).max(1);
        // Soft limit is compared against scaled targets between iterations.
        limits.soft_ms = Some((soft * 6 / 10).max(1));
        limits.hard_ms = Some(hard);
    }
    if limits.infinite {
        limits.soft_ms = None;
        limits.hard_ms = None;
    }
    limits
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let opts = Options {
            hash_mb: 16,
            threads: 1,
            overhead: 20,
            params: Params::default(),
            net: default_network(),
        };
        let mut engine = Engine {
            shared: new_shared(opts.hash_mb),
            threads: Some(Vec::new()),
            worker: None,
            board: Board::startpos(),
            history: Vec::new(),
            opts,
        };
        engine.rebuild_threads();
        engine
    }

    fn wait(&mut self) {
        if let Some(handle) = self.worker.take() {
            self.threads = Some(handle.join().expect("search thread panicked"));
        }
    }

    fn rebuild_threads(&mut self) {
        let threads = (0..self.opts.threads)
            .map(|id| {
                let mut t = Thread::new(id);
                t.set_params(self.opts.params);
                t.net = self.opts.net.clone();
                t
            })
            .collect();
        self.threads = Some(threads);
    }

    fn new_game(&mut self) {
        self.wait();
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.tt.clear();
        } else {
            self.shared = new_shared(self.opts.hash_mb);
        }
        for t in self.threads.as_mut().unwrap() {
            t.histories.clear();
        }
    }

    fn set_position(&mut self, words: &[&str]) -> Result<(), String> {
        let moves_at = words.iter().position(|w| *w == "moves");
        let mut board = match words.get(1) {
            Some(&"startpos") => Board::startpos(),
            Some(&"fen") => {
                let end = moves_at.unwrap_or(words.len());
                Board::from_fen(&words[2..end].join(" "))?
            }
            _ => return Err("position needs startpos or fen".into()),
        };
        let mut history = Vec::new();
        if let Some(i) = moves_at {
            for text in &words[i + 1..] {
                let mv = board
                    .parse_move(text)
                    .ok_or_else(|| format!("illegal move {text}"))?;
                history.push(board.hash);
                board = board.make_move(mv);
            }
        }
        self.board = board;
        self.history = history;
        Ok(())
    }

    fn go(&mut self, words: &[&str]) {
        self.wait();
        if let Some(i) = words.iter().position(|w| *w == "perft") {
            let depth = words.get(i + 1).and_then(|d| d.parse().ok()).unwrap_or(1);
            let start = Instant::now();
            let nodes = perft(&self.board, depth);
            let ms = start.elapsed().as_millis().max(1);
            println!("info string perft {depth} nodes {nodes} time {ms}");
            println!("{nodes}");
            return;
        }
        let limits = limits_from_go(words, self.board.stm, self.opts.overhead);
        let mut threads = self.threads.take().unwrap();
        let shared = Arc::clone(&self.shared);
        let board = self.board;
        let history = self.history.clone();
        threads[0].silent = false;
        self.worker = Some(std::thread::spawn(move || {
            let result = search(&shared, &mut threads, &board, &history, limits);
            let mut line = format!("bestmove {}", result.best.to_uci());
            if !result.ponder.is_none() {
                line.push_str(&format!(" ponder {}", result.ponder.to_uci()));
            }
            println!("{line}");
            threads
        }));
    }

    fn set_option(&mut self, line: &str) {
        self.wait();
        let rest = line.trim_start_matches("setoption").trim();
        let rest = rest.strip_prefix("name ").unwrap_or(rest);
        let (name, value) = match rest.split_once(" value ") {
            Some((n, v)) => (n.trim(), v.trim()),
            None => (rest.trim(), ""),
        };
        match name.to_ascii_lowercase().as_str() {
            "hash" => {
                if let Ok(mb) = value.parse::<usize>() {
                    self.opts.hash_mb = mb.clamp(1, 65_536);
                    self.shared = new_shared(self.opts.hash_mb);
                }
            }
            "threads" => {
                if let Ok(n) = value.parse::<usize>() {
                    self.opts.threads = n.clamp(1, 256);
                    self.rebuild_threads();
                }
            }
            "move overhead" => {
                if let Ok(ms) = value.parse() {
                    self.opts.overhead = ms;
                }
            }
            "evalmaterialscale" => {
                crate::hka::MATERIAL_SCALE
                    .store(value == "true", std::sync::atomic::Ordering::Relaxed);
            }
            "evalscale" => {
                if let Ok(v) = value.parse() {
                    crate::evalnet::set_hka_scale(v);
                }
            }
            "clear hash" => self.new_game(),
            "evalfile" => {
                if value.is_empty() || value == "<empty>" {
                    self.opts.net = None;
                    println!("info string using classical evaluation");
                } else if value == "<default>" {
                    self.opts.net = default_network();
                    println!(
                        "info string using {}",
                        if self.opts.net.is_some() {
                            "embedded network"
                        } else {
                            "classical evaluation"
                        }
                    );
                } else {
                    match Network::load(value) {
                        Ok(net) => {
                            println!("info string loaded network {value}: {}", net.describe());
                            self.opts.net = Some(Arc::new(net));
                        }
                        Err(e) => println!("info string failed to load network: {e}"),
                    }
                }
                self.rebuild_threads();
                self.new_game();
            }
            other => {
                if let Some(param) = other.strip_prefix("tune_") {
                    if set_param(&mut self.opts.params, param, value) {
                        self.rebuild_threads();
                    } else {
                        println!("info string unknown tunable {param}");
                    }
                } else {
                    println!("info string unknown option {name}");
                }
            }
        }
    }

    pub fn handle(&mut self, line: &str) -> bool {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(&cmd) = words.first() else {
            return true;
        };
        match cmd {
            "uci" => {
                println!("id name {NAME}");
                println!("id author Farzin contributors");
                println!("option name Hash type spin default 16 min 1 max 65536");
                println!("option name Threads type spin default 1 min 1 max 256");
                println!("option name Move Overhead type spin default 20 min 0 max 5000");
                println!("option name Clear Hash type button");
                println!("option name EvalFile type string default <default>");
                println!("option name EvalMaterialScale type check default false");
                println!(
                    "option name EvalScale type spin default {} min 1 max 4096",
                    crate::evalnet::DEFAULT_HKA_SCALE
                );
                for (name, value) in param_list(&self.opts.params) {
                    println!(
                        "option name tune_{name} type spin default {value} min -100000 max 100000"
                    );
                }
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => self.new_game(),
            "position" => {
                self.wait();
                if let Err(e) = self.set_position(&words) {
                    println!("info string {e}");
                }
            }
            "go" => self.go(&words),
            "stop" => self.shared.stop.store(true, Ordering::Relaxed),
            "setoption" => self.set_option(line),
            "quit" => {
                self.shared.stop.store(true, Ordering::Relaxed);
                self.wait();
                return false;
            }
            "d" => {
                self.wait();
                println!("{}", self.board.to_fen());
            }
            "eval" => {
                self.wait();
                println!(
                    "info string classical {}",
                    crate::eval::evaluate(&self.board)
                );
                if let Some(net) = &self.opts.net {
                    println!("info string network {}", net.evaluate(&self.board));
                    if let Network::Hka(h) = net.as_ref() {
                        println!("info string raw {}", h.evaluate(&self.board));
                    }
                }
            }
            "bench" => {
                self.wait();
                let depth = words.get(1).and_then(|d| d.parse().ok()).unwrap_or(12);
                bench(depth, self.opts.net.clone());
            }
            _ => println!("info string unknown command {cmd}"),
        }
        true
    }
}

macro_rules! params_table {
    ($m:ident) => {
        $m!(
            rfp_margin,
            rfp_depth,
            nmp_base,
            nmp_div,
            razor_margin,
            futility_base,
            futility_mult,
            see_quiet,
            see_noisy,
            hist_div,
            probcut_margin,
            singular_depth,
            asp_delta,
            qs_futility,
            hist_prune
        )
    };
}

fn param_list(p: &Params) -> Vec<(&'static str, i32)> {
    macro_rules! list {
        ($($f:ident),*) => { vec![$((stringify!($f), p.$f)),*] };
    }
    let mut v = params_table!(list);
    v.push(("lmr_quiet_base", (p.lmr_quiet_base * 100.0) as i32));
    v.push(("lmr_quiet_div", (p.lmr_quiet_div * 100.0) as i32));
    v.push(("lmr_noisy_base", (p.lmr_noisy_base * 100.0) as i32));
    v.push(("lmr_noisy_div", (p.lmr_noisy_div * 100.0) as i32));
    v
}

fn set_param(p: &mut Params, name: &str, value: &str) -> bool {
    let Ok(v) = value.parse::<i32>() else {
        return false;
    };
    macro_rules! set {
        ($($f:ident),*) => {
            match name {
                $(stringify!($f) => { p.$f = v; return true; })*
                _ => {}
            }
        };
    }
    params_table!(set);
    match name {
        "lmr_quiet_base" => p.lmr_quiet_base = f64::from(v) / 100.0,
        "lmr_quiet_div" => p.lmr_quiet_div = f64::from(v) / 100.0,
        "lmr_noisy_base" => p.lmr_noisy_base = f64::from(v) / 100.0,
        "lmr_noisy_div" => p.lmr_noisy_div = f64::from(v) / 100.0,
        _ => return false,
    }
    true
}

/// Fixed-depth search over `BENCH_FENS`; prints a node-count signature.
pub fn bench(depth: i32, net: Option<Arc<Network>>) {
    let mut shared = new_shared(16);
    let mut threads = vec![Thread::new(0)];
    threads[0].net = net;
    let mut total = 0;
    let start = Instant::now();
    for fen in BENCH_FENS {
        let board = Board::from_fen(fen).unwrap();
        Arc::get_mut(&mut shared)
            .expect("bench owns its table")
            .tt
            .clear();
        threads[0].silent = true;
        let limits = Limits {
            depth: Some(depth),
            ..Default::default()
        };
        let r = search(&shared, &mut threads, &board, &[], limits);
        total += r.nodes;
    }
    let ms = start.elapsed().as_millis().max(1) as u64;
    println!("info string bench depth {depth} time {ms}");
    println!("{total} nodes {} nps", total * 1000 / ms);
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("bench") {
        let depth = args.get(2).and_then(|d| d.parse().ok()).unwrap_or(12);
        bench(depth, default_network());
        return;
    }
    if args.get(1).map(String::as_str) == Some("datagen") {
        let get = |key: &str| {
            args.iter()
                .find_map(|a| a.strip_prefix(&format!("{key}=")))
                .map(str::to_string)
        };
        let num =
            |key: &str, default: u64| get(key).and_then(|v| v.parse().ok()).unwrap_or(default);
        let net = match get("evalfile") {
            Some(path) if path == "none" => None,
            Some(path) => Some(Arc::new(Network::load(&path).expect("load network"))),
            None => default_network(),
        };
        let teacher = get("teacher").map(|p| Arc::new(Network::load(&p).expect("load teacher")));
        crate::datagen::run(crate::datagen::Config {
            teacher,
            threads: num("threads", 1) as usize,
            games: num("games", 1000),
            soft_nodes: num("soft_nodes", 5000),
            hard_nodes: num("hard_nodes", 100_000),
            random_plies: num("random_plies", 8) as usize,
            out: get("out").unwrap_or_else(|| "farzin.data".to_string()),
            seed: num("seed", 1),
            net,
        });
        return;
    }
    let mut engine = Engine::new();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if !engine.handle(line.trim()) {
            break;
        }
    }
    engine.shared.stop.store(true, Ordering::Relaxed);
    engine.wait();
}
