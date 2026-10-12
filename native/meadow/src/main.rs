// The meadow at noon: ground, paths, pond, trees and rocks, toon-lit with pixel outlines.
// Args: --fps N (60/30/20/15, default 15; 0 = no cap, vsync), --pixel-size 2|3|4 (default 2).
// Backend override: WGPU_BACKEND=dx12|vulkan.
mod camera;
mod gpu;
mod mesh;
mod render;

use std::sync::Arc;
use std::time::{Duration, Instant};

use meadow_core::frame_pacer::{FramePacer, parse_fps};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use gpu::{Frame, Gpu};
use render::Renderer;

// winit never reports WindowEvent::Occluded on Windows, so while wgpu says the surface is
// occluded we just try again this often.
const OCCLUDED_RETRY: Duration = Duration::from_millis(250);
const MAX_DT: f64 = 0.1; // s: a hitch or a pause doesn't jump the animation

struct App {
    fps: Option<u32>, // None: no cap, vsync paces the loop
    pixel_size: u32,
    gpu: Option<(Gpu, Renderer)>,
    pacer: FramePacer,
    start: Instant,
    resized: Option<PhysicalSize<u32>>, // applied once, just before the next frame
    minimised: bool,
    retry_at: Option<Instant>, // set while the surface is occluded
    last_frame: Option<Instant>,
    time: f64, // animation clock, s
    frames: u32,
    window_start: Instant,
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
        let time = (self.time + dt) as f32;
        match gpu.frame(|device, queue, view| renderer.render(device, queue, view, time)) {
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
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("meadow").with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let gpu = Gpu::new(window);
        let mut renderer = Renderer::new(&gpu.device, gpu.config.format, gpu.config.width, gpu.config.height, self.pixel_size);
        renderer.resize(&gpu.device, &gpu.queue, gpu.config.width, gpu.config.height);
        self.gpu = Some((gpu, renderer));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                // A 0 size is a minimised window: keep the old surface and targets untouched.
                self.minimised = size.width == 0 || size.height == 0;
                if !self.minimised {
                    self.resized = Some(size);
                }
                log::info!("resized {}x{}{}", size.width, size.height, if self.minimised { " (paused)" } else { "" });
            }
            WindowEvent::RedrawRequested => {
                if self.minimised {
                    return;
                }
                // Uncapped, vsync paces the loop and every redraw draws.
                if self.fps.is_none() || self.pacer.should_draw(self.now_ms(), self.fps) {
                    self.redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some((gpu, _)) = self.gpu.as_ref() else { return };
        if gpu.failed() {
            log::error!("gpu failed: exiting");
            event_loop.exit();
            return;
        }
        if self.minimised {
            event_loop.set_control_flow(ControlFlow::Wait); // the restore Resized event wakes us
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
        match self.pacer.next_due(self.fps) {
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

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("meadow=info,wgpu_core=warn,wgpu_hal=warn")).init();
    let args: Vec<String> = std::env::args().collect();
    let fps = match arg(&args, "--fps").as_deref() {
        None => Some(15),
        Some("0") => None,
        Some(v) => Some(parse_fps(v).unwrap_or_else(|| panic!("--fps must be 0, 60, 30, 20 or 15, got {v}"))),
    };
    let pixel_size = match arg(&args, "--pixel-size").as_deref() {
        None => 2,
        Some(v) => v.parse().ok().filter(|n| (2..=4).contains(n)).unwrap_or_else(|| panic!("--pixel-size must be 2, 3 or 4, got {v}")),
    };
    log::info!("fps cap: {}, pixel size {pixel_size}", fps.map_or("none (vsync)".into(), |f| f.to_string()));

    let event_loop = EventLoop::new().expect("event loop");
    let now = Instant::now();
    let mut app = App {
        fps,
        pixel_size,
        gpu: None,
        pacer: FramePacer::default(),
        start: now,
        resized: None,
        minimised: false,
        retry_at: None,
        last_frame: None,
        time: 0.0,
        frames: 0,
        window_start: now,
    };
    event_loop.run_app(&mut app).expect("event loop");
}
