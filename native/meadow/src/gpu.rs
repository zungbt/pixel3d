//! Device and queue, shared by every window, and each window's surface. The Instance is kept so a
//! lost surface can be recreated.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use winit::window::Window;

pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    failed: Arc<AtomicBool>, // set by the device-lost / uncaptured-error callbacks
}

/// A window and its swapchain.
pub struct Surface {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

/// Why a surface gave no texture this frame.
pub enum Missed {
    Occluded,
    Skipped,
}

impl Gpu {
    /// The device is picked to present to the first window's surface; with `timestamps` (--gpu-time)
    /// it gets timestamp queries if the adapter has them.
    pub fn new(window: Arc<Window>, timestamps: bool) -> (Self, Surface) {
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
        let timestamps = timestamps && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: if timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() },
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
        let gpu = Self { instance, adapter, device, queue, failed };
        let surface = gpu.configure_new(window, surface, None);
        (gpu, surface)
    }

    /// A surface for another window on the same device, in the first's format (the pipelines'),
    /// so all monitors on one GPU share it.
    pub fn surface(&self, window: Arc<Window>, format: wgpu::TextureFormat) -> Surface {
        let surface = self.instance.create_surface(window.clone()).expect("create surface");
        assert!(self.adapter.is_surface_supported(&surface), "adapter can't present to this window");
        self.configure_new(window, surface, Some(format))
    }

    fn configure_new(&self, window: Arc<Window>, surface: wgpu::Surface<'static>, format: Option<wgpu::TextureFormat>) -> Surface {
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&self.adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported by adapter");
        let caps = surface.get_capabilities(&self.adapter);
        config.format = format.unwrap_or_else(|| caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]));
        assert!(caps.formats.contains(&config.format), "surface can't take {:?}", config.format);
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&self.device, &config);
        log::info!("surface: {:?} {}x{}", config.format, config.width, config.height);
        Surface { window, surface, config }
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Never called with a 0 size: configuring a 0x0 surface panics.
    pub fn configure(&self, s: &mut Surface, width: u32, height: u32) {
        s.config.width = width;
        s.config.height = height;
        s.surface.configure(&self.device, &s.config);
    }

    /// The next swapchain texture, recovering the surface when it is outdated or lost.
    pub fn acquire(&self, s: &mut Surface) -> Result<(wgpu::SurfaceTexture, bool), Missed> {
        match s.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => Ok((f, false)),
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => Ok((f, true)),
            wgpu::CurrentSurfaceTexture::Occluded => Err(Missed::Occluded),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Validation => Err(Missed::Skipped),
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("surface outdated: reconfiguring");
                s.surface.configure(&self.device, &s.config);
                Err(Missed::Skipped)
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("surface lost: recreating");
                s.surface = self.instance.create_surface(s.window.clone()).expect("recreate surface");
                assert!(self.adapter.is_surface_supported(&s.surface), "adapter can't present to the new surface");
                s.surface.configure(&self.device, &s.config);
                Err(Missed::Skipped)
            }
        }
    }

    /// Presents a texture from `acquire`, reconfiguring after if it was suboptimal.
    pub fn present(&self, s: &Surface, (frame, suboptimal): (wgpu::SurfaceTexture, bool)) {
        s.window.pre_present_notify();
        self.queue.present(frame);
        if suboptimal {
            s.surface.configure(&self.device, &s.config);
        }
    }
}
