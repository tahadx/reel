# Third-party components

Reel's engine is statically linked for `x86_64-unknown-linux-musl`. This
file maps every Rust dependency in the engine's normal dependency graph
(including proc-macro crates used at build time) to its version, license, and
the original license/notice text shipped under `licenses/`.

Toolchain: `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, target `x86_64-unknown-linux-musl`.

The Rust standard library and the self-contained musl libc it links are
covered by `licenses/rust-toolchain/COPYRIGHT-library.html` (the musl libc
copyright and math notices are in that file); see
`licenses/rust-toolchain/NOTICE.md`, and the toolchain licenses are in
`licenses/rust-toolchain/licenses/`. Reel's own code is MIT, see `LICENSE`.

| Component | Version | License | License files |
| --- | --- | --- | --- |
| `anstream` | 1.0.0 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/anstream-1.0.0/LICENSE-APACHE), [LICENSE-MIT](licenses/anstream-1.0.0/LICENSE-MIT) |
| `anstyle` | 1.0.14 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/anstyle-1.0.14/LICENSE-APACHE), [LICENSE-MIT](licenses/anstyle-1.0.14/LICENSE-MIT) |
| `anstyle-parse` | 1.0.0 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/anstyle-parse-1.0.0/LICENSE-APACHE), [LICENSE-MIT](licenses/anstyle-parse-1.0.0/LICENSE-MIT) |
| `anstyle-query` | 1.1.5 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/anstyle-query-1.1.5/LICENSE-APACHE), [LICENSE-MIT](licenses/anstyle-query-1.1.5/LICENSE-MIT) |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/anyhow-1.0.104/LICENSE-APACHE), [LICENSE-MIT](licenses/anyhow-1.0.104/LICENSE-MIT) |
| `clap` | 4.6.7 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/clap-4.6.7/LICENSE-APACHE), [LICENSE-MIT](licenses/clap-4.6.7/LICENSE-MIT) |
| `clap_builder` | 4.6.7 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/clap_builder-4.6.7/LICENSE-APACHE), [LICENSE-MIT](licenses/clap_builder-4.6.7/LICENSE-MIT) |
| `clap_derive` | 4.6.7 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/clap_derive-4.6.7/LICENSE-APACHE), [LICENSE-MIT](licenses/clap_derive-4.6.7/LICENSE-MIT) |
| `clap_lex` | 1.1.1 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/clap_lex-1.1.1/LICENSE-APACHE), [LICENSE-MIT](licenses/clap_lex-1.1.1/LICENSE-MIT) |
| `colorchoice` | 1.0.5 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/colorchoice-1.0.5/LICENSE-APACHE), [LICENSE-MIT](licenses/colorchoice-1.0.5/LICENSE-MIT) |
| `heck` | 0.5.0 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/heck-0.5.0/LICENSE-APACHE), [LICENSE-MIT](licenses/heck-0.5.0/LICENSE-MIT) |
| `is_terminal_polyfill` | 1.70.2 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/is_terminal_polyfill-1.70.2/LICENSE-APACHE), [LICENSE-MIT](licenses/is_terminal_polyfill-1.70.2/LICENSE-MIT) |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/itoa-1.0.18/LICENSE-APACHE), [LICENSE-MIT](licenses/itoa-1.0.18/LICENSE-MIT) |
| `libc` | 0.2.191 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/libc-0.2.191/LICENSE-APACHE), [LICENSE-MIT](licenses/libc-0.2.191/LICENSE-MIT) |
| `memchr` | 2.8.3 | Unlicense OR MIT | [COPYING](licenses/memchr-2.8.3/COPYING), [LICENSE-MIT](licenses/memchr-2.8.3/LICENSE-MIT), [UNLICENSE](licenses/memchr-2.8.3/UNLICENSE) |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/proc-macro2-1.0.107/LICENSE-APACHE), [LICENSE-MIT](licenses/proc-macro2-1.0.107/LICENSE-MIT) |
| `quote` | 1.0.47 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/quote-1.0.47/LICENSE-APACHE), [LICENSE-MIT](licenses/quote-1.0.47/LICENSE-MIT) |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/serde-1.0.229/LICENSE-APACHE), [LICENSE-MIT](licenses/serde-1.0.229/LICENSE-MIT) |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/serde_core-1.0.229/LICENSE-APACHE), [LICENSE-MIT](licenses/serde_core-1.0.229/LICENSE-MIT) |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/serde_derive-1.0.229/LICENSE-APACHE), [LICENSE-MIT](licenses/serde_derive-1.0.229/LICENSE-MIT) |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/serde_json-1.0.154/LICENSE-APACHE), [LICENSE-MIT](licenses/serde_json-1.0.154/LICENSE-MIT) |
| `strsim` | 0.11.1 | MIT | [LICENSE](licenses/strsim-0.11.1/LICENSE) |
| `syn` | 3.0.9 | MIT OR Apache-2.0 | [LICENSE-APACHE](licenses/syn-3.0.9/LICENSE-APACHE), [LICENSE-MIT](licenses/syn-3.0.9/LICENSE-MIT) |
| `unicode-ident` | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | [LICENSE-APACHE](licenses/unicode-ident-1.0.27/LICENSE-APACHE), [LICENSE-MIT](licenses/unicode-ident-1.0.27/LICENSE-MIT), [LICENSE-UNICODE](licenses/unicode-ident-1.0.27/LICENSE-UNICODE) |
| `utf8parse` | 0.2.2 | Apache-2.0 OR MIT | [LICENSE-APACHE](licenses/utf8parse-0.2.2/LICENSE-APACHE), [LICENSE-MIT](licenses/utf8parse-0.2.2/LICENSE-MIT) |
| `zmij` | 1.0.23 | MIT | [LICENSE-MIT](licenses/zmij-1.0.23/LICENSE-MIT) |

Generated from `Cargo.lock` and `cargo metadata --filter-platform
x86_64-unknown-linux-musl`; the files are the originals from the crates.io
archives and the installed Rust toolchain.
