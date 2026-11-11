# Farzin networks

`default.nnue` is embedded into the engine at build time (override with
`FARZIN_EVALFILE=/path/net.nnue cargo build --release`, or at run time with
`setoption name EvalFile value /path/net.nnue`; `<empty>` selects the
classical evaluation).

Two kinds of network are kept here:

* `default.nnue` — **third-party weights, CC0-1.0.** `nn-ad9b42354671.nnue`
  (HalfKAv2_hm, (22528→1024)×2→15→32→1, 8 PSQT buckets, 8 layer stacks),
  published by the Stockfish project in
  <https://github.com/official-stockfish/networks>, whose README states that
  all networks there were uploaded by their authors under CC0
  (see that repository's `LICENSE`). The network description string reads
  "Network trained with the https://github.com/glinscott/nnue-pytorch trainer."
  sha256 `ad9b423546714137916bd38978af6fd68d7b8951bef25ff76bf43da72d6cb786`.
  Farzin runs it with its own independent Rust inference (`src/hka.rs`); no
  Stockfish source code is included. Other CC0 nets of the same family can be
  fetched and verified with `tools/fetch_nets.sh`.
* `fzn2-b1.nnue` — **Farzin's best own net** (Farzin 0.5, Track B): same
  architecture as `fzn2-a1` ((4 mirrored king buckets x 768 -> 768) x 2 -> 1 x 8,
  SCReLU), **warm-started from `fzn2-a1`** (`farzin-train2 init=`) and trained
  3 more epochs (Adam lr 5e-4 cosine to 2.5e-5, wdl 0, batch 16384, 2 threads,
  47 min) on 100.1M *new* tree-sampled positions (`datagen tree_rate=48 seed=2027`,
  mover = Farzin 0.5 search + `default.nnue`, 2500 soft nodes), labelled with
  `default.nnue`'s static eval. Held-out MSE (new seed): 0.002533 vs 0.002853
  for a1. sha256 `3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0`.
  vs fzn2-a1: +63.1 Elo [+36.5, +90.5] at 5+0.05 (SPRT [0,10] H1, 89 pairs).
  Effective training data: 105M (a1) + 100M positions, all from Farzin's own
  play with CC0 labels; no third-party restrictions.
* `fzn2-a1.nnue` — **Farzin's own FZN2 net** (Farzin 0.4, Track B):
  (4 mirrored king buckets x 768 -> 768) x 2 -> 1 x 8, SCReLU (`src/fzn2.rs`).
  Data: 105.4M positions *sampled from Farzin's search trees* (`datagen
  tree_rate=48`, mover = Farzin with `default.nnue` at 2500 soft nodes,
  8-9 random opening plies, 5% random moves in the first 30 plies), each
  labelled with `default.nnue`'s static evaluation (x EvalScale 560/1024).
  Trained by `farzin-train2` (4 epochs, batch 16384, Adam lr 1e-3 cosine to
  5e-5, wdl 0, sigmoid scale 400) in 50 minutes on 3 CPU threads.
  sha256 `f7bb0b7ef9857c3a4f24b39f5152e381e978d76d0c24817b0bebbefb1b6e220c`.
  vs d1-512: +164.3 Elo [+123.2, +210.2] at 5+0.05 (SPRT [0,10] H1, 42 pairs);
  vs `default.nnue`: -230.2 [-265.2, -198.8] at 5+0.05 (100 pairs).
  Derived only from CC0 labels and Farzin's own play, no third-party restrictions.
* `d1-512.nnue` — Farzin's own FZN1 (768→512)×2→1 net **distilled** from
  `default.nnue`: 11.5M positions from g2 self-play (400 soft nodes), each
  labelled with the teacher's static eval (`datagen teacher=`), 12 epochs,
  wdl 0. sha256 `6a4181bbdbf4fd3613a0e771b39fd6cb2f1456f2c4f18e71259f7374b22825d0`.
  +275.5 Elo [+213.7, +356.5] vs g2 at 10+0.1 (50 pairs). Derived from CC0
  labels, so no third-party restrictions.
* `g2.nnue` and older FZN1 nets — trained by Farzin's own trainer
  (`farzin-train`) on Farzin's own self-play; no third-party weights or
  evaluations. Kept as the self-trained baseline (`EvalFile=nets/g2.nnue`).

| file | architecture | result |
|---|---|---|
| default.nnue (sha256 ad9b4235…) | HalfKAv2_hm 1024, CC0, see above | vs g2: +767.6 Elo [+577.9, ∞), SPRT [0,10] H1 after 21 pairs (41.5/42) at 10+0.1; see `docs/v6-report.md` |


Self-trained FZN1 history:

| file | gen | architecture | data | training | result |
|---|---|---|---|---|---|
| g2.nnue (sha256 4bde5a1a…) | g2 | (768→256)×2→1, SCReLU, 8 output buckets | 7.9M positions: gen0 (classical self-play) + 3.0M gen1 (g1a self-play) | 12 epochs, wdl 0.0, scale 400 | +162.4 Elo [+118.1, +212.4] vs g1a, SPRT [0,10] H1 accepted after 47 pairs at 10+0.1; +66.5 [+34.4, +99.8] at 20k nodes |
| (previous, sha256 75e560f7…) | g1a | (768→256)×2→1, SCReLU, 8 output buckets | 5.0M positions, classical-eval self-play, 4k/12k nodes | 10 epochs, wdl 0.0, scale 400, AdamW lr 1e-3 cosine | +50.8 Elo [+16.8, +85.9] vs classical, 148 pairs 10+0.1 |
