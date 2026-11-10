# Phase 4 — اعتبارسنجی NNUE واقعی (Milestone A): **مسدود**

## خلاصه

Milestone A در این اجرا **انجام نشد** و هیچ نتیجهٔ پذیرشی برای مدل آموزش‌دیده ادعا نمی‌شود. هیچ فایل `.pnn` یا `.nnue` در محیط محلی موجود نبود (جست‌وجوی کامل فایل‌سیستم در ۲۰۲۶-۱۱-۰۹). مخزن [Witek902/Caissa-Nets](https://github.com/Witek902/Caissa-Nets) در شاخهٔ پیش‌فرض همچنان فایل `LICENSE`، `LICENSE.md` یا `LICENSE.txt` ندارد (هر سه پاسخ HTTP 404 دادند) و README آن فقط می‌گوید «Repository with neural nets for Caissa chess engine». مجوز MIT کد Caissa خودبه‌خود حق توزیع یا استفادهٔ وزن‌های جداگانه را نمی‌دهد. طبق دستور پروژه، وزن‌ها دانلود، اجرا یا commit نشدند.

## آنچه از Phaseهای قبل معتبر است

- `docs/nnue-investigation.md`: در Phase 2 برای چهار موقعیت، تمام ۳۲ خروجی خام (۸ bucket × ۴ موقعیت) اسکالر فرزین با برنامهٔ مرجع Caissa `395c2e6` و مدل `eval-ml-11a-100B-spsa-transplant.pnn` (۷۵٬۷۰۳٬۸۷۲ بایت) برابر گزارش شده است. آن اجرا در محیط دیگری بوده و در این Phase تکرار نشد.
- Phase 3: برابری AVX2 و scalar و stack و cache فقط با وزن‌های **مصنوعی** و مرجع مستقل پایتون نشان داده شده است.

## آنچه هنوز اثبات نشده

- برابری AVX2 با مرجع Caissa روی مدل واقعی.
- برابری accumulator افزایشی و بازسازی کامل روی مدل واقعی در دنباله‌های طولانی، قلعه، en passant، همهٔ ترفیع‌ها و تغییر king bucket.
- throughput واقعی scalar/AVX2/cache/stack و سهم NNUE از زمان جست‌وجو.
- اینکه stack از cache سریع‌تر باشد. تا آن زمان پیکربندی آزمایشی توصیه‌شده همان **cache + AVX2** باقی می‌ماند و NNUE پیش‌فرض نمی‌شود.

## تغییر Phase 4 که به NNUE مربوط است

null move یک Board جدید push می‌کند. در ساخت `--features nnue-stack` آزمون `nnue_stack_search_cutoffs_and_interruption_restore_root` اکنون با `Persistent History + LMR + Null Move` و عمق ۵ نیز اجرا می‌شود و امضای (نتیجه، گره‌ها) stack و cache در چهار محدودیت گره برابر است؛ وضعیت ریشه بعد از قطع بازگردانده می‌شود. وزن این آزمون مصنوعی است.

## مراحل لازم برای رفع انسداد

۱. مالک پروژه حق استفادهٔ محلی از یک فایل مشخص Caissa-Nets را از نویسندهٔ Caissa یا مجوز صریح منتشرشده تأیید کند.
۲. فایل خارج از Git نگه داشته و SHA-256 آن ثبت شود (`python3 tools/inspect_nnue.py model.pnn`).
۳. اجرای ابزارهای موجود:

```sh
cargo build --release --example caissa_probe
python3 tools/validate_caissa.py model.pnn /path/to/caissa target/release/examples/caissa_probe --json validation.json
python3 tools/compare_caissa_accumulators.py model.pnn /path/to/caissa
python3 tools/bench_v3.py target/release/farzin --commit $(git rev-parse HEAD) --network-kind trained \
  --option EvalFile=model.pnn --option "NNUE SIMD=true" --json bench-trained-avx2.json
```

۴. فقط پس از برابری خروجی خام و بنچمارک، مسابقهٔ جفتی کلاسیک در برابر NNUE با `tools/sharded_match.py`.
