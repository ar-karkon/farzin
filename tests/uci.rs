use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

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
