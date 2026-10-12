# pixel3d native — sub-project 3: sky (spec)

## Context
Sub-project 2 (grass + wind) is merged, and the default pixel size is now 3. The wallpaper exe runs that build.

The native scene is always at noon: the constants are in `render.rs`, `camera::SUN_POS` is fixed, and the Globals are built once in `Renderer::new`. The web follows the PC clock (`src/sky.js`, `src/sun.js`, wired up in `src/main.js`):
- The colours go from night to dawn, day and dusk.
- One directional light is the sun by day and the moon by night.
- Cloud shadows drift over everything.
- Ground fog rolls in from dusk until after sunrise, thicker in winter.
- Sunrise and sunset come from the time zone's city (NOAA formula).

Sub-project 3 ports all of that.

Decided with the user (2026-10-12):
- **Time: the PC clock**, like the web's clock mode. Two test flags:
  - `--hour H` pins a local hour (0–24, fractional, e.g. `18.5`), for screenshots side by side with the web;
  - `--cycle` runs the web's manual 300 s day, starting from the current hour (or from `--hour`), so dusk and night can be watched live.
- **Location:** guessed from the Windows time zone, like the web. A line `place = lat, lon` in `%APPDATA%\pixel3d\settings.txt` overrides it. That file becomes sub-project 5's settings file, with a picker added later. GPS stays dropped.
- **Fog mode is 'auto'** (the web's default). The off/always setting comes with the settings UI in sub-project 5.
- **Not in this sub-project:** the post clock and the settings UI (5); insects that read the sky state (4).

## Branch / commits
- `feat/native-sky` from `main` (223c8dc).
- One idea per commit, no co-author trailer (user rule), push and PR only on user OK.
- First commit: this spec, plus the roadmap's crate line (jiff instead of chrono + iana-time-zone).

## Design

### Core: `meadow-core/src/sun.rs` (pure, tested on Linux)
Ported from `src/sun.js`. Time zones come in as explicit values, never `Local`, so tests give the same result on any machine.
- **`ZONES`:** the web's 42 entries, IANA name → (lat, lon).
- **`zone_place(zone: Option<&str>, utc_offset_min: i32) -> (f64, f64)`:**
  - a table hit → that place;
  - otherwise (0, offset in hours · 15).
- **`Date { year: i32, month: u8, day: u8 }`** is the local calendar date. `day_of_year` gives the same result as the web's `dayOfYear`, leap years included.
- **`sun_times(date, utc_offset_min, (lat, lon)) -> SunTimes { rise, set }`** in local clock hours. The NOAA terms are the web's exactly, with half a day clamped to 0.5–11.5 h, and `noon = 12 − eqTime/60 − lon/15 + utc_offset_min/60`.
  - Note the sign: the web's `getTimezoneOffset()` is minus the UTC offset.
- **`winterness(date, lat) -> f64`:** the web's formula, +182 days south of the equator.
- **`parse_place(settings: &str) -> Result<Option<(f64, f64)>, String>`:**
  - It finds the `place` key in `key = value` lines (whitespace trimmed, `#` comments skipped).
  - No `place` line gives `Ok(None)`.
  - A malformed value, or one out of range (|lat| > 90, |lon| > 180), gives `Err(message)`.
  - It never panics: the wallpaper has no console.

### Core: `meadow-core/src/sky.rs`
Ported 1:1 from `src/sky.js`.
- **Constants:**
  - `DAY_LENGTH = 300.0`, `DAY_START = 0.08`, `NOON = (0.25 − DAY_START) · DAY_LENGTH`;
  - the palette as sRGB hex: `SUN_DAY`, `SUN_LOW`, `MOON`, `SKY_DAY`, `SKY_DUSK`, `SKY_NIGHT`, `HEMI_DAY`, `HEMI_NIGHT`, `FOG_DAY`, `FOG_NIGHT`; `SUN_DIST = 38`;
  - `linear(hex)` moves here from `render.rs`, and every lerp is in linear space (three.js `Color` is linear in the web's colour-managed setup).
- **`hour_to_phase(hour, sun)`, `phase_to_hour(phase, sun)`:** the web's formulas.
- **`clock_time(hour, sun) -> f64`:** the day time `(hour_to_phase − DAY_START) mod 1 · DAY_LENGTH`. It takes the fractional hour, not a `Date`.
- **`fog_amount(elevation, winter) -> f64`:** 'auto' only.
- **`sky_state(day_t, winter) -> SkyState`** holds the web's `update` maths, field by field:

  | field | value |
  |---|---|
  | `phase` | `(day_t / DAY_LENGTH + DAY_START) mod 1` |
  | `elevation` | `sin(angle)` |
  | `daylight` | `smoothstep(e, −0.1, 0.25)` |
  | `sun_pos` | moon mirrored at night: `(cos·38·dir, \|e\|·38, 13.68)` |
  | `sun_color` | rgb × intensity: day `lerp(SUN_LOW, SUN_DAY, smoothstep(e, 0, 0.5))·2.5·fade`, night `MOON·0.12·fade` |
  | `hemi_sky` | rgb × intensity: `lerp(HEMI_NIGHT, HEMI_DAY, twilight) · lerp(2.1, 1.2, twilight)` |
  | `background` | `lerp(SKY_NIGHT, SKY_DAY, daylight)` then toward `SKY_DUSK` by `dusk·0.6` |
  | `fog_color` | `lerp(FOG_NIGHT, FOG_DAY, daylight)` then toward `SKY_DUSK` by `dusk·0.3` |
  | `fog` | `fog_amount(e, winter)` |

  The hemi ground stays 0x3a3326 × 1.2, as on the web.

### App: `meadow/src/sky_clock.rs`
- **Crate: `jiff` 0.2 with default features.**
  - `TimeZone::system()` reads the Windows zone and maps it to IANA with jiff's bundled CLDR table, e.g. "SE Asia Standard Time" → `Asia/Bangkok`.
  - `Timestamp::now().to_zoned(tz)` gives the local date, the hour and the current offset (DST included).
  - jiff is already in the lockfile through env_logger. The default features add the bundled tzdb (~100–200 KB of exe) and `windows-link`, which is already in the tree.
- **`SkyClock::new(mode, settings_text: Option<String>)`, where `mode` is `Clock`, `Hour(h)` or `Cycle { start_hour: Option<f64> }`:**
  - It reads the system zone once.
  - It picks the place:
    - `parse_place` `Ok(Some)` → the settings place;
    - `Err` → a warning in the log, then the zone guess;
    - `Ok(None)` or no file → the zone guess.
  - It logs once: `sky: zone Asia/Bangkok (UTC+07:00), place 13.75, 100.50 (zone guess|settings.txt), rise 05:52, set 17:48, mode clock|hour 18.50|cycle`.
  - A zone jiff can't name is logged as `unknown`, with the offset-based place.
- **`SkyClock::state(&mut self, anim_time: f64) -> SkyState`, every frame:**
  - Read the clock now, with the local date and offset as they are *now* (midnight, DST and long pauses all just work).
  - Compute `sun_times` and `winterness` for today (a few trig calls).
  - Pick `day_t`:

    | mode | `day_t` |
    |---|---|
    | Clock | `clock_time(local hour)` |
    | Hour(h) | `clock_time(h)` |
    | Cycle | `(day_t at the first frame + anim_time) mod DAY_LENGTH`; `day_t` at the first frame comes from `--hour`, or else from the clock |

  - Return `sky_state(day_t, winter)`.
- `settings.txt` is read once at startup from `%APPDATA%\pixel3d\`. A missing file is silent.

### App: GPU side
- **`camera.rs`:**
  - `light_view_proj(sun_pos: Vec3)`; `SUN_POS` goes away.
  - The sun's z is always 13.68, so `look_at` toward +Y never degenerates.
  - The ±22 box and near/far 1..80 stay, as on the web.
- **`render.rs`:**
  - `Globals` gains `fog: vec4` (rgb colour, w amount), growing from 288 to 304 bytes.
  - `render(…, time, sky: &SkyState)` sets these every frame, before the existing per-frame upload:
    - `sun_dir` (normalised `sun_pos`), `sun_color`, `hemi_sky`, `light_view_proj`;
    - `water_tint = ONE.lerp(background, 0.1)`;
    - `fog`;
    - the scene pass's clear colour = `background`.
  - `BACKGROUND`, `SUN` and `HEMI_SKY` are removed; `HEMI_GROUND` stays.
- **`common.wgsl`:** `cloud_shade(world)` and `ground_fog(world)`, the web's GLSL turned into WGSL on the existing `value_noise`. Cloud time is `g.params.x`, the animation clock.
- **`scene.wgsl`:** `lighting(diffuse, normal, ramp, lit, world)`:
  - the direct term × `cloud_shade(world)`;
  - the result mixed toward `g.fog.rgb` by `ground_fog(world)`.
  - All four callers already have `in.world`.
  - The shadow pass is unchanged; the grass stays lit with the up normal.
- **`main.rs`:**
  - Parse `--hour H` (0 ≤ H ≤ 24, panics otherwise, like `--pixel-size`) and `--cycle`.
  - Hold the `SkyClock` in `App`, and in `redraw` pass `sky.state(time)` into `render`.
  - Update the arg comment.

### Costs
- **CPU:** one zoned clock read plus about 40 trig calls per frame; nothing measurable at 15 fps.
- **GPU:** two noise lookups for the clouds plus two for the fog per fragment, at art resolution.
- **RAM:** jiff's tzdb is in the exe image. Expect < 1 MB working set; measured in T4 only if the user OKs a run.

## Tests
- **Ported 1:1 from `test/sky.test.js`** (EQUINOX {6, 18}, WINTER {6.6, 17.25}). Each JS `new Date(…)` becomes an explicit date with offset +420 min.
  1. The clock-time round trip holds within 1e-6 at the web's 7 times (midnight may show as 23.999…); noon has daylight > 0.99 and midnight daylight = 0.
  2. Rise maps to phase 0 and set to 0.5; `phase_to_hour(hour_to_phase(h))` returns h within 1e-9, for the three day lengths.
  3. Twilight is never darker than midnight, with the web's luminance score (hemi + sun) from 4–8 h and 16–20 h in 5-minute steps.
  4. `fog_amount` auto cases: noon 0, a winter night 1, a summer night < 0.5. The off and always cases wait for sub-project 5.
  5. Sun times:
     - HCMC on 2026-10-07 at +7 h: rise ≈ 5.68 h (within 0.1);
     - London: a day > 16 h on 21 June at +60 min, < 8 h on 21 December at +0;
     - an unknown zone: 12 h ± 0.2;
     - winterness at lat 21 and at −33.
- **New tests:**
  - `parse_place`: valid with spaces, valid with a comment line, missing, malformed, out of range.
  - `day_of_year`: 1 Jan → 1, 31 Dec 2024 → 366, 1 Mar 2026 → 60.
  - The cycle `day_t` maths: starting at `--hour 18.5` shows 18.5 at time 0 and wraps at 300 s.
  - The shadow box holds the visible ground (the screen-corner points of a 21:9 view, about 20 units out) at sun elevations 0.1, 0.5 and 1, on both the morning and the evening side.
  - The Globals size equals the WGSL struct size (naga layout), and `shaders_validate` stays green.

## Tasks
1. **Core:** `sun.rs` + `sky.rs` and their tests. → `cargo test -p meadow-core`.
2. **GPU:** Globals `fog`, `light_view_proj(sun_pos)`, the per-frame sky upload and clear colour, `cloud_shade` / `ground_fog` and the new `lighting`. Until T3, `render` takes a `SkyState` that `main.rs` builds at `NOON`. → `cargo test`.
3. **Clock:** `sky_clock.rs`, jiff, the flags, `settings.txt`, the startup log. → `cargo test`, then a Windows release build; one run with `--hour 12` and a log check.
4. **Windows check (exit):**
   - Windowed release runs at `--hour 6`, `12`, `18.5` and `2` (scratch `$env:APPDATA`), captured with `check.ps1`, side by side with the web at the same hours (vite + Playwright).
   - The rise and set in the log match `node` running `src/sun.js` for the same zone and date.
   - One `--cycle` run watched through dusk.
   - The wallpaper exe is replaced only on user OK, keeping a `.bak`; a RAM row only if the user OKs a run.

## Risks
- **Low-sun shadows past the ±22 box:** the web has the same box, so match it first and widen only if the screenshots show a cut.
- **jiff can't map a Windows zone:** the offset fallback is logged; Vietnam/Thailand map through CLDR "SE Asia Standard Time".
- **Vietnam gets Bangkok's sun:** Windows has one zone ("SE Asia Standard Time") for both, and jiff maps it to `Asia/Bangkok`, so a PC in Vietnam sees sunrise ~26 min late (06:08 vs HCMC's 05:42 on 12 Oct). The `place` line fixes it until sub-project 5's city input.
- **Colour drift:** the web lerps three.js `Color`s, which are linear under `ColorManagement`, so native lerps in linear too. The side-by-side screenshots are the check.
- **Clouds tied to the animation clock** stop moving while paused, as everything else does.
