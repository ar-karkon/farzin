# Farzin networks

`default.nnue` is embedded into the engine at build time (override with
`FARZIN_EVALFILE=/path/net.nnue cargo build --release`, or at run time with
`setoption name EvalFile value /path/net.nnue`; `<empty>` selects the
classical evaluation).

All networks here are trained by Farzin's own trainer (`farzin-train`) on
positions from Farzin's own self-play (`farzin datagen`); no third-party
weights or evaluations are used, so they are distributed with the project.

| file | gen | architecture | data | training | result |
|---|---|---|---|---|---|
| default.nnue (sha256 4bde5a1a…) | g2 | (768→256)×2→1, SCReLU, 8 output buckets | 7.9M positions: gen0 (classical self-play) + 3.0M gen1 (g1a self-play) | 12 epochs, wdl 0.0, scale 400 | +162.4 Elo [+118.1, +212.4] vs g1a, SPRT [0,10] H1 accepted after 47 pairs at 10+0.1; +66.5 [+34.4, +99.8] at 20k nodes |
| (previous, sha256 75e560f7…) | g1a | (768→256)×2→1, SCReLU, 8 output buckets | 5.0M positions, classical-eval self-play, 4k/12k nodes | 10 epochs, wdl 0.0, scale 400, AdamW lr 1e-3 cosine | +50.8 Elo [+16.8, +85.9] vs classical, 148 pairs 10+0.1 |
