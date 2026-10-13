// The meadow: ground, paths, pond, trees, rocks and grass, toon-lit with pixel outlines, at the
// PC's local time of day. Settings (pixel size, fps cap, clock or the web's 300 s day, fog, post
// clock, all screens or the primary's) live in %APPDATA%\pixel3d\settings.txt, set from the tray
// menu in wallpaper mode; there,
// `place = lat, lon` overrides the place guessed from the time zone (sunrise and sunset). A city
// chosen from the tray sets `place` (and `city`, its name) and brings its real cloud cover and
// wind, fetched from Open-Meteo every half hour.
// Args, for this run only: --fps N (60/30/20/15; 0 = no cap, vsync), --pixel-size 2|3|4,
// --wallpaper (Windows: behind the desktop icons, with a tray menu), --hour H (pin the sky at
// local hour H, 0-24, e.g. 18.5), --cycle (the 300 s day, from --hour or the clock),
// --toggle-clock S (break and mend the post clock every S seconds, not saved), --screens
// WxH+X+Y,... (windowed: a window per listed monitor, the first the primary, to check the seams).
// Backend: Vulkan; override with WGPU_BACKEND=dx12. Release builds have no console: they log to
// %APPDATA%\pixel3d\log.txt.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod camera;
#[cfg(windows)]
mod city;
mod gpu;
mod grass;
mod mesh;
#[cfg(windows)]
mod net;
mod render;
mod sky_clock;
#[cfg(windows)]
mod wallpaper;

use std::sync::Arc;
use std::time::{Duration, Instant};

use meadow_core::frame_pacer::{FramePacer, parse_fps};
use meadow_core::settings::{self, Setting, Settings};
use meadow_core::wreck::Wreck;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::{Gpu, Missed, Surface};
use render::{ClockPose, Renderer, Screen};

// winit never reports WindowEvent::Occluded on Windows, so while wgpu says the surfaces are
// occluded we just try again this often.
const OCCLUDED_RETRY: Duration = Duration::from_millis(250);
const MAX_DT: f64 = 0.1; // s: a hitch or a pause doesn't jump the animation

#[cfg(windows)]
pub enum UserEvent {
    Menu(tray_icon::menu::MenuEvent),
    City(meadow_core::weather::City),                                         // chosen in the search
    Weather((f64, f64), Result<meadow_core::weather::Weather, String>), // a fetch for this place came back
}
#[cfg(not(windows))]
type UserEvent = ();

struct App {
    settings: Settings, // as loaded, with this run's arguments over them
    screens: Option<Vec<Screen>>, // --screens: windows laid out as these monitors (windowed mode)
    scene: Option<Scene>,
    pacer: FramePacer,
    start: Instant,
    #[cfg(windows)]
    paused: bool, // wallpaper mode: maximised or fullscreen windows hide every screen's meadow
    retry_at: Option<Instant>, // set while the surfaces are occluded
    last_frame: Option<Instant>,
    time: f64, // animation clock, s
    sky: sky_clock::SkyClock,
    wreck: Wreck, // the post clock, standing or in pieces
    toggle_clock: Option<(f64, f64)>, // --toggle-clock: every, next (animation time, s)
    frames: u32,
    window_start: Instant,
    #[cfg(windows)]
    desktop: Option<wallpaper::Desktop>, // Some in wallpaper mode
    #[cfg(windows)]
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    #[cfg(windows)]
    city: Option<String>, // the chosen city's name
    #[cfg(windows)]
    weather: net::LiveWeather,
}

// The open windows, one per screen (the first is the primary's), their surfaces on one device and
// the meadow drawn into them.
struct Scene {
    gpu: Gpu,
    surfaces: Vec<Surface>,
    renderer: Renderer,
    resized: Vec<Option<PhysicalSize<u32>>>, // applied once, just before the next frame
    minimised: Vec<bool>,
    covered: Vec<bool>, // wallpaper mode: a maximised or fullscreen window hides that screen
}

