# Rust toolchain and musl

- Toolchain: `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- Target: `x86_64-unknown-linux-musl`
- Rust standard library: `COPYRIGHT-library.html` (MIT OR Apache-2.0). The same
  file contains the musl libc copyright and the third-party math notices for
  the self-contained libc linked by this target.
- License texts: `licenses/*.txt` from the installed toolchain.

The shipped `bin/dist/reel-native` is rebuilt for this exact target by
`bin/refresh-dist`.
