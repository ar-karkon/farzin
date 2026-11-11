# فرزین | Farzin

فرزین یک موتور شطرنج UCI به زبان Rust است. از نسخهٔ ۰.۲ هستهٔ موتور کاملاً بازنویسی شده و دیگر به کتابخانهٔ `chess` وابسته نیست (آن کتابخانه فقط برای کد آزمایشی قدیمی Caissa در `src/caissa.rs` باقی مانده است).

گزارش مرحلهٔ ۷ / نسخهٔ ۰٫۴ (شبکهٔ مستقل FZN2، تقطیر از درخت جست‌وجو، آزمون‌های قابل‌اعتماد): [docs/v7-report.md](docs/v7-report.md). گزارش مرحلهٔ ۶ (استنتاج مستقل HalfKAv2_hm، هم‌طراحی جست‌وجو، تقطیر): [docs/v6-report.md](docs/v6-report.md). گزارش مرحلهٔ ۵: [docs/v5-report.md](docs/v5-report.md).

## وضعیت فعلی (برآورد محلی، نه رتبهٔ رسمی CCRL)

| نسخه | رتبه روی مقیاس `UCI_Elo` استاک‌فیش ۱۷.۱ در 10+0.1 | ۹۵٪ |
|---|---|---|
| main قدیمی `65ec72a` | حدود ۱۶۷۷ | [۱۵۹۲, ۱۷۵۲] |
| هستهٔ جدید + ارزیاب کلاسیک | حدود ۲۹۹۱ | [۲۹۳۴, ۳۰۴۳] |
| هستهٔ جدید + NNUE نسل ۱ | ارزیاب کلاسیک + ۵۰٫۸ | [+۱۶٫۸, +۸۵٫۹] |
| هستهٔ جدید + NNUE نسل ۲ (`nets/g2.nnue`) | نسل ۱ + ۱۶۲٫۴ | [+۱۱۸٫۱, +۲۱۲٫۴] |
| **۰٫۳: شبکهٔ CC0 `nn-ad9b42354671` با استنتاج مستقل (پیش‌فرض)** | g2 + ۷۶۷٫۶ | [+۵۷۷٫۹, ∞) |
| ۰٫۳ با EvalScale تنظیم‌شده، در برابر استاک‌فیش ۱۵٫۱ کامل (تک‌نخ) | −۴۸٫۱ | [−۷۸٫۵, −۱۸٫۳] |
| شبکهٔ تقطیرشدهٔ خود فرزین `nets/d1-512.nnue` (۷۶۸→۵۱۲، ۱۱٫۵M) | g2 + ۲۷۵٫۵ | [+۲۱۳٫۷, +۳۵۶٫۵] |
| **۰٫۴: شبکهٔ مستقل FZN2 `nets/fzn2-a1.nnue` (۴ king bucket ×۷۶۸، ۱۰۵M)** در 5+0.05 | d1-512 + ۱۶۴٫۳ | [+۱۲۳٫۲, +۲۱۰٫۲] |
| همان، در برابر شبکهٔ CC0 پیش‌فرض در 5+0.05 | −۲۳۰٫۲ | [−۲۶۵٫۲, −۱۹۸٫۸] |

## ساخت و اجرا

```sh
cargo build --release
./target/release/farzin            # حالت UCI
./target/release/farzin bench 12   # امضای گره و سرعت
cargo test --release
```

ساخت با `-C target-cpu=native` انجام می‌شود (`.cargo/config.toml`). شبکهٔ `nets/default.nnue` هنگام ساخت داخل باینری قرار می‌گیرد. این شبکه وزن CC0 منتشرشده توسط پروژهٔ استاک‌فیش است (منشأ و هش در `nets/README.md` و `THIRD_PARTY_NOTICES.md`)؛ استنتاج آن (`src/hka.rs`) مستقل نوشته شده است. شبکهٔ خودآموز قبلی در `nets/g2.nnue` است.

### گزینه‌های UCI

| گزینه | پیش‌فرض | توضیح |
|---|---|---|
| `Hash` | 16 | اندازهٔ جدول انتقال (MB) |
| `Threads` | 1 | Lazy SMP با جدول مشترک |
| `Move Overhead` | 20 | حاشیهٔ زمانی (ms) |
| `Clear Hash` | — | پاک کردن جدول و historyها |
| `EvalFile` | `<default>` | مسیر شبکهٔ FZN1، FZN2 یا HalfKAv2_hm (تشخیص خودکار)؛ `<empty>` = ارزیاب کلاسیک |
| `EvalScale` | 560 | تبدیل خروجی HalfKAv2_hm به سانتی‌پیاده (×/۱۰۲۴) |
| `EvalMaterialScale` | false | مقیاس‌دهی وابسته به مواد برای HalfKAv2_hm |
| `tune_*` | — | پارامترهای جست‌وجو برای SPSA |

فرمان‌های اضافه: `bench [depth]`، `go perft N`، `eval`، `d`.

## زنجیرهٔ ابزار

```sh
# تولید داده با self-play
./target/release/farzin datagen threads=4 games=50000 soft_nodes=4000 hard_nodes=12000 out=gen.data
# آموزش شبکه
./target/release/farzin-train data=gen.data out=net.nnue hidden=256 epochs=12 wdl=0.0 threads=4
# مسابقه و SPRT (نیازمند pip install chess)
python3 tools/arena.py --engine name=new,cmd=./target/release/farzin,opt.EvalFile=net.nnue \
    --engine name=base,cmd=./target/release/farzin --tc 10+0.1 --pairs 200 --sprt 0 5
# برآورد رتبه با لنگرهای ثابت
python3 tools/rating.py results/*.json --anchor sf2800=2800 --anchor sf3000=3000
# یک نسل کامل NNUE روی سرور
tools/pipeline.sh 2 20000000 64
# تقطیر ۰٫۴: نمونه‌برداری از درخت جست‌وجو + آموزش FZN2 با king bucket
./target/release/farzin datagen threads=4 games=30000 soft_nodes=2500 tree_rate=48 random_move_pm=50 out=tree.data
./target/release/farzin-train2 data=tree.data out=net.nnue hidden=768 kb=4m epochs=4 threads=4 resume=1
tools/distill2.sh 100000000 768
# دقت شبکه‌ها نسبت به برچسب معلم
cargo run --release --example net_mse -- quiet-val.data nets/fzn2-a1.nnue nets/d1-512.nnue
# پروتکل قدرت مطلق در چند TC در برابر موتورهای مرجع
tools/strength.sh target/release/farzin sf15=/path/to/stockfish15 sf17=/path/to/stockfish17
# تقطیر قدیمی (۰٫۳): برچسب ارزیابی ایستای شبکهٔ معلم، آموزش دانش‌آموز FZN1، غربال و SPRT
tools/distill.sh nets/default.nnue 20000000 512
# دریافت شبکه‌های CC0 دیگر با بررسی SHA-256
tools/fetch_nets.sh
```

ابزارهای قدیمی‌تر (`tools/match.py`، `tools/sprt.py`، `tools/bench.py` و …) برای بازتولید نتایج مراحل قبل نگه داشته شده‌اند.
