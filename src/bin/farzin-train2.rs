//! CPU trainer for Farzin FZN2 (king-bucketed) networks.
//!
//! farzin-train2 data=a.data[,b.data] out=net.fzn2 hidden=768 kb=16m obuckets=8 \
//!     epochs=12 batch=16384 lr=0.001 wdl=0.0 scale=400 threads=4 [resume=1]
//!
//! Model: `(KB x 768 -> H) x 2 -> 1 x OB`, SCReLU (see `src/fzn2.rs`).
//! Loss: MSE between sigmoid(out) and a blend of sigmoid(label / scale) and
//! the game result (`wdl`).
//!
//! Parallelism: the hidden layer is split into one slice per thread, and
//! every thread runs the whole batch on its own slice. Feature-transformer
//! gradients of different threads therefore never overlap, so there are no
//! per-thread gradient copies to zero and reduce — this is what makes
//! training 10M+ parameter king-bucket nets practical on a few CPU cores.
//! Only rows touched by the batch are updated (lazy Adam).
//!
//! Every epoch writes the quantized net to `out` and a float checkpoint to
//! `out.ckpt`; `resume=1` continues from the checkpoint (same arguments).
//!
//! Warm start: `init=old.nnue` dequantizes an existing FZN2 net and continues
//! from it, so every training run builds on all earlier compute. The new
//! model may *grow*: each new king bucket starts from the old bucket that
//! covers the same king squares, and extra hidden neurons start with zero
//! output weights, so the grown net computes exactly the old function
//! before the first step (up to quantization).
use farzin::datagen::{unpack, RECORD_SIZE};
use farzin::fzn2::{self, feature, king_key, output_bucket, QA, QB};
use std::io::{Read, Write};
use std::time::Instant;

const NET_SCALE: f32 = fzn2::SCALE as f32;
const WEIGHT_CLIP: f32 = 1.98;
const CKPT_MAGIC: &[u8; 8] = b"FZN2CKP1";

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{key}=")).map(str::to_string))
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

#[derive(Clone)]
struct Tensor {
    ft_w: Vec<f32>,
    ft_b: Vec<f32>,
    out_w: Vec<f32>,
    out_b: Vec<f32>,
}

impl Tensor {
    fn zeros(f: usize, h: usize, ob: usize) -> Tensor {
        Tensor {
            ft_w: vec![0.0; f * h],
            ft_b: vec![0.0; h],
            out_w: vec![0.0; ob * 2 * h],
            out_b: vec![0.0; ob],
        }
    }
    fn parts(&self) -> [&Vec<f32>; 4] {
        [&self.ft_w, &self.ft_b, &self.out_w, &self.out_b]
    }
    fn parts_mut(&mut self) -> [&mut Vec<f32>; 4] {
        [
            &mut self.ft_w,
            &mut self.ft_b,
            &mut self.out_w,
            &mut self.out_b,
        ]
    }
}

struct Model {
    h: usize,
    ob: usize,
    kb: usize,
    mirror: bool,
    layout: [u8; 64],
    p: Tensor,
    m: Tensor,
    v: Tensor,
    g: Tensor,
}

/// Decoded mini-batch.
struct Batch {
    feats: Vec<[u16; 64]>,
    count: Vec<u8>,
    bucket: Vec<u8>,
    target: Vec<f32>,
    touched: Vec<u32>,
}

