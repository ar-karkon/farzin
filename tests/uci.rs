use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use farzin::board::Board;

struct Engine {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Engine {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Engine {
            child,
            input,
            output,
        }
    }
    fn send(&mut self, text: &str) {
        writeln!(self.input, "{text}").unwrap();
        self.input.flush().unwrap();
    }
    /// Read lines until one starts with `prefix`; returns all lines read.
    fn until(&mut self, prefix: &str) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            assert_ne!(
                self.output.read_line(&mut line).unwrap(),
                0,
                "engine exited"
            );
            let done = line.starts_with(prefix);
            lines.push(line.trim_end().to_string());
            if done {
                return lines;
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "quit");
        let _ = self.child.wait();
    }
}

fn bestmove(lines: &[String]) -> String {
    lines
        .last()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

#[test]
fn handshake_and_node_limited_search() {
    let mut e = Engine::new();
    e.send("uci");
    let lines = e.until("uciok");
    assert!(lines.iter().any(|l| l.starts_with("id name Farzin")));
    assert!(lines.iter().any(|l| l.starts_with("option name Hash ")));
    e.send("isready");
    e.until("readyok");
    e.send("position startpos\ngo nodes 5000");
    let lines = e.until("bestmove");
    assert!(lines
        .iter()
        .any(|l| l.starts_with("info depth ") && l.contains(" nps ")));
    let mv = bestmove(&lines);
    let board = Board::startpos();
    assert!(board.parse_move(&mv).is_some(), "illegal bestmove {mv}");
}

#[test]
fn principal_variation_replays_legally() {
    let mut e = Engine::new();
    e.send("position fen r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4\ngo depth 8");
    let lines = e.until("bestmove");
    let info = lines.iter().rev().find(|l| l.contains(" pv ")).unwrap();
    let pv = info.split(" pv ").nth(1).unwrap();
    let mut board =
        Board::from_fen("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4")
            .unwrap();
    for mv in pv.split_whitespace() {
        let parsed = board
            .parse_move(mv)
            .unwrap_or_else(|| panic!("illegal pv move {mv}"));
        board = board.make_move(parsed);
    }
}

#[test]
fn finds_mate_and_reports_terminal_positions() {
    let mut e = Engine::new();
    e.send("position fen 6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1\ngo depth 6");
    let lines = e.until("bestmove");
    assert_eq!(bestmove(&lines), "d1d8");
    assert!(lines.iter().any(|l| l.contains("score mate 1")));
    // Checkmated side has no move to report.
    e.send("position fen 3R2k1/5ppp/8/8/8/8/5PPP/6K1 b - - 1 1\ngo depth 3");
    assert_eq!(bestmove(&e.until("bestmove")), "0000");
}

#[test]
fn stop_ends_infinite_search_with_one_bestmove() {
    let mut e = Engine::new();
    e.send("position startpos\ngo infinite");
    std::thread::sleep(Duration::from_millis(200));
    e.send("stop");
    e.until("bestmove");
    e.send("isready");
    let lines = e.until("readyok");
    assert!(
        !lines.iter().any(|l| l.starts_with("bestmove ")),
        "duplicate bestmove"
    );
}

#[test]
fn clock_limited_search_finishes_promptly() {
    let mut e = Engine::new();
    e.send("position startpos moves e2e4 e7e5\ngo wtime 1000 btime 1000 winc 10 binc 10");
    let start = Instant::now();
    e.until("bestmove");
    assert!(start.elapsed() < Duration::from_millis(800));
}

#[test]
fn threads_and_hash_options_work() {
    let mut e = Engine::new();
    e.send("setoption name Threads value 3\nsetoption name Hash value 8\nposition startpos moves d2d4\ngo depth 9");
    let mv = bestmove(&e.until("bestmove"));
    let board = Board::startpos().make_move(Board::startpos().parse_move("d2d4").unwrap());
    assert!(board.parse_move(&mv).is_some());
}
