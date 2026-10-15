//! --gpu-time: the GPU's time in each pass of the first view (PASSES), averaged and
//! logged with the fps. Timestamp queries read back without stalling: while one frame's stamps are
//! on their way back, the next frames go untimed.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const PASSES: [&str; 4] = ["shadow", "clouds", "scene", "post"];
const STAMPS: u64 = 2 * PASSES.len() as u64;

pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    ns_per_tick: f64,
    pending: bool,           // a readback is in flight: this frame isn't timed
    mapped: Arc<AtomicBool>, // set by its map callback
    sums: [f64; PASSES.len()], // ms
    samples: u32,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let buffer = |label, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: STAMPS * 8, usage, mapped_at_creation: false });
        Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("gpu timer"), ty: wgpu::QueryType::Timestamp, count: STAMPS as u32 }),
            resolve: buffer("gpu timer resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            readback: buffer("gpu timer readback", wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ),
            ns_per_tick: queue.get_timestamp_period() as f64,
            pending: false,
            mapped: Arc::new(AtomicBool::new(false)),
            sums: [0.0; PASSES.len()],
            samples: 0,
        }
    }

    /// The stamps for pass `i` (of PASSES), or None while this frame isn't timed.
    pub fn writes(&self, i: u32) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        (!self.pending).then_some(wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(2 * i),
            end_of_pass_write_index: Some(2 * i + 1),
        })
    }

    /// After the passes, in the same encoder: copies a timed frame's stamps out.
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.pending {
            encoder.resolve_query_set(&self.queries, 0..STAMPS as u32, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, STAMPS * 8);
        }
    }

    /// After the submit: starts reading a timed frame back, or picks up the reading in flight.
    pub fn submitted(&mut self, device: &wgpu::Device) {
        if !self.pending {
            let flag = self.mapped.clone();
            self.readback.map_async(wgpu::MapMode::Read, .., move |r| flag.store(r.is_ok(), Ordering::Release));
            self.pending = true;
            return;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if !self.mapped.swap(false, Ordering::Acquire) {
            return;
        }
        {
            let bytes = self.readback.get_mapped_range(..).expect("mapped");
            let stamps: &[u64] = bytemuck::cast_slice(&bytes);
            for (sum, pair) in self.sums.iter_mut().zip(stamps.chunks(2)) {
                *sum += pair[1].wrapping_sub(pair[0]) as f64 * self.ns_per_tick / 1e6;
            }
        }
        self.readback.unmap();
        self.samples += 1;
        self.pending = false;
    }

    /// The mean ms of each pass since the last call, if any frame was timed.
    pub fn take(&mut self) -> Option<[f64; PASSES.len()]> {
        let n = std::mem::take(&mut self.samples);
        let sums = std::mem::take(&mut self.sums);
        (n > 0).then(|| sums.map(|s| s / n as f64))
    }
}
