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