impl Scene {
    fn index(&self, id: WindowId) -> Option<usize> {
        self.surfaces.iter().position(|s| s.window.id() == id)
    }

    fn drawn(&self) -> Vec<bool> {
        self.minimised.iter().zip(&self.covered).map(|(m, c)| !m && !c).collect()
    }
}

impl App {
    fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    fn redraw(&mut self) {
        let Some(scene) = self.scene.as_mut() else { return };
        // In order: the primary's size scales the others.
        for (i, resized) in scene.resized.iter_mut().enumerate() {
            if let Some(size) = resized.take() {
                scene.gpu.configure(&mut scene.surfaces[i], size.width, size.height);
                scene.renderer.resize(&scene.gpu.device, &scene.gpu.queue, i, size.width, size.height);
            }
        }
        let now = Instant::now();
        let dt = self.last_frame.map_or(0.0, |last| (now - last).as_secs_f64().min(MAX_DT));
        let time = self.time + dt;
        // Stepped on a copy, kept only if the frame is drawn (as `time`): a skipped frame's dt
        // comes round again with the next one.
        let unix = unix_now();
        let mut wreck = self.wreck.clone();
        let toggle = self.toggle_clock.is_some_and(|(_, next)| time >= next);
        if toggle {
            wreck.set(!wreck.broken(), unix);
        }
        wreck.update(dt, unix);
        let (mut frames, mut occluded) = (Vec::new(), false);
        let drawn = scene.drawn();
        for (i, surface) in scene.surfaces.iter_mut().enumerate().filter(|(i, _)| drawn[*i]) {
            match scene.gpu.acquire(surface) {
                Ok(frame) => frames.push((i, frame)),
                Err(Missed::Occluded) => occluded = true,
                Err(Missed::Skipped) => {}
            }
        }
        if frames.is_empty() {
            if occluded {
                if self.retry_at.is_none() {
                    log::info!("occluded: pausing");
                }
                self.retry_at = Some(now + OCCLUDED_RETRY);
                self.last_frame = None; // no catch-up jump when visible again
            }
            return;
        }
        #[cfg(windows)]
        let weather = self.weather.current();
        #[cfg(not(windows))]
        let weather = None;
        let sky = self.sky.state(time, weather.as_ref());
        let clock = ClockPose {
            hands: self.sky.hands(wreck.behind(unix)),
            fallen: wreck.progress(),
            shiver: wreck.shiver(),
            weathering: wreck.weathering(unix),
            glow: wreck.glow(),
        };
        scene.renderer.update(&scene.gpu.queue, time, dt, unix, &sky, &clock, weather.as_ref());
        for (i, frame) in frames {
            let view = frame.0.texture.create_view(&Default::default());
            scene.renderer.render(&scene.gpu.device, &scene.gpu.queue, i, &view);
            scene.gpu.present(&scene.surfaces[i], frame);
        }
        self.time += dt;
        self.wreck = wreck;
        if let Some((every, next)) = self.toggle_clock.as_mut().filter(|_| toggle) {
            *next += *every;
        }
        self.last_frame = Some(now);
        if self.retry_at.take().is_some() {
            log::info!("visible again");
        }
        self.frames += 1;
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_secs(5) {
            log::info!("{:.1} fps", self.frames as f64 / elapsed.as_secs_f64());
            self.frames = 0;
            self.window_start = Instant::now();
        }
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) {
        let normal = Window::default_attributes().with_title("meadow");
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut windows: Vec<(WindowAttributes, Option<Screen>)> = match &self.screens {
            Some(screens) => screens.iter().map(|&sc| (normal.clone().with_inner_size(PhysicalSize::new(sc.size.0, sc.size.1)).with_resizable(false), Some(sc))).collect(),
            None => vec![(normal.with_inner_size(winit::dpi::LogicalSize::new(1280, 720)), None)],
        };
        #[cfg(windows)]
        if let Some(desktop) = self.desktop.as_mut()
            && !desktop.fallback
        {
            match desktop.window_attributes(self.settings.all_screens) {
                Some(behind_icons) => windows = behind_icons.into_iter().map(|(attrs, sc)| (attrs, Some(sc))).collect(),
                None if desktop.fallback => {}
                None => return, // no desktop yet: the next check tries again
            }
        }
        let mut created = windows.into_iter().map(|(attrs, sc)| (Arc::new(event_loop.create_window(attrs).expect("create window")), sc));
        let (first, first_screen) = created.next().expect("a window");
        let (gpu, surface) = Gpu::new(first);
        let format = surface.config.format;
        let mut surfaces = vec![surface];
        let mut placed = vec![first_screen];
        for (window, sc) in created {
            surfaces.push(gpu.surface(window, format));
            placed.push(sc);
        }
        // Sized as the windows came out (a monitor's own size, in wallpaper mode).
        let screens: Vec<Screen> =
            surfaces.iter().zip(placed).map(|(s, sc)| Screen { offset: sc.map_or((0, 0), |sc| sc.offset), size: (s.config.width, s.config.height) }).collect();
        log::info!("screens: {screens:?}");
        let mut renderer = Renderer::new(&gpu.device, format, &screens, self.settings.pixel_size);
        for (i, s) in surfaces.iter().enumerate() {
            renderer.resize(&gpu.device, &gpu.queue, i, s.config.width, s.config.height);
        }
        let n = surfaces.len();
        self.scene = Some(Scene { gpu, surfaces, renderer, resized: vec![None; n], minimised: vec![false; n], covered: vec![false; n] });
    }

