//! Local-network microbenchmark; JSON output. No model is downloaded.
use chess::{Board, MoveGen};
use farzin::caissa::{CachedEvaluator, Network, SearchEvaluator};
use std::{hint::black_box, sync::Arc, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: caissa_bench NETWORK [ITERATIONS]")?;
    let iterations: u64 = args.next().unwrap_or_else(|| "10000".into()).parse()?;
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    let net = Arc::new(Network::from_file(path)?);
    let root = Board::default();
    let mut boards = vec![root];
    for ply in 0..64 {
        let board = boards.last().unwrap();
        let moves: Vec<_> = MoveGen::new_legal(board).collect();
        if moves.is_empty() {
            break;
        }
        boards.push(board.make_move_new(moves[(ply * 17 + 3) % moves.len()]));
    }
    let mut cache = CachedEvaluator::new(Arc::clone(&net), &root);
    let started = Instant::now();
    for _ in 0..iterations {
        black_box(cache.evaluate(black_box(&root)));
    }
    let forward = started.elapsed().as_secs_f64();
    let started = Instant::now();
    for i in 0..iterations {
        black_box(net.evaluate(black_box(&boards[i as usize % boards.len()])));
    }
    let full = started.elapsed().as_secs_f64();
    let started = Instant::now();
    for i in 0..iterations {
        black_box(cache.evaluate(black_box(&boards[i as usize % boards.len()])));
    }
    let cached = started.elapsed().as_secs_f64();
    let mut stack = SearchEvaluator::new(Arc::clone(&net), &root);
    let started = Instant::now();
    let mut evaluated = 0;
    while evaluated < iterations {
        let mut pushed = 0;
        for board in boards.iter().skip(1) {
            stack.push(board);
            pushed += 1;
            black_box(stack.evaluate(black_box(board)));
            evaluated += 1;
            if evaluated == iterations {
                break;
            }
        }
        for _ in 0..pushed {
            stack.pop();
        }
    }
    let stacked = started.elapsed().as_secs_f64();
    println!("{{\"iterations\":{iterations},\"forward_seconds\":{forward},\"full_seconds\":{full},\"cached_seconds\":{cached},\"stack_seconds\":{stacked}}}");
    Ok(())
}
