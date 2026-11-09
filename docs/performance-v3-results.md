# گزارش Phase 3 — اجرای مهندسی عملکرد

مبنای واقعی مخزن `efdb2307bded597dbf8c3fbcb01e9d33de3c4c29` بود؛ هر دو شاخهٔ موجود همین head را داشتند. شاخهٔ توسعه `feat/performance-search-v3` است. کد نهایی موتور در `e182b9cd1384e8b94a077e6634251c2377c68057` ثبت شده؛ اصلاح provenance ابزار در `c3be09e` است. این گزارش همراه داده‌ها در commit تحویل ثبت می‌شود. main تغییر نکرده است.

## وضعیت Milestoneها

- **۱ — Accumulator:** پیاده‌سازی lazy per-ply، اعتبار cache، نگه‌داری Board و دو accumulator، استخراج dirty piece از bitboardها، refresh هنگام تغییر king bucket/mirror و بازگشت امن انجام شد. refresh و delta مسیر Caissa تخصیص heap ندارند. شبکه فقط با Arc مشترک است. تست صحت مصنوعی گذشت؛ سود سرعتِ stack اسکالر روشن نبود. اتصال به search فقط با feature ساخت `nnue-stack` و سپس `NNUE Stack=true` فعال می‌شود. معیار پذیرش با مدل واقعی هنوز کامل نیست.
- **۲ — SIMD:** AVX2 لایه‌های مخفی کوانتیزه با تشخیص CPU در runtime، load/store بدون شرط alignment، حفظ wrapping و scalar fallback پیاده شد. `NNUE SIMD` مستقل و پیش‌فرض خاموش است. SSE و NEON پیاده/آزمایش نشده‌اند. صحت با scalar و مرجع مستقل پایتون گذشت؛ مرجع اجرایی Caissa و مدل آموزش‌دیده در این محیط موجود نبودند.
- **۳ — Search improvements:** PVS قبلی حفظ شد و پیش‌فرض خاموش است. LMR، null move، history aging/persistence و TT پایدار اضافه نشدند. گزارش ممیزی مسیرها و خطرها در `docs/performance-audit.md` است. هدف این اجرا انباشت هم‌زمان تغییرات جست‌وجو نبود.
- **۴ — Parallel:** تغییری در الگوریتم موازی یا انتخاب نخ برنده ایجاد نشد. shared TT و آزمون مقیاس‌پذیری ۱/۲/۴/۸ انجام نشدند. فقط صحت توقف/جایگزینی NNUE با Threads=2 آزموده شد؛ این آزمون قدرت یا scalability نیست.
- **۵ — CI/validation:** CI برای ساخت عادی و feature آزمایشی، ابزار بنچمارک تکرارشونده، مقایسهٔ signatureها، ثبت metadata و مرجع مستقل اضافه شدند. مسابقات، external engines، confidence interval قدرت و SPRT انجام نشده‌اند؛ این Milestone از نظر رقابتی کامل نیست.

## روش و محیط

Release، Rust 1.99.1، Intel Xeon Platinum 8573C، یک نخ، Hash=16 MiB، affinity روی CPU 0، Linux و پنج تکرار روی ۲۲ موقعیت دستی متنوع. محیط سه CPU منطقی و quota معادل دو CPU دارد. SHA کامل موتور/باینری/شبکه، ویژگی‌های CPU، OS، تنظیمات، امتیاز، حرکت، PV، گره و زمان هر موقعیت در JSONها هستند. perf در دسترس نبود؛ cycles/node و cache/branch counters اندازه‌گیری نشده‌اند.

**هیچ وزن Caissa دانلود یا توزیع نشد.** مدل‌های محلی، مصنوعی و آموزش‌ندیده‌اند. fixture عملکرد، hidden arithmetic متراکم و output weights صفر دارد تا امتیاز دلخواهِ مدل آموزش‌ندیده درخت جست‌وجوی بسیار پرهزینه نسازد. دو اجرای اکتشافی ناتمام با خروجی غیرصفر در فایل‌های `aborted-depth*.json` ثبت شده‌اند و زمان ناقص‌شان نتیجهٔ موفق محسوب نشده است. fixture صحت خروجی غیرصفر و وزن‌های تمام laneها را دارد. این اعداد نمایندهٔ مدل آموزش‌دیده یا distribution ورودی‌های sparse آن نیستند.

## زمان قبل/بعد