    /// Wallpaper mode, every CHECK_EVERY: (re)open the windows behind the icons if they're gone or
    /// the monitors changed, and pause each while its screen is hidden. Returns when to check next
    /// (None outside wallpaper mode).
    #[cfg(windows)]
    fn check_desktop(&mut self, event_loop: &ActiveEventLoop) -> Option<Instant> {
        let desktop = self.desktop.as_mut()?;
        let now = Instant::now();
        if now < desktop.next_check {
            return Some(desktop.next_check);
        }
        desktop.next_check = now + wallpaper::CHECK_EVERY;
        let (next, fallback) = (desktop.next_check, desktop.fallback);
        if self.scene.is_some() && !fallback {
            // An Explorer restart destroys our windows with the desktop, but winit sends no Destroyed.
            if !desktop.attached() {
                log::warn!("desktop gone (Explorer restarted?): reopening");
                self.scene = None;
                desktop.lost();
            } else if desktop.monitors_changed(self.settings.all_screens) {
                log::info!("monitors changed: reopening on {:?}", wallpaper::monitors());
                self.scene = None;
                desktop.lost();
            }
        }
        if self.scene.is_none() {
            self.open(event_loop);
            return Some(next);
        }
        let scene = self.scene.as_mut().unwrap();
        let covered = if fallback { vec![false; scene.covered.len()] } else { self.desktop.as_ref().unwrap().covered() };
        for (i, (&now_covered, was)) in covered.iter().zip(&mut scene.covered).enumerate() {
            if now_covered != *was {
                log::info!("screen {i} {}", if now_covered { "covered" } else { "visible again" });
                *was = now_covered;
            }
        }
        let all = covered.iter().all(|&c| c);
        if all != self.paused {
            log::info!("{}", if all { "desktop covered: pausing" } else { "desktop visible again" });
            self.paused = all;
            self.last_frame = None; // no catch-up jump
            self.frames = 0; // and the fps log doesn't average over the pause
            self.window_start = now;
            if all {
                wallpaper::trim_memory();
            }
        }
        Some(next)
    }

    #[cfg(not(windows))]
    fn check_desktop(&mut self, _: &ActiveEventLoop) -> Option<Instant> {
        None
    }

