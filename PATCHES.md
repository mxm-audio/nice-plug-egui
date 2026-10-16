# MXM patch set for nice-plug-egui 0.5.1

Upstream crate: `nice-plug-egui 0.5.1`, ISC, as published on crates.io (the first commit here). The
upstream licence is `LICENSE`, copied from the nice-plug fork, whose terms are the same; the
published package carries no licence file of its own. Every MXM plugin takes it through
`[patch.crates-io]`, as it takes nice-plug and egui-baseview.

## Host keys

**Files:** `Cargo.toml`, `src/lib.rs`, `src/host_keys.rs`, `src/editor.rs`; every hunk contains
`MXM PATCH (*Host keys*)` or sits in `host_keys.rs`. `Cargo.toml.orig` is upstream's, untouched.

An editor says which keys it uses, once, in `EguiNiceSettings::host_keys` (`with_host_keys`): a
`WantsKey`, asked with the window's egui context, an `egui::Key` and the modifiers held. MXM editors
answer with `mxm_ui::navigation::takes_key` (mxm-kit's `mxm-editor`). Two paths ask it:

- **Keys a host offers.** A VST3 host may keep a key from the window and offer it through
  `onKeyDown` (Ableton Live 11.3.13: letters, and Shift with the arrows). The nice-plug fork's
  *Host keys* registry hands it here, keyed by the parent window; a key the editor uses is taken,
  queued, and given to egui at the start of the next frame as `egui::Event::Key`, in order, with
  `physical_key` set. Live sends no key-up, so on Windows a held key is polled (`GetAsyncKeyState`,
  every 30 ms) and let go when it is released. A key the editor doesn't use is declined, and stays
  the host's.
- **Keys the window has.** The window's `KeyCapture` is egui-baseview's `Ask`, so a key the editor
  doesn't use is answered `Ignored`, and the baseview fork gives it to the host's window (Live
  11.3.25 hands the window every key, and plays T or starts its transport with Space only if they
  come back).

`None`, the default, keeps upstream's behaviour on both paths. The crate depends on the nice-plug
fork for the registry (`[dependencies.nice-plug]`, `default-features = false`), and its own
`[patch.crates-io]` takes the nice-plug and egui-baseview forks, for building this crate alone; every
repository that builds an MXM plugin patches them in anyway.

Tried in Ableton Live 11.3.26 (VST3) and Bitwig 5 (CLAP), 2026-10-16: S, D and F with the arrows
edit, the arrows move the cursor, T and Space go to the host, with or without a control focused,
and with Bitwig's Caps Lock note input on. **Regression tests:** `host_keys::tests` (egui's names,
a taken key down once, repeated while held and up once, the queue given at a frame's start in
order, and Windows' key codes).

## Refresh/drop procedure

1. Extract a new published nice-plug-egui over this folder, retaining this file and `LICENSE`.
2. Check whether upstream now answers `on_virtual_key_from_host` with letters, and lets the app say
   which keys its window keeps.
3. If it does both, drop this fork and the `[patch.crates-io]` entries that take it.
4. Otherwise reapply the `MXM PATCH` hunks and `src/host_keys.rs`, then try S with an arrow, T and
   Space in an MXM editor in Ableton Live and in Bitwig.
