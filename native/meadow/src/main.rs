// Day-1 spike: a window cleared to the night-sky colour plus an egui label, to check the
// cross-compiled exe runs on Windows, which backend it gets, the fps cap and the RAM floor.
// Args: --fps N (0 = no cap, vsync). Backend override: WGPU_BACKEND=dx12|vulkan.
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const STALL: Duration = Duration::from_millis(250); // a longer gap restarts the schedule
// winit never reports WindowEvent::Occluded on Windows, so while wgpu says the surface is
// occluded we just try again this often.
const OCCLUDED_RETRY: Duration = Duration::from_millis(250);

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    adapter: String,
    egui: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no adapter");
        let info = adapter.get_info();
        let adapter_name = format!("{} ({:?})", info.name, info.backend);
        log::info!("adapter: {adapter_name}, driver {}", info.driver_info);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("request device");

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported by adapter");
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        log::info!("surface: {:?} {}x{}", config.format, config.width, config.height);

        let ctx = egui::Context::default();
        let egui = egui_winit::State::new(ctx, egui::ViewportId::ROOT, &window, None, None, None);
        let egui_renderer = egui_wgpu::Renderer::new(&device, config.format, Default::default());
        Self { window, surface, device, queue, config, adapter: adapter_name, egui, egui_renderer }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // minimised: configuring a 0x0 surface panics
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    // Returns false when the window is occluded and drawing should pause.
    fn render(&mut self, fps: f64) -> bool {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Occluded => return false,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Validation => return true,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("surface outdated/lost: reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return true;
            }
        };
        let view = frame.texture.create_view(&Default::default());

        let input = self.egui.take_egui_input(&self.window);
        let ctx = self.egui.egui_ctx().clone();
        let out = ctx.run_ui(input, |ui| {
            egui::Window::new("meadow spike").show(ui.ctx(), |ui| {
                ui.label(&self.adapter);
                ui.label(format!("{fps:.1} fps"));
            });
        });
        self.egui.handle_platform_output(&self.window, out.platform_output);
        let jobs = ctx.tessellate(out.shapes, out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: out.pixels_per_point,
        };
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.egui_renderer.update_texture(&self.device, &self.queue, *id, delta);
            }
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut cmds = self.egui_renderer.update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear + egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        // 0x151729, the web scene.background, in linear
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0075, g: 0.0080, b: 0.0222, a: 1.0 }), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut pass, &jobs, &screen);
        }
        cmds.push(encoder.finish());
        self.queue.submit(cmds);
        self.window.pre_present_notify();
        self.queue.present(frame);
        for id in &out.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        true
    }
}

struct App {
    cap: Option<Duration>, // None: no cap, vsync paces the loop
    gpu: Option<Gpu>,
    minimised: bool,
    retry_at: Option<Instant>, // set while the surface is occluded
    next: Instant,
    frames: u32,
    window_start: Instant,
    fps: f64,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("meadow").with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        self.gpu = Some(Gpu::new(window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let _ = gpu.egui.on_window_event(&gpu.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.minimised = size.width == 0 || size.height == 0;
                gpu.resize(size.width, size.height);
                log::info!("resized {}x{}{}", size.width, size.height, if self.minimised { " (paused)" } else { "" });
            }
            WindowEvent::RedrawRequested => {
                if !gpu.render(self.fps) {
                    if self.retry_at.is_none() {
                        log::info!("occluded: pausing");
                    }
                    self.retry_at = Some(Instant::now() + OCCLUDED_RETRY);
                    return;
                }
                if self.retry_at.take().is_some() {
                    log::info!("visible again");
                }
                self.frames += 1;
                let elapsed = self.window_start.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    self.fps = self.frames as f64 / elapsed.as_secs_f64();
                    log::info!("{:.1} fps", self.fps);
                    self.frames = 0;
                    self.window_start = Instant::now();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gpu) = self.gpu.as_ref() else { return };
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
        let Some(interval) = self.cap else {
            gpu.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        };
        if now >= self.next {
            gpu.window.request_redraw();
            // Step along the schedule so rounding doesn't drift; restart it after a stall.
            self.next = if now - self.next > STALL { now + interval } else { self.next + interval };
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("meadow=info,wgpu_core=warn,wgpu_hal=warn")).init();
    let args: Vec<String> = std::env::args().collect();
    let fps: u32 = args.iter().position(|a| a == "--fps").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
    log::info!("fps cap: {}", if fps == 0 { "none (vsync)".into() } else { fps.to_string() });

    let event_loop = EventLoop::new().expect("event loop");
    let now = Instant::now();
    let mut app = App {
        cap: (fps > 0).then(|| Duration::from_secs_f64(1.0 / fps as f64)),
        gpu: None,
        minimised: false,
        retry_at: None,
        next: now,
        frames: 0,
        window_start: now,
        fps: 0.0,
    };
    event_loop.run_app(&mut app).expect("event loop");
}
