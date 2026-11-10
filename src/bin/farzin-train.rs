//! Minimal multi-threaded CPU trainer for Farzin FZN1 networks.
//!
//! farzin-train data=a.data[,b.data] out=net.nnue hidden=256 buckets=8 \
//!     epochs=10 batch=16384 lr=0.001 wdl=0.3 threads=4 [init=old.nnue]
//!
//! Model: (768 -> H) shared feature transformer per perspective, SCReLU,
//! concatenated [stm, nstm] -> output bucket chosen by piece count.
//! Loss: MSE between sigmoid(cp / scale) and a blend of sigmoid(score / scale)
//! and the game result.
use farzin::datagen::{unpack, RECORD_SIZE};
use farzin::net::{feature, Network, INPUTS, QA, QB};
use std::time::Instant;

/// Network output unit: one output unit = SCALE centipawns (see net.rs).
const NET_SCALE: f32 = 400.0;
const WEIGHT_CLIP: f32 = 1.98;

struct Sample {
    features: [[u16; 32]; 2],
    count: u8,
    bucket: u8,
    target: f32,
}

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{key}=")).map(str::to_string))
}

struct Params {
    h: usize,
    nb: usize,
    ft_w: Vec<f32>,
    ft_b: Vec<f32>,
    out_w: Vec<f32>,
    out_b: Vec<f32>,
}

