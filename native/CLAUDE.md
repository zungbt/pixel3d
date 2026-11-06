# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

This file covers `native/`: a Rust + wgpu (Vulkan) Windows app that draws an animated pixel-art meadow, as the desktop wallpaper (behind the icons, tray menu) or in a window. It is a port of the three.js scene in `../src/`, which is frozen as the **visual and behaviour reference**: when behaviour is ambiguous, the web code is the spec unless `../docs/native-roadmap.md` deliberately diverges. Design history: `../docs/native-roadmap.md`, `../docs/native-sp{1,2,3}-*.md`; perf numbers: `../docs/perf-baseline.md`.

## Commands (run from `native/`)

```bash
cargo test -p meadow-core                     # sim crate: pure Rust, any host
cargo test -p meadow                          # bin crate: shader validation, camera/mesh/grass maths
cargo test -p meadow shaders_validate         # one test
cargo test -p meadow-core --lib world         # one module
cargo build --target x86_64-pc-windows-gnu    # the shippable exe (Linux/WSL, mingw)
./run.sh [--release] [-- <args>]              # WSL dev loop
```

**Release builds are slow on purpose.** `[profile.release]` uses fat LTO (`lto = true`), whose final step runs on about one core: about 2 min 30 s on this 16-core Windows machine, even after a one-line change. For look-only iterations, `[profile.quick]` (`inherits = "release"`, `lto = "thin"`, parallel): `cargo build --profile quick` takes 116 s the first time and 32 s after a change in `meadow`, and writes the exe to `target/quick/`. RAM and GPU numbers (`perf-baseline.md`) must still come from `--release`, so they stay comparable.

`run.sh` kills any running `meadow.exe` (a running exe is locked), cross-builds, copies the exe to `/mnt/c/Users/d/pixel3d/` and launches it there with `--window`.