fn decode(recs: &[&[u8]], model: &Model, wdl: f32, eval_scale: f32, mark: &mut [bool]) -> Batch {
    let n = recs.len();
    let mut b = Batch {
        feats: vec![[0u16; 64]; n],
        count: vec![0; n],
        bucket: vec![0; n],
        target: vec![0.0; n],
        touched: Vec::new(),
    };
    for (i, rec) in recs.iter().enumerate() {
        let r = unpack(rec);
        let cnt = r.pieces.len().min(32);
        let mut ksq = [0usize; 2];
        for &(code, sq) in &r.pieces {
            if code % 6 == 5 {
                ksq[code / 6] = sq;
            }
        }
        // Perspective 0 = side to move, 1 = other side.
        for (slot, persp) in [r.stm, r.stm ^ 1].into_iter().enumerate() {
            let (kb, mir) = king_key(&model.layout, model.mirror, persp, ksq[persp]);
            for (j, &(code, sq)) in r.pieces.iter().take(cnt).enumerate() {
                let f = feature(persp, kb, mir, code, sq);
                b.feats[i][slot * 32 + j] = f as u16;
                if !mark[f] {
                    mark[f] = true;
                    b.touched.push(f as u32);
                }
            }
        }
        b.count[i] = cnt as u8;
        b.bucket[i] = output_bucket(cnt, model.ob) as u8;
        let score = if r.stm == 0 { r.score } else { -r.score } as f32;
        let result = r.result as f32 / 2.0;
        let result = if r.stm == 0 { result } else { 1.0 - result };
        let soft = 1.0 / (1.0 + (-score / eval_scale).exp());
        b.target[i] = wdl * result + (1.0 - wdl) * soft;
    }
    for &f in &b.touched {
        mark[f as usize] = false;
    }
    b
}

/// Raw pointer wrapper so threads can write disjoint hidden slices.
#[derive(Clone, Copy)]
struct Ptr(*mut f32);
unsafe impl Send for Ptr {}
unsafe impl Sync for Ptr {}

#[inline(always)]
fn screlu(x: f32) -> f32 {
    let c = x.clamp(0.0, 1.0);
    c * c
}

/// Forward over hidden slice `lo..hi` for the whole batch. Writes the
/// slice's pre-activations to `acc` (`[n][2][hi-lo]`) and returns the
/// partial output sums.
fn forward_slice(model: &Model, batch: &Batch, lo: usize, hi: usize, acc: &mut [f32]) -> Vec<f32> {
    let h = model.h;
    let w = hi - lo;
    let n = batch.count.len();
    let mut partial = vec![0.0f32; n];
    for i in 0..n {
        let cnt = batch.count[i] as usize;
        let b = batch.bucket[i] as usize;
        let mut sum = 0.0f32;
        for side in 0..2 {
            let a = &mut acc[(i * 2 + side) * w..(i * 2 + side + 1) * w];
            a.copy_from_slice(&model.p.ft_b[lo..hi]);
            for &f in &batch.feats[i][side * 32..side * 32 + cnt] {
                let row = &model.p.ft_w[f as usize * h + lo..f as usize * h + hi];
                for (x, wv) in a.iter_mut().zip(row) {
                    *x += wv;
                }
            }
            let ow = &model.p.out_w[b * 2 * h + side * h + lo..b * 2 * h + side * h + hi];
            for (x, wv) in a.iter().zip(ow) {
                sum += screlu(*x) * wv;
            }
        }
        partial[i] = sum;
    }
    partial
}

fn split(h: usize, threads: usize) -> Vec<(usize, usize)> {
    let per = h.div_ceil(threads).div_ceil(8) * 8;
    (0..threads)
        .map(|t| ((t * per).min(h), ((t + 1) * per).min(h)))
        .filter(|(a, b)| a < b)
        .collect()
}

