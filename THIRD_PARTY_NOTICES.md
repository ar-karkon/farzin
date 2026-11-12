# Third-party notices

`tools/compare_caissa_accumulators.py` and `src/caissa.rs` implement the network format and scalar
mathematics described by Caissa, commit `395c2e6bb7361bacdde813b014dd438af2d21339`.
The original Caissa code is distributed under the following MIT license. This
notice concerns the code; it does not establish a license for neural-network weights.

MIT License

Copyright (c) 2021 Michał Witanowski

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Stockfish NNUE network weights (CC0-1.0)

`nets/default.nnue` is `nn-ad9b42354671.nnue` from
<https://github.com/official-stockfish/networks>. That repository states that
all its networks were uploaded by their authors under the CC0 1.0 Universal
public-domain dedication. The file is redistributed unmodified
(sha256 `ad9b423546714137916bd38978af6fd68d7b8951bef25ff76bf43da72d6cb786`).

Farzin's inference for this format (`src/hka.rs`) is an independent Rust
implementation written from the file layout; it contains no Stockfish
(GPL-3.0) source code. A locally built, unmodified-except-for-one-debug-print
Stockfish binary was used only as a test oracle and is not distributed.

## Linked runtime libraries

The self-contained release executables also contain code from the Rust
standard library and its runtime dependencies, and the Windows executables
use the MinGW-w64 startup/runtime libraries. These are third-party code,
not original Farzin code. Their licenses and exceptions continue to apply.

The release ZIP includes `RUNTIME-LICENSES/`: Rust's complete
`COPYRIGHT-library.html` and referenced license texts copied verbatim from
the pinned rustc 1.97.0 distribution, plus the installed Ubuntu MinGW-w64
and GCC runtime copyright/license notices. These notices describe the
upstream library distribution, including components for other platforms;
they do not imply that every listed component is linked into Farzin.
Windows system DLLs are supplied by Windows and are not redistributed.