جدول، میانهٔ مجموع wall time جست‌وجوی ۲۲ موقعیت را نشان می‌دهد؛ بارگذاری شبکه پیش از زمان‌گیری و بعد از isready انجام شده است. NPS زیر از مجموع گره / میانهٔ زمان محاسبه شده و شامل startup هر go است. NPS و time گزارش‌شدهٔ خود موتور نیز در دادهٔ خام نگه‌داری شده‌اند.

| پیکربندی | عمق | مجموع گره | میانه ms | min–max ms | wall NPS |
|---|---:|---:|---:|---:|---:|
| Baseline NNUE scalar cache | 3 | 24,091 | 496.13 | 461.96–528.30 | 48,558 |
| Final experimental stack, scalar | 3 | 24,091 | 531.09 | 467.20–682.36 | 45,361 |
| Final experimental stack, AVX2 | 3 | 24,091 | 118.10 | 90.39–149.18 | 203,986 |
| Final default build cache, AVX2 opt-in | 3 | 24,091 | 103.18 | 92.73–116.68 | 233,481 |

در ساخت نهایی، نسبت میانهٔ stack scalar به stack AVX2 برابر **4.50×** بود. cache با AVX2 نسبت به baseline scalar در این workload مصنوعی **4.81×** نسبت میانه داشت. این نسبت‌ها ادعای سرعت شبکهٔ واقعی یا Elo نیستند. stack در برابر cache سود روشن و عمومی نشان نداد و پیش‌فرض فعال نشد.

در microbenchmark باینری `1866458`، ۱۰٬۰۰۰ ارزیابی در هر نمونه و پنج تکرار برای هر backend با ترتیب متناوب اجرا شد:

| حالت | scalar ms | AVX2 ms | نسبت میانه‌ها |
|---|---:|---:|---:|
| forward روی Board ثابت | 203.11 | 10.32 | 19.68× |
| full refresh | 294.61 | 102.65 | 2.87× |
| cache traversal | 269.93 | 28.39 | 9.51× |
| stack traversal | 220.04 | 29.48 | 7.46× |

forward ثابت به‌ترتیب حدود 49,236 و 969,115 evaluation/s داشت. workload حالت‌های مختلف micro یکسان نیست؛ فقط هر حالت را بین backendها مقایسه کنید. پراکندگی زیاد بود و min/max/stdev در JSON ثبت شده‌اند. پروفایل counterمحور و مدل واقعی برای تعیین bottleneck بعدی لازم‌اند. افزایش عمق در زمان ثابت یا افزایش قدرت بازی اندازه‌گیری نشده است.

بنچمارک متناوب کلاسیکِ نهایی، میانهٔ baseline را 2969.59 و ساخت پیش‌فرض نهایی را 2958.53 میلی‌ثانیه نشان داد. نسبت جفتیِ میانه 1.007 بود؛ با این نمونه و نویز، افزایش سرعت کلاسیک ثابت نشده است. تمام signatureهای جست‌وجو برابر بودند.

## آزمایش ردشده و اصلاح ابزار

تخصصی‌سازی root در `b1bf9c0` در آزمون متناوب بهبود نداشت و در `8b6dd01` revert شد؛ داده‌ها در `benchmarks/v3-integration/paired-classical.json` حفظ شده‌اند. در نتیجه hookهای آزمایشی از ساخت پیش‌فرض با feature ساخت حذف شدند. گزارشگر backend نیز ابتدا اولین پیام بارگذاری را نگه می‌داشت؛ با تغییر گزینه بعد از بارگذاری، این مقدار stale می‌شد. در `c3be09e` ثبت آخرین backend و transcript تنظیمات اصلاح و با آزمون regression پوشش داده شد. اجراهای AVX2 مرتبط از نو اجرا شدند؛ گزارش نهایی backend واقعی avx2 را تأیید می‌کند.

## صحت، CI، حافظه و مجوز

