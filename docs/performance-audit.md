# Phase 3 audit

Source: `efdb2307bded` (full SHA recorded in baseline JSON), fetched from origin/main on 2026-11-09. No newer main commit or CI was present. Development branch: `feat/performance-search-v3`.

## Execution and state

- `uci::run` parses `go`, joins any previous worker and clones `Game`; `start_search` constructs `SearchControl`, a fresh evaluator and `IterationState`. Iterative deepening calls `IterationState::search`, optionally retries an aspiration failure, then emits `info` and `bestmove`.
- `search::best_move_window` searches root children; `search::search` implements negamax, TT probing, ordering and experimental PVS. First child uses the full window, later children use a null window and re-search inside (alpha,beta). Root uses ordinary alpha-beta. PVS/aspiration default off. Existing tests cover two PVS reference positions; competitive proof remains insufficient.
- `search::quiescence` handles checks, captures and promotions. At MAX_QPLY=16, checked nodes examine one layer of evasions; that boundary remains approximate.
- `game::Game::play` saves repetition hashes, updates halfmove and uses immutable `Board::make_move_new`. `unplay` restores the caller's Board and halfmove and pops history. Every recursive result is unmade before propagating `None` on stop. Draw detection includes threefold, fifty moves and conservative insufficient material. Board deltas correctly represent special moves; NNUE should track this same lifetime.
- `SearchState` killers/history are freshly zeroed for every `best_move_window`, including each iteration and aspiration re-search. Saturating history capped at 70,000 exists; aging and persistence do not. Hints and TT persist across iterations only. PV arrays are bounded; TT cutoffs can shorten PV.

## Allocations and evaluation

- `caissa::Network::from_file/from_bytes` checks exact length, magic and v15. Load transiently holds byte and decoded i16 weights, then drops transformer bytes. Subnets remain packed. No network file is in this checkout; prior report hashes are historical, not a current model validation.
- `CachedEvaluator::evaluate` diffs the last **evaluated** board, not the search parent. `Network::transition` originally allocated two sorted Vec feature lists for each perspective and binary-searched them. Bucket changes reconcile almost all features. `Network::accumulator` also allocated features. Transformer updates use wrapping i16 arithmetic. Hidden layers use stack arrays and skip zero input groups; scalar inference only at baseline.
- `nnue::Transformer/Accumulator` is a separate unused toy prototype. It allocates and checks i32 overflow transactionally; it is not the Caissa format or production evaluator.
- Search move lists and scores are stack arrays (256). `MoveGen` uses chess crate storage. `Game::history` can grow/reallocate; root Game is cloned. Hints HashMap grows to a 200k cap. Root PV Vec is returned per iteration. Formatting UCI output allocates. TT allocates one Vec per worker per `go`. Evaluator allocates once at construction plus feature Vecs per evaluation at baseline. Optional exchange ordering recursively creates Boards, not heap move lists. No network copying occurs per node.

## TT and concurrency

- `tt::Table` is direct-mapped, full 64-bit keys, depth/generation replacement, occupancy-based hashfull, probe/hit/cutoff counters, mate encoding/decoding. A generation advances each iteration. Score use/storage is restricted to halfmove=0; keep this conservative history condition. `Clear Hash` is effectively a no-op because tables are discarded per search. Persistence needs explicit ownership and evaluator invalidation design.
- `uci::start_parallel_search` launches independent workers with per-worker TT/hints/evaluator/history; only immutable Arc<Network>, atomic stop/node budget, and mpsc reports are shared. SearchControl RefCell is local to each worker. Shared node budget uses CAS with relaxed ordering, appropriate to counters. Coordinator chooses max completed depth, then max score. This selection is not proven stronger. No shared TT, LMR or null move exists.
- Environment exposes 3 logical CPUs; 4/8 workers would oversubscribe. Any scaling result must disclose this. No perf executable was found initially. No TSan claim is made.

## Evidence and constraints

Read all search/evaluation/game/TT/UCI sources, tests and prior JSON report metadata plus `docs/nnue-investigation.md`, `docs/search-core-status.md`, and notices. Existing reports are small, hand-selected tests, not sufficient Elo evidence. Caissa code notice is MIT; weight licensing is unresolved. Do not download/distribute weights automatically. Use a local user-supplied licensed network for reference tests.

Rust was absent initially; installed minimal stable toolchain plus rustfmt/clippy. A detached worktree builds the untouched baseline independently of development edits. Baseline required checks and raw repeated benchmark data are under `benchmarks/v3-baseline/`. Machine-specific performance is not playing strength.
