use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

#[test]
fn parallel_search_obeys_shared_node_limit() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(input, "uci\nsetoption name Threads value 2\nsetoption name PVS value true\nsetoption name Aspiration value true\nposition startpos\ngo nodes 1000").unwrap();
    input.flush().unwrap();
    let mut advertised = false;
    let mut nodes = None;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        advertised |= line.starts_with("option name Threads ");
        if let Some(value) = line.strip_prefix("info string parallel workers 2 nodes ") {
            nodes = Some(value.trim().parse::<u64>().unwrap());
        }
        if let Some(mv) = line.strip_prefix("bestmove ") {
            let mv = chess::ChessMove::from_str(mv.trim()).unwrap();
            assert!(chess::MoveGen::new_legal(&chess::Board::default()).any(|legal| legal == mv));
            break;
        }
    }
    assert!(advertised);
    assert_eq!(nodes, Some(1000));
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn parallel_stop_and_replacement_do_not_leak_results() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        input,
        "setoption name Threads value 2\nposition startpos\ngo infinite\nstop\nisready"
    )
    .unwrap();
    input.flush().unwrap();
    let mut bestmoves = 0;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        bestmoves += usize::from(line.starts_with("bestmove "));
        if line.trim() == "readyok" {
            break;
        }
    }
    assert_eq!(bestmoves, 1);
    writeln!(input, "go infinite\nposition startpos moves e2e4\nisready").unwrap();
    input.flush().unwrap();
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        assert!(!line.starts_with("bestmove "), "stale result: {line}");
        if line.trim() == "readyok" {
            break;
        }
    }
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn handshake_and_node_limited_search() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(input, "uci\nisready\nposition startpos\ngo nodes 1000").unwrap();
    input.flush().unwrap();
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        let done = line.starts_with("bestmove ");
        lines.push(line);
        if done {
            break;
        }
    }
    assert!(lines.iter().any(|s| s.trim() == "uciok"));
    assert!(lines.iter().any(|s| s.trim() == "readyok"));
    assert!(lines
        .iter()
        .any(|s| s.starts_with("info depth ") && s.contains(" nps ")));
    let stats = lines
        .iter()
        .find(|s| s.starts_with("info string tt probes "))
        .unwrap();
    let fields: Vec<_> = stats.split_whitespace().collect();
    let probes: u64 = fields[4].parse().unwrap();
    let hits: u64 = fields[6].parse().unwrap();
    let cutoffs: u64 = fields[8].parse().unwrap();
    assert!(cutoffs <= hits && hits <= probes);
    assert!(!lines.last().unwrap().contains("0000"));
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn terminal_position_reports_no_move() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        input,
        "position startpos moves f2f3 e7e5 g2g4 d8h4\ngo depth 2"
    )
    .unwrap();
    input.flush().unwrap();
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        if line.starts_with("bestmove ") {
            assert_eq!(line.trim(), "bestmove 0000");
            break;
        }
    }
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn stop_infinite_search_produces_one_bestmove() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(input, "position startpos\ngo infinite\nstop\nisready").unwrap();
    input.flush().unwrap();
    let mut bestmoves = 0;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        if line.starts_with("bestmove ") {
            bestmoves += 1;
        }
        if line.trim() == "readyok" {
            break;
        }
    }
    assert_eq!(bestmoves, 1);
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn clock_limited_search_finishes_promptly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let started = Instant::now();
    writeln!(
        input,
        "position startpos\ngo wtime 1000 btime 1000 movestogo 10"
    )
    .unwrap();
    input.flush().unwrap();
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        if line.starts_with("bestmove ") {
            break;
        }
    }
    assert!(started.elapsed() < Duration::from_secs(3));
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn replacing_search_does_not_emit_stale_bestmove() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        input,
        "position startpos\ngo infinite\nposition startpos moves e2e4\nisready"
    )
    .unwrap();
    input.flush().unwrap();
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        assert!(!line.starts_with("bestmove "), "stale result: {line}");
        if line.trim() == "readyok" {
            break;
        }
    }
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn reported_principal_variation_replays_legally() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(input, "position startpos\ngo depth 3").unwrap();
    input.flush().unwrap();
    let mut deepest = 0;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        if line.starts_with("info depth ") {
            let words: Vec<_> = line.split_whitespace().collect();
            let depth: u32 = words[2].parse().unwrap();
            let pv_index = words.iter().position(|word| *word == "pv").unwrap();
            let mut board = chess::Board::default();
            assert!(pv_index + 1 < words.len());
            for token in &words[pv_index + 1..] {
                let mv = chess::ChessMove::from_str(token).unwrap();
                assert!(chess::MoveGen::new_legal(&board).any(|legal| legal == mv));
                board = board.make_move_new(mv);
            }
            deepest = deepest.max(depth);
        }
        if line.starts_with("bestmove ") {
            break;
        }
    }
    assert_eq!(deepest, 3);
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn aspiration_option_completes_search() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(input, "uci\nsetoption name Aspiration value true\nsetoption name PVS value true\nposition startpos\ngo depth 3").unwrap();
    input.flush().unwrap();
    let mut advertised = false;
    let mut depth_three = false;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        advertised |= line.starts_with("option name Aspiration ");
        depth_three |= line.starts_with("info depth 3 ");
        if line.starts_with("bestmove ") {
            assert!(!line.contains("0000"));
            break;
        }
    }
    assert!(advertised && depth_three);
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
}

