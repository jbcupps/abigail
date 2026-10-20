# Temporary updater dependency patch

This is the Rust package from `tauri-plugin-updater` **2.10.0**, published at
<https://crates.io/crates/tauri-plugin-updater/2.10.0>.

- Published crate SHA-256: `3fe8e9bebd88fc222938ffdfbdcfa0307081423bd01e3252fc337d8bde81fc61`
- Upstream revision: `50b159f668a5fcf42d6356e2af1dc549632b413d`
- The only upstream source/configuration change is the Windows `zip` dependency:
  `4` -> `9.0.5`. The Rust implementation, default features, signature verification,
  embedded JavaScript, permissions and licenses are unchanged.
- Registry cache markers, the upstream standalone lockfile, original workspace
  manifest, and JavaScript development sources/configuration are omitted. The
  repository's root `Cargo.lock` governs this package.

ZIP 9 fixes RUSTSEC-2026-0367, RUSTSEC-2026-0368 and RUSTSEC-2026-0369, covering
unsafe extraction permissions and symlink targets. Disabling the updater's `zip`
feature would break the retained signed Windows ZIP and Linux AppImage update
formats. The published updater 2.14.0 still requires ZIP 4 and additionally raises
the Tauri requirement; vendoring the existing version avoids that unrelated change.

`tauri-app/Cargo.toml` pins this version and the root `[patch.crates-io]` selects
this directory. Remove the patch and directory once a compatible upstream release
uses ZIP 9 or newer, after verifying the signed updater archive formats and native
builds. Do not replace this fix with audit exceptions or disable signature checks.
