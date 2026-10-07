# Native wallpaper, sub-project 1: core

## Goal

A native Windows app that draws the pixel3d meadow behind the desktop icons, using far less memory and CPU than the
browser does (~650 MB for a headless Chrome running the web version).

The web version stays as it is: it keeps its own wallpaper mode (`?wallpaper`, PR #9) and remains the reference for
colours and world generation. The native app lives next to it in `native/` and touches nothing outside that folder.

## Decisions (agreed with the user)

| Question | Decision |
|---|---|
| Priority | Lowest memory/CPU: full rewrite in Rust, no browser engine |
| Look | Same pixel-art meadow style and features; layout and shading may differ or be simplified |
| Graphics API | wgpu on DirectX 12 |
| Toolchain | Rust installed on Windows (MSVC); driven from WSL through `cmd.exe` |
| Desktop behaviour | Behind the icons, pause when covered, tray icon with Quit, start with Windows |
| Out of scope | Other monitors (primary only), settings UI, config file |
| Sub-project 2 (later, own spec) | Flowers, butterflies, dragonflies, gnats, fireflies |

## Success criteria

1. The meadow is drawn behind the desktop icons on the primary monitor and follows the local clock (sunrise 06:00,
   sunset 18:00).
2. Working set **under 80 MB** while drawing, measured on the user's PC.
3. About 0% CPU and GPU while a maximised or fullscreen window covers the primary monitor.
4. The tray menu's Quit exits and restores the normal wallpaper. Its Start with Windows toggle adds and removes the
   start-up entry, and its tick shows the current state.
5. `cargo test` passes.

## Structure

```
native/
  Cargo.toml
  src/
    main.rs       event loop: 30 fps tick, pause, quit
    world.rs      world generation (port of src/world.js)
    grass.rs      blade placement + view culling (port of src/grass.js)
    sky.rs        clock -> lights and sky colour (port of src/sky.js)
    desktop.rs    Win32: behind the icons, covered check, start-up registry entry
    tray.rs       tray icon and menu
    render/
      mod.rs      device, surface, targets, per-frame uniforms
      *.wgsl      shaders
  examples/
    snapshot.rs   renders one frame offscreen to a PNG (verification only)
```

Dependencies: `wgpu`, `winit`, `windows` (Win32), `tray-icon`, `glam`, `bytemuck`. `snapshot.rs` additionally uses
`png` as a dev-dependency. There are no asset files: everything is generated in code, as on the web.

Each module has one job:

| Module | Job | Input | Output | GPU? |
|---|---|---|---|---|
| `world` | Port of `world.js` generation: `mulberry32`, value noise / `fbm`, `heightAt`, pond, sand, path, trees (seed 42, up to 120), rock clusters (12) | seed | ground mesh (positions, normals, vertex colours), pond and sand shapes, tree and rock transforms | no |
| `grass` | Port of the blade placement (seed 99, density rules, canopy mask) and of `fitView`: drop blades above/below the view, sort by \|view x\|, count for the current aspect | world, camera | blade instances, `count(aspect)` | no |
| `sky` | Port of `sky.update` and `clockTime`: phase, sun/moon direction, light colour and intensity, hemisphere colours, sky colour, daylight | local time | `SkyState` | no |
| `render` | Pipelines, shaders, frame drawing | the data above + time | frames | yes |
| `desktop` | Behind the icons, covered check, Run registry entry | HWND | — | no |
| `tray` | Icon (built in code), menu: Start with Windows ✓, Quit | — | menu events | no |
| `main` | Wires it together | — | — | — |

`world`, `grass` and `sky` have no Windows or GPU dependencies, so their tests run in WSL with a Linux toolchain too.

## Camera and resolution

The camera is the same as the web version's: orthographic, yaw 45°, pitch 30°, distance 30, 18 world units visible
vertically. Everything renders at **art resolution**: the window size divided by 4, rounded down (480×270 on 1080p,
960×540 on 4K). The result is stretched to the window with nearest-neighbour sampling.

## Rendering pipeline (each drawn frame)

1. **Shadow pass**: depth only, from the sun (by day) or the moon (by night), orthographic ±22 units like the web
   version. The casters are the trees and rocks. The map is 1024×1024 (4 MB) instead of the web's 2048: one texel is
   0.043 units, still smaller than one art pixel (0.067 units).
2. **Scene pass**: one geometry pass with two colour targets, colour (RGBA8) and view-space normal (RGBA8), plus depth
   (Depth32). The web version draws the scene twice to get these. It draws:
   - ground with vertex colours (grass, path, sand tint)
   - the sand disc and the pond; the pond's colours animate over time and pick up a hint of the sky colour
   - trunks, crowns and rocks, instanced (3 draws); trees sway like `world.update`, computed in the vertex shader from
     the instance's phase rather than re-uploaded from the CPU
   - grass, instanced: wind on the tips (same formula as `addWind`), darker root to lighter tip, up-facing normals
   - Lighting is toon: hemisphere light plus the sun or moon, quantised to the 4 bands of `toonGradient`
     (60, 125, 195, 255), with shadows and drifting cloud shadows (scrolling noise, as in `sky.js`).
3. **Outline pass**: a full-screen pass at art resolution reading colour, normals and depth, which darkens depth
   edges and lightens normal edges (depth strength 0.4, normal strength 0.3, like `RenderPixelatedPass`).
4. **Upscale**: a blit of the art texture to the swapchain with nearest sampling.

Per frame the CPU uploads only one uniform block: time, light directions and colours, sky colour, and the grass count.
Meshes and instance data go to the GPU once at start-up.

Toon and shadow maths are our own rather than a copy of Three.js. Palette values come from the JS sources, including
the approved grass colours (commit 522ea47).

## Frame timing and pause

- 30 fps, driven by `ControlFlow::WaitUntil(next tick)`. The process sleeps between frames, with no busy loop.
  Present mode Fifo, 2 swapchain frames.
- **Time:** the sky follows the local clock (`clockTime`, as in PR #9). Wind, water and clouds use the time since
  start-up.
- **Covered check, once a second:** enumerate visible, non-minimised top-level windows on the primary monitor. Skip
  cloaked ones (other virtual desktops, suspended UWP), the shell and our own window. If any is maximised
  (`IsZoomed`) or covers the whole monitor rectangle, pause: no frames are drawn, and only this check keeps running.
  When nothing covers the monitor, resume.

## Desktop integration

- **Behind the icons:**
  1. Send `Progman` the message `0x052C` so Explorer creates the `WorkerW` layer.
  2. Find it, handling both layouts:
     - Before Windows 11 24H2: the `WorkerW` sibling after the one that holds `SHELLDLL_DefView`.
     - 24H2 and later: a `WorkerW` child of `Progman`.
  3. `SetParent` our borderless window to it and size it to the primary monitor's rectangle, converted to the
     parent's coordinates.
- **Fallback:** if no `WorkerW` is found, run as a borderless window at the bottom of the Z-order
  (`HWND_BOTTOM`, `WS_EX_TOOLWINDOW` so there's no taskbar button).
- **Explorer restart:** on the registered `TaskbarCreated` message, re-attach and re-add the tray icon.
- **Quit:** detach and re-apply the current wallpaper (`SystemParametersInfoW(SPI_SETDESKWALLPAPER, …)` with the path
  read through `SPI_GETDESKWALLPAPER`), so the last frame doesn't stay on screen.
- **Start with Windows:** the value `pixel3d` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, holding the
  quoted path of the running `.exe`. The tick is on when that value exists and matches this `.exe`.
- **Single instance:** a named mutex (`Local\pixel3d-wallpaper`); a second copy exits straight away.
- **Release profile:** `#![windows_subsystem = "windows"]` (no console), `lto = true`, `panic = "abort"`,
  `opt-level = 3`, `codegen-units = 1`.

## Error handling

| Situation | Behaviour |
|---|---|
| No usable GPU adapter, or device creation fails | Message box with the reason, exit |
| Surface `Lost` / `Outdated` (sleep, driver reset, display change) | Reconfigure the surface and rebuild the art-size targets |
| Primary monitor resolution or DPI change | Resize the window, rebuild the targets, recompute the grass count |
| Surface `Timeout` | Skip the frame |
| Attaching behind the icons fails | Bottom-most window fallback (see above) |
| Registry write fails | Tick stays as it was; no crash |

## Build

- **Source:** in WSL (`/home/dung/Study/pixel3d/native`).
- **Building:** `cargo` runs on Windows through `cmd.exe`, with `CARGO_TARGET_DIR` on the Windows drive, because
  building over `\\wsl$` is slow.
- **The user installs once:**
  - `rustup-init.exe` (stable, `x86_64-pc-windows-msvc`)
  - Visual Studio Build Tools with the "Desktop development with C++" workload

## Testing

1. **Unit tests** (`cargo test`):
   - `sky`: the `clockTime` cases from `test/sky.test.js` (0:00, 5:59, 6:00, 12:00, 18:00, 23:30, compared across the
     24-hour wrap), noon daylight > 0.99, midnight daylight 0.
   - `world`: the same seed gives the same world; heights are finite; no tree has `sandQ < 1.15` or `pathEdge < 1`;
     no rock's centre is closer than its radius to the pond or path edge (`pondEdge`, `pathEdge`).
   - `grass`: deterministic; every kept blade is within the view height plus the margin; `count(aspect)` grows with
     the aspect and covers every blade within the view width plus the margin.
2. **Snapshot**: `cargo run --example snapshot -- --hour 12 --width 1920 --height 1080 out.png` renders one frame
   offscreen. Taken at 12:00, 18:30 and 02:00 and compared by eye with web screenshots at the same hours.
3. **On the user's PC**: run the release `.exe` and record working set, private bytes and CPU over 60 s with PowerShell
   `Get-Process`, both while drawing and while a maximised window covers the desktop. Compare with the browser figures
   (~650 MB). Check Quit, the restored wallpaper and the start-up toggle by hand.
