# taffy 0.9.0, patched for Aether

Copy of [taffy 0.9.2](https://crates.io/crates/taffy/0.9.2) (MIT,
see LICENSE-MIT), pulled by GPUI. GPUI pins `=0.9.0`, while 0.9.0 pulls
`grid 0.18.0` which is vulnerable to GHSA-38c5-483c-4qqp (safe-API integer overflow).
The package version is kept as 0.9.0 so GPUI's exact requirement is met, pulling `grid >= 1.0.0` (resolves to 1.0.1).
The root `Cargo.toml` applies it with `[patch.crates-io]`; drop once GPUI updates.
