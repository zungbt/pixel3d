//! Device, queue and the window's surface. The Instance is kept so a lost surface can be recreated.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use winit::window::Window;

pub struct Gpu {
    pub window: Arc<Window>,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    failed: Arc<AtomicBool>, // set by the device-lost / uncaptured-error callbacks
}

pub enum Frame {
    Drawn,
    Occluded,
    Skipped,
}

impl Gpu {
    pub fn new(window: Arc<Window>) -> Self {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::VULKAN; // probing DX12 and GL as well loads their DLLs: +14 MB
        let instance = wgpu::Instance::new(desc.with_env());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no adapter");
        let info = adapter.get_info();
        log::info!("adapter: {} ({:?}), driver {}", info.name, info.backend, info.driver_info);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("request device");

        // wgpu 30 has no out-of-memory variant to recover from: log and let the app exit.
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        device.set_device_lost_callback(move |reason, msg| {
            log::error!("device lost ({reason:?}): {msg}");
            flag.store(true, Ordering::Relaxed);
        });
        let flag = failed.clone();
        device.on_uncaptured_error(Arc::new(move |e| {
            log::error!("gpu error: {e}");
            flag.store(true, Ordering::Relaxed);
        }));

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported by adapter");
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        log::info!("surface: {:?} {}x{}", config.format, config.width, config.height);
        Self { window, instance, adapter, surface, device, queue, config, failed }
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Never called with a 0 size: configuring a 0x0 surface panics.
    pub fn configure(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Acquires the next swapchain texture, hands it to `draw`, presents it, and recovers the
    /// surface when it is outdated, suboptimal or lost.
    pub fn frame(&mut self, draw: impl FnOnce(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView)) -> Frame {
        let (frame, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => (f, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => (f, true),
            wgpu::CurrentSurfaceTexture::Occluded => return Frame::Occluded,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Validation => return Frame::Skipped,
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("surface outdated: reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return Frame::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("surface lost: recreating");
                self.surface = self.instance.create_surface(self.window.clone()).expect("recreate surface");
                assert!(self.adapter.is_surface_supported(&self.surface), "adapter can't present to the new surface");
                self.surface.configure(&self.device, &self.config);
                return Frame::Skipped;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        draw(&self.device, &self.queue, &view);
        self.window.pre_present_notify();
        self.queue.present(frame);
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }
        Frame::Drawn
    }
}