impl Params {
    fn len(&self) -> usize {
        self.ft_w.len() + self.ft_b.len() + self.out_w.len() + self.out_b.len()
    }
    fn zeros_like(&self) -> Params {
        Params {
            h: self.h,
            nb: self.nb,
            ft_w: vec![0.0; self.ft_w.len()],
            ft_b: vec![0.0; self.ft_b.len()],
            out_w: vec![0.0; self.out_w.len()],
            out_b: vec![0.0; self.out_b.len()],
        }
    }
    fn slices_mut(&mut self) -> [&mut Vec<f32>; 4] {
        [
            &mut self.ft_w,
            &mut self.ft_b,
            &mut self.out_w,
            &mut self.out_b,
        ]
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn uniform(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn init(h: usize, nb: usize, rng: &mut Rng) -> Params {
    let ft_scale = (1.0 / 32.0f32).sqrt();
    let out_scale = (1.0 / (2.0 * h as f32)).sqrt();
    Params {
        h,
        nb,
        ft_w: (0..INPUTS * h)
            .map(|_| (rng.uniform() * 2.0 - 1.0) * ft_scale)
            .collect(),
        ft_b: vec![0.0; h],
        out_w: (0..nb * 2 * h)
            .map(|_| (rng.uniform() * 2.0 - 1.0) * out_scale)
            .collect(),
        out_b: vec![0.0; nb],
    }
}

fn from_network(net: &Network) -> Params {
    Params {
        h: net.hidden,
        nb: net.buckets,
        ft_w: net
            .ft_weights
            .iter()
            .map(|&w| w as f32 / QA as f32)
            .collect(),
        ft_b: net.ft_bias.iter().map(|&w| w as f32 / QA as f32).collect(),
        out_w: net
            .out_weights
            .iter()
            .map(|&w| w as f32 / QB as f32)
            .collect(),
        out_b: net
            .out_bias
            .iter()
            .map(|&b| b as f32 / (QA * QB) as f32)
            .collect(),
    }
}

fn quantize(p: &Params) -> Network {
    let q = |v: f32, s: i32| (v * s as f32).round().clamp(-32767.0, 32767.0) as i16;
    Network {
        hidden: p.h,
        buckets: p.nb,
        ft_weights: p.ft_w.iter().map(|&w| q(w, QA)).collect(),
        ft_bias: p.ft_b.iter().map(|&w| q(w, QA)).collect(),
        out_weights: p.out_w.iter().map(|&w| q(w, QB)).collect(),
        out_bias: p
            .out_b
            .iter()
            .map(|&b| (b * (QA * QB) as f32).round() as i32)
            .collect(),
    }
}

fn load(paths: &str, wdl: f32, buckets: usize, eval_scale: f32) -> Vec<Sample> {
    let mut samples = Vec::new();
    for path in paths.split(',') {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        for rec in bytes.chunks_exact(RECORD_SIZE) {
            let r = unpack(rec);
            if r.pieces.len() > 32 {
                continue;
            }
            let mut s = Sample {
                features: [[0; 32]; 2],
                count: r.pieces.len() as u8,
                bucket: (((r.pieces.len() - 2) * buckets / 31).min(buckets - 1)) as u8,
                target: 0.0,
            };
            // features[0] = side to move perspective, [1] = other side.
            for (i, &(code, sq)) in r.pieces.iter().enumerate() {
                s.features[0][i] = feature(r.stm, code, sq) as u16;
                s.features[1][i] = feature(r.stm ^ 1, code, sq) as u16;
            }
            let score = if r.stm == 0 { r.score } else { -r.score } as f32;
            let result = r.result as f32 / 2.0;
            let result = if r.stm == 0 { result } else { 1.0 - result };
            let wdl_target = 1.0 / (1.0 + (-score / eval_scale).exp());
            s.target = wdl * result + (1.0 - wdl) * wdl_target;
            samples.push(s);
        }
    }
    samples
}

/// Forward + backward over `batch`, accumulating gradients into `g`. Returns loss sum.
fn gradient(p: &Params, batch: &[Sample], g: &mut Params, k: f32) -> f32 {
    let h = p.h;
    let mut acc = vec![0.0f32; 2 * h];
    let mut dacc = vec![0.0f32; 2 * h];
    let mut loss = 0.0;
    for s in batch {
        for side in 0..2 {
            let a = &mut acc[side * h..(side + 1) * h];
            a.copy_from_slice(&p.ft_b);
            for &f in &s.features[side][..s.count as usize] {
                let row = &p.ft_w[f as usize * h..(f as usize + 1) * h];
                for (x, w) in a.iter_mut().zip(row) {
                    *x += w;
                }
            }
        }
        let b = s.bucket as usize;
        let ow = &p.out_w[b * 2 * h..(b + 1) * 2 * h];
        let mut out = p.out_b[b];
        for i in 0..2 * h {
            let c = acc[i].clamp(0.0, 1.0);
            out += c * c * ow[i];
        }
        let pred = 1.0 / (1.0 + (-out * k).exp());
        let err = pred - s.target;
        loss += err * err;
        let dout = 2.0 * err * pred * (1.0 - pred) * k;
        g.out_b[b] += dout;
        let gow = &mut g.out_w[b * 2 * h..(b + 1) * 2 * h];
        for i in 0..2 * h {
            let x = acc[i];
            let c = x.clamp(0.0, 1.0);
            gow[i] += dout * c * c;
            dacc[i] = if x > 0.0 && x < 1.0 {
                dout * ow[i] * 2.0 * c
            } else {
                0.0
            };
        }
        for side in 0..2 {
            let d = &dacc[side * h..(side + 1) * h];
            for (gb, dv) in g.ft_b.iter_mut().zip(d) {
                *gb += dv;
            }
            for &f in &s.features[side][..s.count as usize] {
                let row = &mut g.ft_w[f as usize * h..(f as usize + 1) * h];
                for (gw, dv) in row.iter_mut().zip(d) {
                    *gw += dv;
                }
            }
        }
    }
    loss
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = arg(&args, "data").expect("data=FILE[,FILE...]");
    let out = arg(&args, "out").unwrap_or_else(|| "net.nnue".into());
    let num = |k: &str, d: f64| arg(&args, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let h = num("hidden", 256.0) as usize;
    let nb = num("buckets", 8.0) as usize;
    let epochs = num("epochs", 10.0) as usize;
    let batch_size = num("batch", 16384.0) as usize;
    let lr0 = num("lr", 0.001) as f32;
    let wdl = num("wdl", 0.3) as f32;
    let threads = num("threads", 4.0) as usize;
    let val_frac = num("val", 0.02);
    // Centipawn scale of the win-probability sigmoid; fit it to the data
    // (Farzin self-play: ~160).
    let eval_scale = num("scale", 160.0) as f32;
    let k = NET_SCALE / eval_scale;
    let mut rng = Rng(num("seed", 12345.0) as u64 | 1);

    let t0 = Instant::now();
    let mut samples = load(&data, wdl, nb, eval_scale);
    // Shuffle (Fisher-Yates).
    for i in (1..samples.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        samples.swap(i, j);
    }
    let n_val = ((samples.len() as f64 * val_frac) as usize).min(200_000);
    let (val, train) = samples.split_at_mut(n_val);
    eprintln!(
        "loaded {} train / {} val samples in {:.1}s",
        train.len(),
        val.len(),
        t0.elapsed().as_secs_f32()
    );

    let mut p = match arg(&args, "init") {
        Some(path) => from_network(&Network::load(&path).expect("init network")),
        None => init(h, nb, &mut rng),
    };
    let mut m = p.zeros_like();
    let mut v = p.zeros_like();
    let (b1, b2, eps) = (0.9f32, 0.999f32, 1e-8f32);
    let mut step = 0i32;
    let total_steps = epochs * train.len().div_ceil(batch_size);
    let _ = p.len();
    for epoch in 0..epochs {
        // Reshuffle each epoch.
        for i in (1..train.len()).rev() {
            let j = (rng.next() % (i as u64 + 1)) as usize;
            train.swap(i, j);
        }
        let mut loss_sum = 0.0f64;
        let te = Instant::now();
        for batch in train.chunks(batch_size) {
            step += 1;
            // Cosine decay to 5% of the initial rate.
            let progress = step as f32 / total_steps as f32;
            let lr = lr0 * (0.05 + 0.95 * 0.5 * (1.0 + (std::f32::consts::PI * progress).cos()));
            let chunk = batch.len().div_ceil(threads);
            let (grads, losses): (Vec<Params>, Vec<f32>) = std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .chunks(chunk)
                    .map(|part| {
                        let p = &p;
                        scope.spawn(move || {
                            let mut g = p.zeros_like();
                            let l = gradient(p, part, &mut g, k);
                            (g, l)
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).unzip()
            });
            loss_sum += losses.iter().map(|&l| l as f64).sum::<f64>();
            let mut g = grads.into_iter();
            let mut total = g.next().unwrap();
            for other in g {
                for (dst, src) in total.slices_mut().into_iter().zip(other_slices(&other)) {
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d += s;
                    }
                }
            }
            let scale = 1.0 / batch.len() as f32;
            let bc1 = 1.0 - b1.powi(step);
            let bc2 = 1.0 - b2.powi(step);
            let pm = p.slices_mut();
            let mm = m.slices_mut();
            let vm = v.slices_mut();
            let gm = total.slices_mut();
            for (k, (((pw, mw), vw), gw)) in pm.into_iter().zip(mm).zip(vm).zip(gm).enumerate() {
                let clip = k == 0 || k == 2;
                for i in 0..pw.len() {
                    let gi = gw[i] * scale;
                    if gi == 0.0 && mw[i] == 0.0 {
                        continue;
                    }
                    mw[i] = b1 * mw[i] + (1.0 - b1) * gi;
                    vw[i] = b2 * vw[i] + (1.0 - b2) * gi * gi;
                    let upd = (mw[i] / bc1) / ((vw[i] / bc2).sqrt() + eps);
                    pw[i] -= lr * upd;
                    if clip {
                        pw[i] = pw[i].clamp(-WEIGHT_CLIP, WEIGHT_CLIP);
                    }
                }
            }
        }
        // Validation.
        let mut g = p.zeros_like();
        let vloss = gradient(&p, val, &mut g, k) / val.len().max(1) as f32;
        eprintln!(
            "epoch {:>3} train loss {:.6} val loss {:.6} ({:.0}s)",
            epoch + 1,
            loss_sum / train.len() as f64,
            vloss,
            te.elapsed().as_secs_f32()
        );
        std::fs::write(&out, quantize(&p).to_bytes()).unwrap();
    }
    eprintln!("wrote {out}");
}

fn other_slices(p: &Params) -> [&Vec<f32>; 4] {
    [&p.ft_w, &p.ft_b, &p.out_w, &p.out_b]
}
