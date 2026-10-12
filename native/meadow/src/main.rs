// The meadow: ground, paths, pond, trees, rocks and grass, toon-lit with pixel outlines, at the
// PC's local time of day. Settings (pixel size, fps cap, clock or the web's 300 s day, fog, post
// clock) live in %APPDATA%\pixel3d\settings.txt, set from the tray menu in wallpaper mode; there,
// `place = lat, lon` overrides the place guessed from the time zone (sunrise and sunset).
// Args, for this run only: --fps N (60/30/20/15; 0 = no cap, vsync), --pixel-size 2|3|4,
// --wallpaper (Windows: behind the desktop icons, with a tray menu), --hour H (pin the sky at
// local hour H, 0-24, e.g. 18.5), --cycle (the 300 s day, from --hour or the clock),
// --toggle-clock S (break and mend the post clock every S seconds, not saved).
// Backend: Vulkan; override with WGPU_BACKEND=dx12. Release builds have no console: they log to
// %APPDATA%\pixel3d\log.txt.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod camera;
mod gpu;
mod grass;
mod mesh;
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
use winit::window::{Window, WindowId};

use gpu::{Frame, Gpu};
use render::{ClockPose, Renderer};

// winit never reports WindowEvent::Occluded on Windows, so while wgpu says the surface is
// occluded we just try again this often.
const OCCLUDED_RETRY: Duration = Duration::from_millis(250);
const MAX_DT: f64 = 0.1; // s: a hitch or a pause doesn't jump the animation

#[cfg(windows)]
type UserEvent = tray_icon::menu::MenuEvent;
#[cfg(not(windows))]
type UserEvent = ();

struct App {
    settings: Settings, // as loaded, with this run's arguments over them
    gpu: Option<(Gpu, Renderer)>,
    pacer: FramePacer,
    start: Instant,
    resized: Option<PhysicalSize<u32>>, // applied once, just before the next frame
    minimised: bool,
    paused: bool, // wallpaper mode: a maximised or fullscreen window hides the desktop
    retry_at: Option<Instant>, // set while the surface is occluded
    last_frame: Option<Instant>,
    time: f64, // animation clock, s
    sky: sky_clock::SkyClock,
    wreck: Wreck, // the post clock, standing or in pieces
    toggle_clock: Option<(f64, f64)>, // --toggle-clock: every, next (animation time, s)
    frames: u32,
    window_start: Instant,
    #[cfg(windows)]
    desktop: Option<wallpaper::Desktop>, // Some in wallpaper mode
}

impl App {
    fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    fn redraw(&mut self) {
        let Some((gpu, renderer)) = self.gpu.as_mut() else { return };
        if let Some(size) = self.resized.take() {
            gpu.configure(size.width, size.height);
            renderer.resize(&gpu.device, &gpu.queue, size.width, size.height);
        }
        let now = Instant::now();
        let dt = self.last_frame.map_or(0.0, |last| (now - last).as_secs_f64().min(MAX_DT));
        let time = self.time + dt;
        let unix = unix_now();
        if let Some((every, next)) = self.toggle_clock.as_mut()
            && time >= *next
        {
            *next += *every;
            self.wreck.set(!self.wreck.broken(), unix);
        }
        self.wreck.update(dt, unix);
        let sky = self.sky.state(time);
        let clock = ClockPose {
            hands: self.sky.hands(self.wreck.behind(unix)),
            fallen: self.wreck.progress(),
            shiver: self.wreck.shiver(),
            weathering: self.wreck.weathering(unix),
            glow: self.wreck.glow(),
        };
        match gpu.frame(|device, queue, view| renderer.render(device, queue, view, time, dt, &sky, &clock)) {
            Frame::Occluded => {
                if self.retry_at.is_none() {
                    log::info!("occluded: pausing");
                }
                self.retry_at = Some(now + OCCLUDED_RETRY);
                self.last_frame = None; // no catch-up jump when visible again
                return;
            }
            Frame::Skipped => return,
            Frame::Drawn => {}
        }
        self.time += dt;
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
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes().with_title("meadow").with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        #[cfg(windows)]
        if let Some(desktop) = self.desktop.as_mut()
            && !desktop.fallback
        {
            match desktop.window_attributes() {
                Some(behind_icons) => attrs = behind_icons,
                None if desktop.fallback => {}
                None => return, // no desktop yet: the next check tries again
            }
        }
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let gpu = Gpu::new(window);
        let mut renderer = Renderer::new(&gpu.device, gpu.config.format, gpu.config.width, gpu.config.height, self.settings.pixel_size);
        renderer.resize(&gpu.device, &gpu.queue, gpu.config.width, gpu.config.height);
        self.gpu = Some((gpu, renderer));
    }

