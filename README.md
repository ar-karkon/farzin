# فرزین | Farzin

فرزین یک موتور شطرنج UCI به زبان Rust است. از نسخهٔ ۰.۲ هستهٔ موتور کاملاً بازنویسی شده و دیگر به کتابخانهٔ `chess` وابسته نیست (آن کتابخانه فقط برای کد آزمایشی قدیمی Caissa در `src/caissa.rs` باقی مانده است).

گزارش کامل این مرحله، نتایج اندازه‌گیری و راهنمای اجرا روی سرور: [docs/v5-report.md](docs/v5-report.md).

## وضعیت فعلی (برآورد محلی، نه رتبهٔ رسمی CCRL)

| نسخه | رتبه روی مقیاس `UCI_Elo` استاک‌فیش ۱۷.۱ در 10+0.1 | ۹۵٪ |
|---|---|---|
| main قدیمی `65ec72a` | حدود ۱۶۷۷ | [۱۵۹۲, ۱۷۵۲] |
| هستهٔ جدید + ارزیاب کلاسیک | حدود ۲۹۹۱ | [۲۹۳۴, ۳۰۴۳] |
| هستهٔ جدید + NNUE نسل ۱ | ارزیاب کلاسیک + ۵۰٫۸ | [+۱۶٫۸, +۸۵٫۹] |
| هستهٔ جدید + NNUE نسل ۲ (پیش‌فرض) | نسل ۱ + ۱۶۲٫۴ | [+۱۱۸٫۱, +۲۱۲٫۴] |

## ساخت و اجرا

```sh
cargo build --release
./target/release/farzin            # حالت UCI
./target/release/farzin bench 12   # امضای گره و سرعت
cargo test --release
```

ساخت با `-C target-cpu=native` انجام می‌شود (`.cargo/config.toml`). شبکهٔ `nets/default.nnue` هنگام ساخت داخل باینری قرار می‌گیرد.

### گزینه‌های UCI

| گزینه | پیش‌فرض | توضیح |
|---|---|---|
| `Hash` | 16 | اندازهٔ جدول انتقال (MB) |
| `Threads` | 1 | Lazy SMP با جدول مشترک |
| `Move Overhead` | 20 | حاشیهٔ زمانی (ms) |
| `Clear Hash` | — | پاک کردن جدول و historyها |
| `EvalFile` | `<default>` | مسیر شبکهٔ FZN1؛ `<empty>` = ارزیاب کلاسیک |
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
```

ابزارهای قدیمی‌تر (`tools/match.py`، `tools/sprt.py`، `tools/bench.py` و …) برای بازتولید نتایج مراحل قبل نگه داشته شده‌اند.