/// Full forward; returns (outputs, per-slice pre-activation buffers).
fn forward(
    model: &Model,
    batch: &Batch,
    slices: &[(usize, usize)],
    bufs: &mut [Vec<f32>],
) -> Vec<f32> {
    let n = batch.count.len();
    let partials: Vec<Vec<f32>> = std::thread::scope(|scope| {
        let hs: Vec<_> = slices
            .iter()
            .zip(bufs.iter_mut())
            .map(|(&(lo, hi), buf)| {
                buf.resize(n * 2 * (hi - lo), 0.0);
                scope.spawn(move || forward_slice(model, batch, lo, hi, buf))
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    (0..n)
        .map(|i| {
            partials.iter().map(|p| p[i]).sum::<f32>() + model.p.out_b[batch.bucket[i] as usize]
        })
        .collect()
}

struct Adam {
    lr: f32,
    bc1: f32,
    bc2: f32,
}
const B1: f32 = 0.9;
const B2: f32 = 0.999;
const EPS: f32 = 1e-8;

#[inline(always)]
fn adam(p: &mut f32, m: &mut f32, v: &mut f32, g: &mut f32, a: &Adam, clip: bool) {
    let gi = *g;
    *g = 0.0;
    *m = B1 * *m + (1.0 - B1) * gi;
    *v = B2 * *v + (1.0 - B2) * gi * gi;
    *p -= a.lr * (*m / a.bc1) / ((*v / a.bc2).sqrt() + EPS);
    if clip {
        *p = p.clamp(-WEIGHT_CLIP, WEIGHT_CLIP);
    }
}

/// Backward + Adam over one hidden slice. `dout[i]` = dLoss/dOutput / n.
#[allow(clippy::too_many_arguments)]
unsafe fn backward_slice(
    h: usize,
    ob: usize,
    ptrs: [[Ptr; 4]; 4],
    batch: &Batch,
    dout: &[f32],
    acc: &[f32],
    lo: usize,
    hi: usize,
    adam_p: &Adam,
) {
    let w = hi - lo;
    let [pp, mp, vp, gp] = ptrs;
    let g_ft = gp[0].0;
    let g_ftb = gp[1].0;
    let g_out = gp[2].0;
    let mut d = vec![0.0f32; w];
    for i in 0..batch.count.len() {
        let cnt = batch.count[i] as usize;
        let b = batch.bucket[i] as usize;
        let go = dout[i];
        if go == 0.0 {
            continue;
        }
        for side in 0..2 {
            let a = &acc[(i * 2 + side) * w..(i * 2 + side + 1) * w];
            let ow_off = b * 2 * h + side * h + lo;
            let ow = std::slice::from_raw_parts(pp[2].0.add(ow_off), w);
            for j in 0..w {
                let x = a[j];
                let c = x.clamp(0.0, 1.0);
                *g_out.add(ow_off + j) += go * c * c;
                d[j] = if x > 0.0 && x < 1.0 {
                    go * ow[j] * 2.0 * c
                } else {
                    0.0
                };
                *g_ftb.add(lo + j) += d[j];
            }
            for &f in &batch.feats[i][side * 32..side * 32 + cnt] {
                let row = g_ft.add(f as usize * h + lo);
                for (j, dv) in d.iter().enumerate() {
                    *row.add(j) += dv;
                }
            }
        }
    }
    // Adam on touched rows of this slice, the bias slice and the output
    // weights of this slice.
    for &f in &batch.touched {
        let off = f as usize * h + lo;
        for j in 0..w {
            adam(
                &mut *pp[0].0.add(off + j),
                &mut *mp[0].0.add(off + j),
                &mut *vp[0].0.add(off + j),
                &mut *gp[0].0.add(off + j),
                adam_p,
                true,
            );
        }
    }
    for j in lo..hi {
        adam(
            &mut *pp[1].0.add(j),
            &mut *mp[1].0.add(j),
            &mut *vp[1].0.add(j),
            &mut *gp[1].0.add(j),
            adam_p,
            false,
        );
    }
    for b in 0..ob {
        for side in 0..2 {
            let off = b * 2 * h + side * h + lo;
            for j in 0..w {
                adam(
                    &mut *pp[2].0.add(off + j),
                    &mut *mp[2].0.add(off + j),
                    &mut *vp[2].0.add(off + j),
                    &mut *gp[2].0.add(off + j),
                    adam_p,
                    true,
                );
            }
        }
    }
}

fn loss_and_grad(batch: &Batch, out: &[f32], k: f32) -> (f32, Vec<f32>) {
    let n = out.len() as f32;
    let mut loss = 0.0;
    let dout = out
        .iter()
        .zip(&batch.target)
        .map(|(&o, &t)| {
            let pred = 1.0 / (1.0 + (-o * k).exp());
            let err = pred - t;
            loss += err * err;
            2.0 * err * pred * (1.0 - pred) * k / n
        })
        .collect();
    (loss, dout)
}

fn quantize(model: &Model) -> fzn2::Network {
    let q = |v: f32, s: i32| (v * s as f32).round().clamp(-32767.0, 32767.0) as i16;
    fzn2::Network {
        hidden: model.h,
        out_buckets: model.ob,
        king_buckets: model.kb,
        mirror: model.mirror,
        layout: model.layout,
        ft_weights: model.p.ft_w.iter().map(|&w| q(w, QA)).collect(),
        ft_bias: model.p.ft_b.iter().map(|&w| q(w, QA)).collect(),
        out_weights: model.p.out_w.iter().map(|&w| q(w, QB)).collect(),
        out_bias: model
            .p
            .out_b
            .iter()
            .map(|&b| (b * (QA * QB) as f32).round() as i32)
            .collect(),
    }
}

fn write_ckpt(path: &str, model: &Model, epoch: usize, step: i32, rng: u64) {
    let mut out = Vec::new();
    out.extend_from_slice(CKPT_MAGIC);
    for v in [epoch as u64, step as u64, rng] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for t in [&model.p, &model.m, &model.v] {
        for part in t.parts() {
            for x in part.iter() {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    let tmp = format!("{path}.tmp");
    std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(&out))
        .expect("write checkpoint");
    std::fs::rename(&tmp, path).expect("rename checkpoint");
}

fn read_ckpt(path: &str, model: &mut Model) -> Option<(usize, i32, u64)> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() < 32 || &bytes[..8] != CKPT_MAGIC {
        return None;
    }
    let u = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
    let (epoch, step, rng) = (u(8) as usize, u(16) as i32, u(24));
    let mut off = 32;
    let total: usize = model.p.parts().iter().map(|p| p.len()).sum::<usize>() * 3;
    if bytes.len() != off + 4 * total {
        eprintln!("checkpoint shape mismatch, ignoring {path}");
        return None;
    }
    for t in [&mut model.p, &mut model.m, &mut model.v] {
        for part in t.parts_mut() {
            for x in part.iter_mut() {
                *x = f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
                off += 4;
            }
        }
    }
    Some((epoch, step, rng))
}

/// Initialise `model` from a quantized FZN2 net, growing buckets/width.
fn warm_start(model: &mut Model, old: &fzn2::Network, rng: &mut Rng) {
    assert_eq!(
        old.out_buckets, model.ob,
        "init net must have the same output buckets"
    );
    assert!(old.hidden <= model.h, "init net is wider than the model");
    assert!(
        old.mirror == model.mirror || old.king_buckets == 1,
        "init net mirroring differs"
    );
    let (h, oh) = (model.h, old.hidden);
    // Old bucket for every new bucket: the first king square mapped to it.
    let mut parent = vec![usize::MAX; model.kb];
    for sq in 0..64 {
        if model.mirror && sq % 8 >= 4 {
            continue;
        }
        let nb = model.layout[sq] as usize;
        if parent[nb] == usize::MAX {
            parent[nb] = if old.king_buckets == 1 {
                0
            } else {
                old.layout[sq] as usize
            };
        }
    }
    let qa = QA as f32;
    let qb = QB as f32;
    for (nb, &ob) in parent.iter().enumerate() {
        let ob = if ob == usize::MAX { 0 } else { ob };
        for i in 0..768 {
            let dst = (nb * 768 + i) * h;
            let src = (ob * 768 + i) * oh;
            for j in 0..h {
                model.p.ft_w[dst + j] = if j < oh {
                    f32::from(old.ft_weights[src + j]) / qa
                } else {
                    // Fresh neurons: small random inputs, zero output weight.
                    (rng.uniform() * 2.0 - 1.0) * 0.05
                };
            }
        }
    }
    for j in 0..h {
        model.p.ft_b[j] = if j < oh {
            f32::from(old.ft_bias[j]) / qa
        } else {
            0.0
        };
    }
    for b in 0..model.ob {
        for side in 0..2 {
            for j in 0..h {
                model.p.out_w[b * 2 * h + side * h + j] = if j < oh {
                    f32::from(old.out_weights[b * 2 * oh + side * oh + j]) / qb
                } else {
                    0.0
                };
            }
        }
        model.p.out_b[b] = old.out_bias[b] as f32 / (qa * qb);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = arg(&args, "data").expect("data=FILE[,FILE...]");
    let out = arg(&args, "out").unwrap_or_else(|| "net.fzn2".into());
    let num = |k: &str, d: f64| arg(&args, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let h = num("hidden", 768.0) as usize;
    let ob = num("obuckets", 8.0) as usize;
    let kb_name = arg(&args, "kb").unwrap_or_else(|| "16m".into());
    let (layout, kb, mirror) = fzn2::layout(&kb_name).expect("kb= one of 1,1m,4m,16m,32m");
    let epochs = num("epochs", 12.0) as usize;
    let batch_size = num("batch", 16384.0) as usize;
    let lr0 = num("lr", 0.001) as f32;
    let lr_end = num("lr_end", 0.05) as f32;
    let wdl = num("wdl", 0.0) as f32;
    let threads = num("threads", 4.0) as usize;
    let eval_scale = num("scale", 400.0) as f32;
    let max_samples = num("max", 0.0) as usize;
    let val_n = num("val", 200_000.0) as usize;
    let k = NET_SCALE / eval_scale;
    let mut rng = Rng(num("seed", 12345.0) as u64 | 1);
    assert!(h.is_multiple_of(32), "hidden must be a multiple of 32");

    // Load raw 32-byte records (decoded per batch, so 100M positions fit
    // in ~3.2 GB).
    let t0 = Instant::now();
    let mut raw = Vec::new();
    for path in data.split(',') {
        let mut f = std::fs::File::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        f.read_to_end(&mut raw).unwrap();
        raw.truncate(raw.len() / RECORD_SIZE * RECORD_SIZE);
    }
    let mut n = raw.len() / RECORD_SIZE;
    if max_samples > 0 && n > max_samples {
        n = max_samples;
    }
    let mut order: Vec<u32> = (0..n as u32).collect();
    for i in (1..order.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    let n_val = val_n.min(n / 20);
    let (val_idx, train_idx) = order.split_at_mut(n_val);
    let val_idx = val_idx.to_vec();
    let mut train_idx = train_idx.to_vec();
    drop(order);
    eprintln!(
        "loaded {} train / {} val positions in {:.1}s; model {}x768x{} -> 1x{}{}",
        train_idx.len(),
        val_idx.len(),
        t0.elapsed().as_secs_f32(),
        kb,
        h,
        ob,
        if mirror { " (mirrored)" } else { "" }
    );
    let rec = |i: u32| &raw[i as usize * RECORD_SIZE..(i as usize + 1) * RECORD_SIZE];

    let f_count = kb * 768;
    let mut model = Model {
        h,
        ob,
        kb,
        mirror,
        layout,
        p: Tensor::zeros(f_count, h, ob),
        m: Tensor::zeros(f_count, h, ob),
        v: Tensor::zeros(f_count, h, ob),
        g: Tensor::zeros(f_count, h, ob),
    };
    let ft_scale = (1.0 / 32.0f32).sqrt();
    let out_scale = (1.0 / (2.0 * h as f32)).sqrt();
    for w in model.p.ft_w.iter_mut() {
        *w = (rng.uniform() * 2.0 - 1.0) * ft_scale;
    }
    for w in model.p.out_w.iter_mut() {
        *w = (rng.uniform() * 2.0 - 1.0) * out_scale;
    }
    if let Some(init) = arg(&args, "init") {
        let bytes = std::fs::read(&init).unwrap_or_else(|e| panic!("{init}: {e}"));
        let old = fzn2::Network::from_bytes(&bytes).unwrap_or_else(|e| panic!("{init}: {e}"));
        warm_start(&mut model, &old, &mut rng);
        eprintln!(
            "warm start from {init}: {}x768x{} -> {}x768x{}",
            old.king_buckets, old.hidden, kb, h
        );
    }
    let ckpt = format!("{out}.ckpt");
    let mut start_epoch = 0;
    let mut step = 0i32;
    if num("resume", 0.0) != 0.0 {
        if let Some((e, s, r)) = read_ckpt(&ckpt, &mut model) {
            eprintln!("resumed from {ckpt} after epoch {e}");
            start_epoch = e;
            step = s;
            rng = Rng(r);
        }
    }

    if epochs == 0 {
        // Conversion only (e.g. verify or materialise a grown warm start).
        std::fs::write(&out, quantize(&model).to_bytes()).unwrap();
        eprintln!("wrote {out} (no training)");
        return;
    }
    let slices = split(h, threads);
    let mut bufs: Vec<Vec<f32>> = vec![Vec::new(); slices.len()];
    let mut mark = vec![false; f_count];
    let total_steps = (epochs * train_idx.len().div_ceil(batch_size)) as f32;

    // Fixed validation batches.
    let val_batches: Vec<Batch> = val_idx
        .chunks(batch_size)
        .map(|c| {
            let recs: Vec<&[u8]> = c.iter().map(|&i| rec(i)).collect();
            decode(&recs, &model, wdl, eval_scale, &mut mark)
        })
        .collect();

    for epoch in start_epoch..epochs {
        for i in (1..train_idx.len()).rev() {
            let j = (rng.next() % (i as u64 + 1)) as usize;
            train_idx.swap(i, j);
        }
        let te = Instant::now();
        let mut loss_sum = 0.0f64;
        for chunk in train_idx.chunks(batch_size) {
            step += 1;
            let progress = step as f32 / total_steps;
            let lr = lr0
                * (lr_end
                    + (1.0 - lr_end)
                        * 0.5
                        * (1.0 + (std::f32::consts::PI * progress.min(1.0)).cos()));
            let recs: Vec<&[u8]> = chunk.iter().map(|&i| rec(i)).collect();
            let batch = decode(&recs, &model, wdl, eval_scale, &mut mark);
            let outs = forward(&model, &batch, &slices, &mut bufs);
            let (loss, dout) = loss_and_grad(&batch, &outs, k);
            loss_sum += loss as f64;
            let adam_p = Adam {
                lr,
                bc1: 1.0 - B1.powi(step),
                bc2: 1.0 - B2.powi(step),
            };
            // Output bias (shared by all slices): handled here.
            for (i, &d) in dout.iter().enumerate() {
                model.g.out_b[batch.bucket[i] as usize] += d;
            }
            for b in 0..ob {
                let (mut p, mut m, mut v, mut g) = (
                    model.p.out_b[b],
                    model.m.out_b[b],
                    model.v.out_b[b],
                    model.g.out_b[b],
                );
                adam(&mut p, &mut m, &mut v, &mut g, &adam_p, false);
                model.p.out_b[b] = p;
                model.m.out_b[b] = m;
                model.v.out_b[b] = v;
                model.g.out_b[b] = g;
            }
            let ptr = |t: &mut Tensor| {
                let [a, b, c, d] = t.parts_mut();
                [
                    Ptr(a.as_mut_ptr()),
                    Ptr(b.as_mut_ptr()),
                    Ptr(c.as_mut_ptr()),
                    Ptr(d.as_mut_ptr()),
                ]
            };
            let ptrs = [
                ptr(&mut model.p),
                ptr(&mut model.m),
                ptr(&mut model.v),
                ptr(&mut model.g),
            ];
            let (mh, mob) = (model.h, model.ob);
            std::thread::scope(|scope| {
                for (&(lo, hi), buf) in slices.iter().zip(bufs.iter()) {
                    let (batch, dout, adam_p) = (&batch, &dout, &adam_p);
                    scope.spawn(move || unsafe {
                        // SAFETY: every thread reads the shared parameters
                        // only within its own hidden slice and the output
                        // weights it writes are in that slice too, so no
                        // two threads access the same element mutably.
                        backward_slice(mh, mob, ptrs, batch, dout, buf, lo, hi, adam_p)
                    });
                }
            });
        }
        let mut vloss = 0.0f64;
        let mut vn = 0usize;
        for vb in &val_batches {
            let outs = forward(&model, vb, &slices, &mut bufs);
            vloss += loss_and_grad(vb, &outs, k).0 as f64;
            vn += vb.count.len();
        }
        let secs = te.elapsed().as_secs_f32();
        eprintln!(
            "epoch {:>3} train loss {:.6} val loss {:.6} ({:.0}s, {:.0} pos/s)",
            epoch + 1,
            loss_sum / train_idx.len() as f64,
            vloss / vn.max(1) as f64,
            secs,
            train_idx.len() as f32 / secs
        );
        std::fs::write(&out, quantize(&model).to_bytes()).unwrap();
        write_ckpt(&ckpt, &model, epoch + 1, step, rng.0);
    }
    eprintln!("wrote {out}");
}
