# xattr 0.2.3, patched for Aether

Verbatim copy of [xattr 0.2.3](https://crates.io/crates/xattr/0.2.3) (MIT or
Apache-2.0, see the licence files), pulled by GPUI through `zed-async-tar`.
libc 0.2.190 removed the Linux `ENOATTR` alias this release relies on; the only
change is in `src/sys/mod.rs` (Linux and Android use `ENODATA`). The root
`Cargo.toml` applies it with `[patch.crates-io]`; drop both once GPUI moves to
xattr 1.x.