#[test]
fn experimental_network_loads_and_can_be_disabled() {
    use std::fs::OpenOptions;
    let path = std::env::temp_dir().join(format!("farzin-uci-network-{}.pnn", std::process::id()));
    let mut network = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    network.set_len(75_703_872).unwrap();
    network.write_all(&0x43534e4eu32.to_le_bytes()).unwrap();
    network.write_all(&15u32.to_le_bytes()).unwrap();
    drop(network);
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let stack_option = if cfg!(feature = "nnue-stack") {
        "setoption name NNUE Stack value true\n"
    } else {
        ""
    };
    writeln!(
        input,
        "{stack_option}setoption name NNUE SIMD value true\nsetoption name Threads value 2\nsetoption name EvalFile value {}\nisready\nposition startpos\ngo depth 2",
        path.display()
    )
    .unwrap();
    input.flush().unwrap();
    let mut loaded = false;
    let mut ready = false;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        loaded |= line.contains("experimental network loaded");
        ready |= line.trim() == "readyok";
        if line.starts_with("bestmove ") {
            assert!(!line.contains("0000"));
            break;
        }
    }
    assert!(loaded && ready);
    writeln!(
        input,
        "setoption name NNUE SIMD value false\nsetoption name NNUE SIMD value true\nsetoption name EvalFile value /definitely/not/a/network\nisready"
    )
    .unwrap();
    input.flush().unwrap();
    let mut rejected = false;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        rejected |= line.contains("failed to load network");
        if line.trim() == "readyok" {
            break;
        }
    }
    assert!(rejected);
    writeln!(input, "setoption name EvalFile value <empty>\nisready").unwrap();
    input.flush().unwrap();
    let mut disabled = false;
    loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        disabled |= line.contains("classical evaluator enabled");
        if line.trim() == "readyok" {
            break;
        }
    }
    assert!(disabled);
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
    std::fs::remove_file(path).unwrap();
}

/// Runs a UCI script line by line, waiting for `bestmove` after each `go`, and
/// returns every output line up to the final `readyok`.
fn transcript(script: &str) -> Vec<String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_farzin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut lines = Vec::new();
    let mut read_until = |prefix: &str, lines: &mut Vec<String>| loop {
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        let line = line.trim().to_string();
        let done = line.starts_with(prefix);
        lines.push(line);
        if done {
            break;
        }
    };
    for command in script.lines().chain(["isready"]) {
        writeln!(input, "{}", command.trim()).unwrap();
        input.flush().unwrap();
        if command.trim().starts_with("go") {
            read_until("bestmove ", &mut lines);
        } else if command.trim() == "isready" {
            read_until("readyok", &mut lines);
        }
    }
    writeln!(input, "quit").unwrap();
    child.wait().unwrap();
    lines
}

#[test]
fn persistent_hash_is_reused_only_until_cleared() {
    let lines = transcript(
        "uci\nsetoption name Persistent Hash value true\nposition startpos\ngo depth 4\n\
         position startpos moves e2e4\ngo depth 4\nposition startpos moves e2e4 e7e5\ngo depth 4\n\
         ucinewgame\nposition startpos\ngo depth 4\nsetoption name Clear Hash\nposition startpos\n\
         go depth 4\nsetoption name Hash value 8\nposition startpos\ngo depth 4\n\
         position startpos moves d2d4\ngo depth 3",
    );
    for (option, default) in [
        ("PVS", true),
        ("LMR", true),
        ("Null Move", true),
        ("Persistent History", false),
        ("Persistent Hash", true),
    ] {
        assert!(lines.contains(&format!(
            "option name {option} type check default {default}"
        )));
    }
    let reuse: Vec<_> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("info string tt reused "))
        .map(|rest| rest.starts_with("true"))
        .collect();
    assert_eq!(
        reuse,
        [false, true, true, false, false, false, true],
        "{lines:#?}"
    );
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("bestmove ")).count(),
        7
    );
}

#[test]
fn hash_is_not_reused_when_disabled() {
    let lines = transcript(
        "setoption name Persistent Hash value false\nposition startpos\ngo depth 3\n\
         position startpos moves e2e4\ngo depth 3",
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("info string tt reused false"))
            .count(),
        2
    );
}

#[test]
fn pruning_options_search_and_replay_legally() {
    let lines = transcript(
        "setoption name LMR value true\nsetoption name Null Move value true\n\
         setoption name Persistent History value true\nsetoption name PVS value true\n\
         setoption name Aspiration value true\n\
         position fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6\n\
         go depth 6",
    );
    let board =
        chess::Board::from_str("r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6")
            .unwrap();
    let infos: Vec<_> = lines
        .iter()
        .filter(|line| line.starts_with("info depth "))
        .collect();
    assert_eq!(infos.len(), 6);
    for info in infos {
        let pv = info.split(" pv ").nth(1).unwrap();
        let mut position = board;
        for mv in pv.split_whitespace() {
            let mv = chess::ChessMove::from_str(mv).unwrap();
            assert!(chess::MoveGen::new_legal(&position).any(|legal| legal == mv));
            position = position.make_move_new(mv);
        }
    }
}