    /// A setting changed in the tray: applied at once and remembered.
    #[cfg_attr(not(windows), allow(dead_code))] // the tray is Windows only
    fn apply(&mut self, setting: Setting) {
        log::info!("setting: {setting:?}");
        self.settings.set(setting);
        match setting {
            Setting::PixelSize(n) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.renderer.set_pixel_size(n);
                    for (resized, s) in scene.resized.iter_mut().zip(&scene.surfaces) {
                        *resized = resized.or(Some(PhysicalSize::new(s.config.width, s.config.height)));
                    }
                }
            }
            Setting::Fps(_) => {}
            Setting::Clock(on) => self.sky.set_clock(on, self.time),
            Setting::Fog(fog) => self.sky.fog = fog,
            Setting::ShowClock(on) => {
                self.wreck.set(!on, unix_now());
                // The hands stop where they are (even mid catch-up): the pieces' age counts from then.
                if let Some(stopped) = self.wreck.stopped().filter(|_| !on) {
                    let broken_at = Setting::BrokenAt(stopped.round() as i64);
                    self.settings.set(broken_at);
                    save_setting(broken_at);
                }
            }
            Setting::BrokenAt(_) => {}
            Setting::AllScreens(_) => {
                // Reopened on the new monitors at once.
                #[cfg(windows)]
                if let Some(desktop) = self.desktop.as_mut().filter(|d| !d.fallback) {
                    self.scene = None;
                    desktop.lost();
                    desktop.next_check = Instant::now();
                }
            }
        }
        save_setting(setting);
    }

    /// A city was chosen in the search: its sun and weather from now on, and remembered.
    #[cfg(windows)]
    fn choose_city(&mut self, city: meadow_core::weather::City) {
        log::info!("city: {}, {} ({:.4}, {:.4})", city.name, city.detail, city.lat, city.lon);
        self.sky.set_place((city.lat, city.lon));
        self.weather.set_place((city.lat, city.lon));
        if let Some(desktop) = self.desktop.as_ref() {
            desktop.set_city(&city.name);
        }
        rewrite_settings("the city", |text| settings::save_city(text, &city));
        self.city = Some(city.name);
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(windows)]
        if let Some(desktop) = self.desktop.as_mut() {
            desktop.show_tray(&self.settings, self.city.as_deref());
        }
        if self.scene.is_none() {
            self.open(event_loop);
        }
    }

    #[cfg(windows)]
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let event = match event {
            UserEvent::Menu(event) => event,
            UserEvent::City(city) => return self.choose_city(city),
            UserEvent::Weather(place, reply) => return self.weather.got(place, reply),
        };
        match self.desktop.as_ref().and_then(|desktop| desktop.menu(&event)) {
            Some(wallpaper::TrayAction::Exit) => {
                log::info!("tray: exit");
                event_loop.exit();
            }
            Some(wallpaper::TrayAction::Set(setting)) => self.apply(setting),
            Some(wallpaper::TrayAction::City) => city::open(self.proxy.clone(), self.city.clone()),
            None => {}
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("close requested: exiting");
                event_loop.exit();
            }
            // Wallpaper mode: an Explorer restart takes our windows down with the desktop; the
            // next check opens new ones. Windows we closed ourselves are no longer in the scene.
            #[cfg(windows)]
            WindowEvent::Destroyed if self.desktop.is_some() => {
                if self.scene.as_ref().and_then(|scene| scene.index(id)).is_some() {
                    log::warn!("window destroyed with the desktop: reopening");
                    self.scene = None;
                    self.desktop.as_mut().unwrap().lost();
                }
            }
            WindowEvent::Resized(size) => {
                let Some((scene, i)) = self.scene.as_mut().and_then(|scene| scene.index(id).map(|i| (scene, i))) else { return };
                // A 0 size is a minimised window: keep the old surface and targets untouched.
                scene.minimised[i] = size.width == 0 || size.height == 0;
                if !scene.minimised[i] {
                    scene.resized[i] = Some(size);
                }
                log::info!("window {i} resized {}x{}{}", size.width, size.height, if scene.minimised[i] { " (paused)" } else { "" });
            }
            WindowEvent::RedrawRequested => {
                if !self.scene.as_ref().is_some_and(|scene| scene.drawn().contains(&true)) {
                    return;
                }
                // Uncapped, vsync paces the loop and every redraw draws.
                let fps = self.settings.fps;
                if fps.is_none() || self.pacer.should_draw(self.now_ms(), fps) {
                    self.redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(windows)]
        self.weather.poll(&self.proxy);
        // Frames come far more often than checks, so waiting for the next frame never delays one.
        let check_at = self.check_desktop(event_loop);
        let idle = check_at.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
        let Some(scene) = self.scene.as_ref() else {
            event_loop.set_control_flow(idle);
            return;
        };
        if scene.gpu.failed() {
            log::error!("gpu failed: exiting");
            event_loop.exit();
            return;
        }
        // One window asks for every frame; with all minimised or covered, the restore Resized
        // event or the next check wakes us.
        let Some(i) = scene.drawn().iter().position(|&d| d) else {
            event_loop.set_control_flow(idle);
            return;
        };
        let window = &scene.surfaces[i].window;
        let now = Instant::now();
        if let Some(retry) = self.retry_at {
            if now < retry {
                event_loop.set_control_flow(ControlFlow::WaitUntil(retry));
            } else {
                window.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + OCCLUDED_RETRY));
            }
            return;
        }
        match self.pacer.next_due(self.settings.fps) {
            // Ask again only once a frame is due; RedrawRequested makes the final call.
            Some(due) => {
                if self.now_ms() >= due - 1.0 {
                    window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.start + Duration::from_secs_f64(due.max(0.0) / 1000.0)));
            }
            None => {
                window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }
}