    /// Wallpaper mode, every CHECK_EVERY: (re)open the window behind the icons if it's gone, and
    /// pause while the desktop is hidden. Returns when to check next (None outside wallpaper mode).
    #[cfg(windows)]
    fn check_desktop(&mut self, event_loop: &ActiveEventLoop) -> Option<Instant> {
        let desktop = self.desktop.as_mut()?;
        let now = Instant::now();
        if now < desktop.next_check {
            return Some(desktop.next_check);
        }
        desktop.next_check = now + wallpaper::CHECK_EVERY;
        let (next, fallback) = (desktop.next_check, desktop.fallback);
        // An Explorer restart destroys our window with the desktop, but winit sends no Destroyed.
        if self.gpu.is_some() && !fallback && !desktop.attached() {
            log::warn!("desktop gone (Explorer restarted?): reopening");
            self.gpu = None;
            desktop.lost();
        }
        if self.gpu.is_none() {
            self.open(event_loop);
            return Some(next);
        }
        let covered = !fallback && wallpaper::desktop_covered();
        if covered != self.paused {
            log::info!("{}", if covered { "desktop covered: pausing" } else { "desktop visible again" });
            self.paused = covered;
            self.last_frame = None; // no catch-up jump
            self.frames = 0; // and the fps log doesn't average over the pause
            self.window_start = now;
            if covered {
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
                if let Some((gpu, renderer)) = self.gpu.as_mut() {
                    renderer.set_pixel_size(n);
                    self.resized = self.resized.or(Some(PhysicalSize::new(gpu.config.width, gpu.config.height)));
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
        }
        save_setting(setting);
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(windows)]
        if let Some(desktop) = self.desktop.as_mut() {
            desktop.show_tray(&self.settings);
        }
        if self.gpu.is_none() {
            self.open(event_loop);
        }
    }

    #[cfg(windows)]
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match self.desktop.as_ref().and_then(|desktop| desktop.menu(&event)) {
            Some(wallpaper::TrayAction::Exit) => {
                log::info!("tray: exit");
                event_loop.exit();
            }
            Some(wallpaper::TrayAction::Set(setting)) => self.apply(setting),
            None => {}
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("close requested: exiting");
                event_loop.exit();
            }
            // Wallpaper mode: an Explorer restart takes our window down with the desktop; the
            // next check opens a new one.
            #[cfg(windows)]
            WindowEvent::Destroyed if self.desktop.is_some() => {
                if self.gpu.take().is_some() {
                    log::warn!("window destroyed with the desktop: reopening");
                }
                self.desktop.as_mut().unwrap().lost();
            }
            WindowEvent::Resized(size) => {
                // A 0 size is a minimised window: keep the old surface and targets untouched.
                self.minimised = size.width == 0 || size.height == 0;
                if !self.minimised {
                    self.resized = Some(size);
                }
                log::info!("resized {}x{}{}", size.width, size.height, if self.minimised { " (paused)" } else { "" });
            }
            WindowEvent::RedrawRequested => {
                if self.minimised || self.paused {
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
        // Frames come far more often than checks, so waiting for the next frame never delays one.
        let check_at = self.check_desktop(event_loop);
        let idle = check_at.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
        let Some((gpu, _)) = self.gpu.as_ref() else {
            event_loop.set_control_flow(idle);
            return;
        };
        if gpu.failed() {
            log::error!("gpu failed: exiting");
            event_loop.exit();
            return;
        }
        if self.minimised || self.paused {
            event_loop.set_control_flow(idle); // the restore Resized event or the next check wakes us
            return;
        }
        let now = Instant::now();
        if let Some(retry) = self.retry_at {
            if now < retry {
                event_loop.set_control_flow(ControlFlow::WaitUntil(retry));
            } else {
                gpu.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + OCCLUDED_RETRY));
            }
            return;
        }
        match self.pacer.next_due(self.settings.fps) {
            // Ask again only once a frame is due; RedrawRequested makes the final call.
            Some(due) => {
                if self.now_ms() >= due - 1.0 {
                    gpu.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.start + Duration::from_secs_f64(due.max(0.0) / 1000.0)));
            }
            None => {
                gpu.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }
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
    let Some(path) = settings_path() else { return };
    let text = match sky_clock::read_settings(&path) {
        Ok(text) => text.unwrap_or_default(),
        Err(e) => return log::warn!("{e}; not saving {setting:?}"),
    };
    let written = std::fs::create_dir_all(path.parent().unwrap()).and_then(|()| std::fs::write(&path, settings::save(&text, setting)));
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
    settings.clock = mode == sky_clock::Mode::Clock;
    let toggle_clock = arg(&args, "--toggle-clock").map(|v| v.parse().ok().filter(|s: &f64| *s > 0.0).unwrap_or_else(|| panic!("--toggle-clock must be seconds, like 8, got {v}")));
    log::info!("settings: {settings:?}");

    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    #[cfg(windows)]
    {
        let proxy = event_loop.create_proxy();
        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(event);
        }));
    }
    let now = Instant::now();
    let mut app = App {
        sky: sky_clock::SkyClock::new(mode, text.as_deref(), settings.fog),
        // Off since `brokenAt` (or now, if that's missing).
        wreck: Wreck::new((!settings.show_clock).then(|| settings.broken_at.map_or_else(unix_now, |t| t as f64))),
        toggle_clock: toggle_clock.map(|every| (every, every)),
        settings,
        gpu: None,
        pacer: FramePacer::default(),
        start: now,
        resized: None,
        minimised: false,
        paused: false,
        retry_at: None,
        last_frame: None,
        time: 0.0,
        frames: 0,
        window_start: now,
        #[cfg(windows)]
        desktop: wallpaper.then(wallpaper::Desktop::new),
    };
    event_loop.run_app(&mut app).expect("event loop");
    log::info!("event loop ended");
}
