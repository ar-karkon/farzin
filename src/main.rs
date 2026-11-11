fn main() {
    farzin::uci::run();
}

/// Start-up CPU check that runs before Rust's runtime and `main`.
///
/// Release builds target x86-64-v3 / x86-64-v2 with fat LTO, so the
/// compiler may use AVX (or SSE4/POPCNT) instructions anywhere, including in
/// `main` itself, before `uci::run` gets to its own feature check. On a CPU
/// without those instructions the process would die with "illegal
/// instruction" instead of explaining why. This guard is registered as a
/// static initializer (ELF `.init_array`, Windows CRT `.CRT$XCB`), uses only
/// `cpuid` / `xgetbv` and scalar integer code, and on a mismatch writes a
/// message to stdout (as a UCI `info string`) and stderr and exits with
/// status 1. It changes nothing on a supported CPU.
#[cfg(target_arch = "x86_64")]
mod cpu_guard {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    extern "C" {
        #[cfg_attr(windows, link_name = "_write")]
        fn write(fd: i32, buf: *const u8, len: usize) -> isize;
        fn _exit(code: i32) -> !;
    }

    #[inline(always)]
    fn put(fd: i32, s: &[u8]) {
        // SAFETY: writes a static byte string to a standard stream.
        unsafe {
            write(fd, s.as_ptr(), s.len() as _);
        }
    }

    /// The instruction sets this build was compiled for (a compile-time
    /// constant, so the message needs no run-time formatting).
    const REQUIRED: &[u8] = if cfg!(target_feature = "avx512bw") {
        b"AVX-512 (F, BW), AVX2, BMI1/BMI2, FMA, LZCNT, POPCNT"
    } else if cfg!(target_feature = "avx2") && cfg!(target_feature = "bmi2") {
        b"x86-64-v3: AVX2, BMI1/BMI2, FMA, LZCNT, MOVBE, POPCNT"
    } else if cfg!(target_feature = "avx2") {
        b"x86-64-v3 without BMI2: AVX2, BMI1, FMA, LZCNT, MOVBE, POPCNT"
    } else {
        b"x86-64-v2: SSE4.2, POPCNT"
    };

    /// Only constant strings are written, by separate calls, so this
    /// compiles to scalar code with no vector registers (the compiler may
    /// otherwise use AVX for copies, which is exactly what must not run).
    #[inline(never)]
    #[cold]
    fn report() -> ! {
        put(1, b"info string ");
        put(1, b"Farzin: this build requires a CPU with ");
        put(1, REQUIRED);
        put(1, b". This processor lacks some of them; use another Farzin build (see README: Hardware requirements).\n");
        put(2, b"Farzin: this build requires a CPU with ");
        put(2, REQUIRED);
        put(2, b". This processor lacks some of them; use another Farzin build (see README: Hardware requirements).\n");
        // SAFETY: terminates the process without running any further code.
        unsafe { _exit(1) }
    }

    #[inline(never)]
    extern "C" fn check() {
        // Leaf 7 and the extended leaf are only read when the CPU reports them.
        let (max, l1) = (__cpuid(0).eax, __cpuid(1));
        let l7 = if max >= 7 { __cpuid_count(7, 0).ebx } else { 0 };
        let ext_max = __cpuid(0x8000_0000).eax;
        let e1 = if ext_max >= 0x8000_0001 {
            __cpuid(0x8000_0001).ecx
        } else {
            0
        };
        let c = l1.ecx;
        // OS support for the AVX (YMM) and AVX-512 (ZMM) register state.
        let xcr0 = if c & (1 << 27) != 0 {
            let (lo, _hi): (u32, u32);
            // SAFETY: OSXSAVE is set, so xgetbv is enabled.
            unsafe {
                core::arch::asm!("xgetbv", in("ecx") 0, out("eax") lo, out("edx") _hi,
                    options(nomem, nostack, preserves_flags));
            }
            lo
        } else {
            0
        };
        let ymm = xcr0 & 0x6 == 0x6;
        let zmm = ymm && xcr0 & 0xe0 == 0xe0;
        let mut missing = 0u32;
        let mut need = |i: u32, compiled: bool, present: bool| {
            if compiled && !present {
                missing |= 1 << i;
            }
        };
        let bit = |r: u32, b: u32| r & (1 << b) != 0;
        need(0, cfg!(target_feature = "sse3"), bit(c, 0));
        need(1, cfg!(target_feature = "ssse3"), bit(c, 9));
        need(2, cfg!(target_feature = "sse4.1"), bit(c, 19));
        need(3, cfg!(target_feature = "sse4.2"), bit(c, 20));
        need(4, cfg!(target_feature = "popcnt"), bit(c, 23));
        need(5, cfg!(target_feature = "avx"), bit(c, 28) && ymm);
        need(6, cfg!(target_feature = "fma"), bit(c, 12) && ymm);
        need(7, cfg!(target_feature = "f16c"), bit(c, 29) && ymm);
        need(8, cfg!(target_feature = "movbe"), bit(c, 22));
        need(9, cfg!(target_feature = "bmi1"), bit(l7, 3));
        need(10, cfg!(target_feature = "avx2"), bit(l7, 5) && ymm);
        need(11, cfg!(target_feature = "bmi2"), bit(l7, 8));
        need(12, cfg!(target_feature = "lzcnt"), bit(e1, 5));
        need(13, cfg!(target_feature = "avx512f"), bit(l7, 16) && zmm);
        need(14, cfg!(target_feature = "avx512bw"), bit(l7, 30) && zmm);
        if missing != 0 {
            report();
        }
    }

    // Runs before the Rust runtime (`.init_array.00099`) and before C++-style
    // constructors in `.CRT$XCU`.
    #[used]
    #[cfg_attr(windows, link_section = ".CRT$XCB")]
    #[cfg_attr(not(windows), link_section = ".init_array.00090")]
    static CPU_GUARD: extern "C" fn() = check;
}