/// `WxH+X+Y,...`: the first is the primary; the others' corners are taken relative to its.
fn parse_screens(v: &str) -> Option<Vec<Screen>> {
    let rects = v
        .split(',')
        .map(|r| {
            let (size, pos) = r.split_once('+')?;
            let (w, h) = size.split_once('x')?;
            let (x, y) = pos.split_once('+')?;
            Some(((x.parse::<i32>().ok()?, y.parse::<i32>().ok()?), (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        })
        .collect::<Option<Vec<_>>>()?;
    let ((x0, y0), _) = rects[0];
    Some(rects.into_iter().map(|((x, y), size)| Screen { offset: (x - x0, y - y0), size }).collect())
}

fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn log_file() -> Option<std::fs::File> {
    let dir = std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("pixel3d");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::File::create(dir.join("log.txt")).ok()
}

fn settings_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("pixel3d").join("settings.txt"))
}

fn settings_text() -> Option<String> {
    sky_clock::read_settings(&settings_path()?).unwrap_or_else(|e| {
        log::warn!("{e}; using the default settings and the zone guess");
        None
    })
}

/// Rewrites the setting's line in settings.txt (creating the file and its folder). Failures are
/// logged and otherwise ignored, as the web's; an unreadable file is left alone rather than lost.
fn save_setting(setting: Setting) {
    rewrite_settings(&format!("{setting:?}"), |text| settings::save(text, setting));
}

fn rewrite_settings(what: &str, edit: impl FnOnce(&str) -> String) {
    let Some(path) = settings_path() else { return };
    let text = match sky_clock::read_settings(&path) {
        Ok(text) => text.unwrap_or_default(),
        Err(e) => return log::warn!("{e}; not saving {what}"),
    };
    let written = std::fs::create_dir_all(path.parent().unwrap()).and_then(|()| std::fs::write(&path, edit(&text)));
    if let Err(e) = written {
        log::warn!("saving {}: {e}", path.display());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let wallpaper = args.iter().any(|a| a == "--wallpaper");
    #[cfg(windows)]
    if wallpaper && wallpaper::already_running() {
        return; // before the logger: the running one keeps its log file
    }
    #[cfg(not(windows))]
    assert!(!wallpaper, "--wallpaper is Windows only");
    let mut logger = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("meadow=info,wgpu_core=warn,wgpu_hal=warn"));
    if cfg!(all(windows, not(debug_assertions)))
        && let Some(file) = log_file()
    {
        logger.target(env_logger::Target::Pipe(Box::new(file)));
        std::panic::set_hook(Box::new(|info| log::error!("{info}"))); // no console to print to
    }
    logger.init();
    let text = settings_text();
    let mut settings = Settings::load(text.as_deref().unwrap_or(""));
    match arg(&args, "--fps").as_deref() {
        None => {}
        Some("0") => settings.fps = None,
        Some(v) => settings.fps = Some(parse_fps(v).unwrap_or_else(|| panic!("--fps must be 0, 60, 30, 20 or 15, got {v}"))),
    }
    if let Some(v) = arg(&args, "--pixel-size") {
        settings.pixel_size = v.parse().ok().filter(|n| settings::PIXEL_SIZES.contains(n)).unwrap_or_else(|| panic!("--pixel-size must be 2, 3 or 4, got {v}"));
    }
    let hour = arg(&args, "--hour").map(|v| v.parse().ok().filter(|h: &f64| (0.0..=24.0).contains(h)).unwrap_or_else(|| panic!("--hour must be 0 to 24, like 18.5, got {v}")));
    let mode = match (args.iter().any(|a| a == "--cycle"), hour) {
        (true, start) => sky_clock::Mode::Cycle(start),
        (false, Some(h)) => sky_clock::Mode::Hour(h),
        (false, None) if settings.clock => sky_clock::Mode::Clock,
        (false, None) => sky_clock::Mode::Cycle(None),
    };
    // --hour pins the sky for this run only: the tray still shows the saved choice.
    if !matches!(mode, sky_clock::Mode::Hour(_)) {
        settings.clock = mode == sky_clock::Mode::Clock;
    }
    let screens = arg(&args, "--screens").map(|v| parse_screens(&v).unwrap_or_else(|| panic!("--screens must be like 1280x720+0+0,960x540+170+720, got {v}")));
    let toggle_clock = arg(&args, "--toggle-clock").map(|v| v.parse().ok().filter(|s: &f64| *s > 0.0).unwrap_or_else(|| panic!("--toggle-clock must be seconds, like 8, got {v}")));
    log::info!("settings: {settings:?}");

    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    #[cfg(windows)]
    {
        let proxy = event_loop.create_proxy();
        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }));
    }
    let now = Instant::now();
    let mut app = App {
        sky: sky_clock::SkyClock::new(mode, text.as_deref(), settings.fog),
        // Off since `brokenAt` (or now, if that's missing).
        wreck: Wreck::new((!settings.show_clock).then(|| settings.broken_at.map_or_else(unix_now, |t| t as f64))),
        toggle_clock: toggle_clock.map(|every| (every, every)),
        settings,
        screens,
        scene: None,
        pacer: FramePacer::default(),
        start: now,
        #[cfg(windows)]
        paused: false,
        retry_at: None,
        last_frame: None,
        time: 0.0,
        frames: 0,
        window_start: now,
        #[cfg(windows)]
        desktop: wallpaper.then(wallpaper::Desktop::new),
        #[cfg(windows)]
        proxy: event_loop.create_proxy(),
        #[cfg(windows)]
        city: text.as_deref().and_then(settings::city).map(str::to_string),
        #[cfg(windows)]
        weather: net::LiveWeather::new(text.as_deref().and_then(|t| meadow_core::sun::parse_place(t).ok().flatten())),
    };
    event_loop.run_app(&mut app).expect("event loop");
    log::info!("event loop ended");
}