- ساخت پیش‌فرض: **۵۵ تست Rust**؛ feature `nnue-stack`: **۵۶ تست Rust**؛ **۱۲ تست Python** موفق. این‌ها ۱۱۱ تست مستقل نیستند؛ دو ساخت تست‌های مشترک دارند. fmt، clippy بدون هشدار، test با locked و release build موفق بودند. logهای baseline، مراحل و نهایی در پوشه‌های مربوط ذخیره‌اند.
- محاسبات مرجع مستقل پایتون برای **۵۲ موقعیت و هر هشت خروجی خام در هر دو backend درخواستی** بدون اختلاف بود. وزن این آزمون مصنوعی غیرصفر است؛ این آزمون جایگزین executable مرجع Caissa نیست. تست‌ها قلعهٔ هر دو رنگ/طرف، همهٔ ترفیع‌ها، en passant، mirror/bucket شاه، مسیرهای قانونی طولانی، sibling reuse، ارزیابی‌های skipped، cutoff و interruption را پوشش می‌دهند.
- تمام runهای مقایسه‌شدهٔ NNUE در هر مجموعه، روی گره نهایی، امتیاز، حرکت و PV هر iteration برابر بودند؛ `equivalence.json`ها hash گزارش‌های ورودی را دارند. تطابق فعلیِ خروجی مدل واقعی با Caissa هنوز اثبات نشده است.
- تست UCI با stack+AVX2 و دو نخ، توقف در جست‌وجو، یک bestmove، جایگزینی search، تغییر backend پس از join و شروع مجدد را گذراند. Thread Sanitizer اجرا نشد. TT همچنان خصوصی هر worker است؛ shared mutable NNUE اضافه نشده است. Network فقط پس از join و با Arc::get_mut تغییر backend می‌دهد.
- ۱۲۸ frame از ابتدا تخصیص می‌یابد؛ هر frame دارای ۶۱۴۴ بایت accumulator به‌علاوه Board و validity است. make/unmake فقط index/Board/validity را تغییر می‌دهد؛ هنگام materialization کپی accumulator انجام می‌شود. این توضیح ساختاری است؛ RSS یا scaling حافظه در این اجرا بنچمارک نشده است.
- مجوز MIT مرجع Caissa در notices حفظ شد. کد موتور GPL/AGPL وارد Rust نشد. python-chess، وابستگی اختیاری ابزار آزمون و GPL-3.0-or-later است و vendored نیست. مجوز وزن واقعی روشن نیست؛ مخزن نیز LICENSE عمومی برای کل پروژه ندارد. انتخاب مجوز پروژه و تأیید حقوق وزن هنوز لازم است.
- workflowهای `.github/workflows/ci.yml` تست اصلی بدون مدل خارجی و تست feature را اجرا می‌کنند. وضعیت remote CI را در PR بررسی کنید؛ این گزارش نتیجهٔ اجرای محلی را بیان می‌کند.

## بازتولید و گام بعد

دستورها، مدل مصنوعی و گزینه‌ها در `docs/performance-v3.md`، ابزارها در `tools/` و داده‌های خام در `benchmarks/v3-*` هستند. برای engine با feature، `--build-features nnue-stack` را در runner ثبت کنید. source SHA موتور را صریح بدهید؛ tool hash و binary hash نیز ثبت می‌شوند. دو مدل مصنوعی با `tools/synthetic_network.py`، با و بدون `--neutral-output`، قابل بازسازی‌اند.

۱. یک وزن مجاز Caissa v15 و executable مرجع مشخص فراهم و raw outputs/accumulators را روی مجموعهٔ گسترده و همان SHA مقایسه کنید؛ `validate_caissa.py` و probe هر دو backend را پشتیبانی می‌کنند. هیچ بارگیری پیش‌فرض یا پذیرش مجوز ضمنی اضافه نشده است.
۲. با مدل واقعی، profile مسیر refresh، dirty update، input activation و memory copying را پس از SIMD بگیرید. تفاوت forward و full-refresh در workload مصنوعی، لزوم سنجش transformer و locality را نشان می‌دهد؛ bottleneck مدل واقعی هنوز مشخص نیست. برای stack، dirty records مشترک دو perspective و lazy refresh هدفمندتر را مستقل بیازمایید.
۳. سپس history/killer پایدار با aging، مالکیت TT بین حرکت‌ها و invalidation هنگام تعویض evaluator را جدا پیاده کنید. شرط نیم‌حرکت صفر TT را بدون اثبات ایمنی حذف نکنید. PVS را با full alpha-beta گسترده مقایسه و سپس LMR محافظه‌کارانه و null move با guardهای zugzwang را مستقل آزمایش کنید.
۴. scaling را روی سخت‌افزار دارای حداقل هشت CPU قابل استفاده و بودجهٔ hash/CPU برابر بسنجید. shared TT فقط پس از شواهد و طراحی atomic معتبر Rust مطرح شود.
۵. paired matches در برابر baseline و نسخه‌های دقیق Caissa/Reckless/Berserk/Stockfish با openings معتبر، رنگ معکوس، زمان/وزن تأییدشده، گزارش بازی‌های ناتمام، CI و SPRT از پیش تعریف‌شده لازم‌اند. هیچ Elo دقیقی از داده‌های این اجرا به دست نیامده است.