On Windows the exe is the **wallpaper by default**; `--window`, `--screens` or `--fullscreen` make it windowed (`--wallpaper` is still accepted because old autostart entries pass it). Per-run args (not saved): `--fps 0|60|30|20|15` (0 = no cap, vsync), `--pixel-size 2|3|4`, `--hour H` (pin the sky, 0–24), `--cycle` (the web's 300 s day), `--date MM-DD`, `--toggle-clock S`, `--screens WxH+X+Y,...` (one window per fake monitor, first = primary, to check the seams), `--gpu-time`, `--fullscreen`. Bad values panic with the expected format. Logging is `env_logger` (`RUST_LOG` works; default `meadow=info,wgpu_core=warn,wgpu_hal=warn`).

`tools/dump-world.mjs` regenerates `meadow-core/tests/fixtures/world42.txt`, the web layout (seed 42) that `world`'s golden test compares against. Run `node native/tools/dump-world.mjs` from the repo root after changing placement code in `../src/world.js`.

Known flaky on a Windows host: `sky_clock::an_unknown_city_zone_leaves_the_clock_on_the_pc` (a float lerp settles at 420.00000000000006, exact `assert_eq!`). It passes on Linux. Don't reword it unless asked.

## Architecture

**Two crates, split by GPU dependency, not by feature.**

- **`meadow-core`**: the simulation and everything time-dependent, with no GPU/window deps (`libm` only), so `cargo test` is a plain host build. `rng` is mulberry32 matching the JS PRNG bit for bit; `world` holds height, path SDF and placements; `tiles::place_outer` builds the seeded outer tiles that extend the world across wide multi-monitor setups; one module per creature; plus `sky`, `sun`, `clock`, `weather`, `clouds`, `wreck` (the post clock), `frame_pacer`, `settings`, `label`.
  - Simulators output plain structs (positions, colours, wing phases). The bin reads those to build GPU instances. Keep this separation: it is what makes the sim testable.
  - Anything time-zone dependent takes an **explicit offset or IANA zone**; never read the local zone inside the sim. Tests must be deterministic on any machine.
- **`meadow`**: the winit + wgpu binary. `main.rs` (`ApplicationHandler`, frame loop, args, settings I/O), `gpu` (device/surface), `camera` (ortho `Pane`s), `render` (shadow → MRT scene → post upscale, all pipelines), `grass` (instance packing, per-pane culling), `mesh` (hand-built flat primitives), `sky_clock` (time of day, city travel, post-clock hands). Windows-only, behind `cfg(windows)`: `wallpaper` (desktop attach, tray, HKCU Run autostart, power throttling), `net` (WinHTTP → Open-Meteo, every 15 min), `city`.

**Shaders** (`meadow/src/shaders/`) are assembled in code, not read from disk. `common.wgsl` (noise, cloud shading, ground fog, toon steps, PCF, the `Globals` uniform) is prepended via `include_str!` to shadow/clouds/scene. `render::prelude()` additionally generates struct code into `scene.wgsl`. `post.wgsl` stands alone. A `#[repr(C)]` struct mirrored in WGSL must stay in sync with `common.wgsl`: `globals_match_the_wgsl_struct` and `shaders_validate` (naga) enforce it.

**Rendering model.** Everything is instanced (mat4 + colour + wind factor). Fixed pipelines: shadow, clouds, scene (MRT: `Rgba16Float` colour + `Rgba8Unorm` view normals + `Depth32Float`), post. There is no egui: all UI is the native Windows tray. One `Renderer` drives many `Screen`s/`Pane`s: each monitor draws its own view of one shared meadow. Art size = `ceil(physical / pixel_size)`, and post maps screen px → art px by integer division, so every art pixel is exactly `pixel_size²` screen pixels. Grass and flowers write **un-flipped** `(0,1,0)` normals on purpose: flipping by `front_facing` would outline every blade. Lighting still flips for back faces.

**Frame loop** (differs from the web on purpose: the web filters rAF ticks, which can't be ported). `frame_pacer` returns the next deadline; `main` sleeps with `ControlFlow::WaitUntil`, then requests a redraw. No cap = `PresentMode::Fifo`. `dt` comes from `Instant`, clamped to 0.1 s, and is 0 on the first frame. Minimised → stop redrawing. Occluded (wgpu reports it, winit doesn't on Windows) → retry every 250 ms. Surface `Outdated` → reconfigure and skip; `Lost` → recreate the surface and skip; `Timeout`/`Validation` → skip. Never reload or rebuild the scene. Resize to 0 is ignored; `configure` skips an unchanged size (swapchain recreation costs 80–180 ms on Intel Vulkan); art targets are rebuilt only when the art size changes.

**Settings, logging, backend, build.**
- Settings are `key = value` lines in `%APPDATA%\pixel3d\settings.txt`, hand-parsed in `meadow_core::settings` (no serde). Every value is validated; missing or garbage values fall back to defaults. In wallpaper mode they are set from the tray.
- Backend is Vulkan only (probing DX12/GL loads their DLLs, +14 MB); `WGPU_BACKEND=dx12` overrides it. Debug builds keep the console; release builds are `windows_subsystem = "windows"` and log (panics included) to `%APPDATA%\pixel3d\log.txt`.
- `.cargo/config.toml` sets only the mingw **linker** and deliberately no default `build.target`, so `cargo test` stays a host build. Windows-only deps are target-specific for the same reason.
- `build.rs` embeds `meadow.manifest` via `meadow.rc` and `x86_64-w64-mingw32-windres`, which must be on PATH for the cross build.

## Conventions

- **Never commit to `main`.** Branch `feat/`, `fix/`, `refactor/`, `docs/` + a lowercase hyphenated name, cut from the latest `main` (`../CONTRIBUTING.md`). Merge with `--no-ff`, then delete the branch. Push only when the user says so. No co-author trailer.
- One idea per commit, in English; the first line says what changed. Native commits use the `feat(native): …` / `fix(native): …` style.
- Visual changes have **no automated tests**. Verify by eye against the web scene (`npm run dev` from the repo root) at the hours listed in the relevant `../docs/native-sp*.md` exit criteria. Every native milestone also gets a RAM row in `../docs/perf-baseline.md` (`powershell.exe "Get-Process meadow | Select WorkingSet64,PrivateMemorySize64"` after 60 s).
