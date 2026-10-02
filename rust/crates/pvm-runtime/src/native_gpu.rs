use crate::gpu_wire::{self, GpuOpcode};
use anyhow::{anyhow, bail, Context, Result};
use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const FORMAT_RGBA8_UNORM: u16 = 1;
const EVENT_HEADER_BYTES: usize = 24;
const MAX_COMPUTE_WORKGROUP_STORAGE_SIZE: u32 = 16 * 1024;
const MAX_STORAGE_BUFFERS_PER_SHADER_STAGE: u32 = 8;
const MAX_COMPUTE_WORKGROUPS_PER_DIMENSION: u32 = 65_535;

// Match the browser worker's resource and per-batch allocation budgets. Charges
// outlive guest handles when a dependent object or an encoded batch retains them.
const RESOURCE_LIMITS: [usize; 10] = [
    gpu_wire::MAX_GPU_BUFFERS,
    gpu_wire::MAX_GPU_TEXTURES,
    gpu_wire::MAX_GPU_TEXTURE_VIEWS,
    gpu_wire::MAX_GPU_SAMPLERS,
    gpu_wire::MAX_GPU_SHADER_MODULES,
    gpu_wire::MAX_GPU_BIND_GROUP_LAYOUTS,
    gpu_wire::MAX_GPU_PIPELINE_LAYOUTS,
    gpu_wire::MAX_GPU_BIND_GROUPS,
    gpu_wire::MAX_GPU_RENDER_PIPELINES,
    gpu_wire::MAX_GPU_RENDER_PIPELINES,
];

#[derive(Default)]
struct Quotas {
    counts: [AtomicUsize; 10],
    buffer_bytes: AtomicUsize,
    texture_bytes: AtomicUsize,
}

struct Charge {
    quotas: Arc<Quotas>,
    kind: usize,
    bytes: usize,
    _dependencies: Vec<Arc<Charge>>,
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.quotas.counts[self.kind].fetch_sub(1, Ordering::Relaxed);
        if self.kind == 0 {
            self.quotas
                .buffer_bytes
                .fetch_sub(self.bytes, Ordering::Relaxed);
        } else if self.kind == 1 {
            self.quotas
                .texture_bytes
                .fetch_sub(self.bytes, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
struct BatchBudget {
    counts: [usize; 10],
    buffer_bytes: usize,
    texture_bytes: usize,
    upload_bytes: usize,
    compilations: usize,
    renders: usize,
    computes: usize,
    draws: usize,
}

fn add_budget(value: &mut usize, amount: usize, limit: usize) -> Result<()> {
    let next = value
        .checked_add(amount)
        .filter(|next| *next <= limit)
        .ok_or_else(|| anyhow!("GPU resource or batch budget exceeded"))?;
    *value = next;
    Ok(())
}

impl Quotas {
    fn reserve(
        self: &Arc<Self>,
        kind: usize,
        bytes: usize,
        dependencies: Vec<Arc<Charge>>,
        batch: &mut BatchBudget,
    ) -> Result<Arc<Charge>> {
        if self.counts[kind].load(Ordering::Relaxed) >= RESOURCE_LIMITS[kind] {
            bail!("GPU resource count quota exceeded");
        }
        let (used, limit) = match kind {
            0 => (
                self.buffer_bytes.load(Ordering::Relaxed),
                gpu_wire::MAX_GPU_TOTAL_BUFFER_BYTES,
            ),
            1 => (
                self.texture_bytes.load(Ordering::Relaxed),
                gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES,
            ),
            _ => (0, 0),
        };
        if bytes > limit.saturating_sub(used) {
            bail!("GPU resource memory quota exceeded");
        }
        add_budget(&mut batch.counts[kind], 1, RESOURCE_LIMITS[kind])?;
        match kind {
            0 => add_budget(&mut batch.buffer_bytes, bytes, limit)?,
            1 => add_budget(&mut batch.texture_bytes, bytes, limit)?,
            4 | 8 | 9 => add_budget(&mut batch.compilations, 1, gpu_wire::MAX_GPU_COMPILATIONS)?,
            _ => {}
        }
        self.counts[kind].fetch_add(1, Ordering::Relaxed);
        if kind == 0 {
            self.buffer_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        if kind == 1 {
            self.texture_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        Ok(Arc::new(Charge {
            quotas: Arc::clone(self),
            kind,
            bytes,
            _dependencies: dependencies,
        }))
    }
}

/// A tightly packed RGBA8 image read back from the native GPU surface.
#[derive(Debug)]
pub struct NativeGpuFrame {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Row-major RGBA8 pixels, without row padding.
    pub rgba: Vec<u8>,
}

/// Guest events and an optional frame produced by a submission.
#[derive(Debug)]
pub struct NativeGpuOutput {
    /// Encoded GPU completion or rejection events.
    pub events: Vec<Vec<u8>>,
    /// Surface pixels when the batch completed a render pass.
    pub frame: Option<NativeGpuFrame>,
}

enum Resource {
    Buffer { value: wgpu::Buffer, size: u64 },
    Texture(wgpu::Texture),
    TextureView(wgpu::TextureView),
    Sampler(wgpu::Sampler),
    Shader(wgpu::ShaderModule),
    BindGroupLayout(wgpu::BindGroupLayout),
    PipelineLayout(wgpu::PipelineLayout),
    BindGroup(wgpu::BindGroup),
    RenderPipeline(wgpu::RenderPipeline),
    ComputePipeline(wgpu::ComputePipeline),
}

struct PendingPass {
    depth_view: u32,
    flags: u32,
    clear_color: wgpu::Color,
    clear_depth: f32,
    operations: Vec<RenderOperation>,
}

enum RenderOperation {
    Pipeline(u32),
    VertexBuffer {
        slot: u32,
        buffer: u32,
        offset: u64,
        size: u64,
    },
    IndexBuffer {
        buffer: u32,
        format: wgpu::IndexFormat,
        offset: u64,
        size: u64,
    },
    BindGroup {
        slot: u32,
        bind_group: u32,
        offsets: Vec<u32>,
    },
    Viewport([f32; 6]),
    Scissor([u32; 4]),
    Draw([u32; 4]),
    DrawIndexed {
        indices: u32,
        instances: u32,
        first_index: u32,
        base_vertex: i32,
        first_instance: u32,
    },
}

struct PendingComputePass {
    operations: Vec<ComputeOperation>,
}

enum ComputeOperation {
    Pipeline(u32),
    BindGroup {
        slot: u32,
        bind_group: u32,
        offsets: Vec<u32>,
    },
    Dispatch([u32; 3]),
}

/// Bounded native executor for the guest GPU wire protocol.
pub struct NativeGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resources: HashMap<u32, Resource>,
    charges: HashMap<u32, Arc<Charge>>,
    quotas: Arc<Quotas>,
    surface: wgpu::Texture,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row_bytes: u32,
    generation: u32,
}

impl NativeGpuRenderer {
    /// Creates an off-screen renderer with a surface of at most 4096×4096 pixels.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            bail!("invalid native GPU surface dimensions");
        }
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow!("native WebGPU adapter is unavailable"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("PolkaVM native GPU"),
                required_features: wgpu::Features::empty(),
                required_limits: native_required_limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .context("create native WebGPU device")?;
        let (surface, readback, padded_row_bytes) = create_surface(&device, width, height)?;
        Ok(Self {
            device,
            queue,
            resources: HashMap::new(),
            charges: HashMap::new(),
            quotas: Arc::default(),
            surface,
            readback,
            width,
            height,
            padded_row_bytes,
            generation: 1,
        })
    }

    /// Returns the encoded limits and current surface generation.
    pub fn capabilities(&self) -> Vec<u8> {
        encode_capabilities(self.width, self.height, self.generation)
    }

    /// Replaces the output surface and advances its generation.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            bail!("invalid native GPU surface dimensions");
        }
        if self.width == width && self.height == height {
            return Ok(());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("GPU surface generation overflow"))?;
        let (surface, readback, padded) = create_surface(&self.device, width, height)?;
        self.surface = surface;
        self.readback = readback;
        self.width = width;
        self.height = height;
        self.padded_row_bytes = padded;
        self.generation = generation;
        Ok(())
    }

    /// Executes a bounded batch, reporting guest mistakes as rejection events.
    ///
    /// Successful commands preceding a rejection are not rolled back. Their
    /// resources remain charged, including references held by dependent objects.
    pub fn execute(&mut self, batch_bytes: &[u8]) -> NativeGpuOutput {
        let sequence = gpu_wire::decode_gpu_batch(batch_bytes)
            .map(|batch| batch.sequence())
            .unwrap_or(0);
        push_error_scopes(&self.device);
        let result = self.execute_inner(batch_bytes);
        // Flush queued uploads even when validation aborted before submission.
        self.queue.submit([]);
        self.device.poll(wgpu::Maintain::Wait);
        let backend = pop_error_scopes(&self.device);
        match result.and_then(|frame| backend.map(|()| frame)) {
            Ok(frame) => NativeGpuOutput {
                events: vec![submission_complete(sequence)],
                frame,
            },
            Err(error) => NativeGpuOutput {
                events: vec![batch_rejected(sequence, &format!("{error:#}"))],
                frame: None,
            },
        }
    }

    fn execute_inner(&mut self, batch_bytes: &[u8]) -> Result<Option<NativeGpuFrame>> {
        let batch = gpu_wire::decode_gpu_batch(batch_bytes).context("decode native GPU batch")?;
        let mut encoder: Option<wgpu::CommandEncoder> = None;
        let mut pending_pass: Option<PendingPass> = None;
        let mut pending_compute_pass: Option<PendingComputePass> = None;
        let mut presented = false;
        let mut compute_dispatches = 0usize;
        let mut budget = BatchBudget::default();
        // Keep destroyed resources charged until all submitted work has completed.
        let mut retained = Vec::new();
        for command in batch.commands() {
            let creation = self.reserve_creation(command.opcode, command.payload, &mut budget)?;
            let mut reader = Reader::new(command.payload);
            push_error_scopes(&self.device);
            let result = (|| -> Result<()> {
                match command.opcode {
                    GpuOpcode::CreateBuffer => {
                        let id = reader.u32()?;
                        self.require_new(id)?;
                        let usage = wgpu::BufferUsages::from_bits(reader.u32()?)
                            .ok_or_else(|| anyhow!("invalid buffer usage"))?;
                        let size = reader.u64()?;
                        reader.finish()?;
                        validate_buffer_descriptor(size, usage)?;
                        let value = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: None,
                            size,
                            usage,
                            mapped_at_creation: false,
                        });
                        self.resources.insert(id, Resource::Buffer { value, size });
                    }
                    GpuOpcode::WriteBuffer => {
                        let id = reader.u32()?;
                        reader.zero(4)?;
                        let offset = reader.u64()?;
                        let length = reader.u32()? as usize;
                        reader.zero(4)?;
                        let data = reader.take(length)?;
                        reader.zero_remaining()?;
                        let (buffer, size) = self.buffer(id)?;
                        if offset
                            .checked_add(length as u64)
                            .is_none_or(|end| end > size)
                        {
                            bail!("buffer write exceeds resource");
                        }
                        add_budget(
                            &mut budget.upload_bytes,
                            length,
                            gpu_wire::MAX_GPU_UPLOAD_BYTES_PER_TICK,
                        )?;
                        self.queue.write_buffer(buffer, offset, data);
                    }
                    GpuOpcode::CreateTexture => {
                        let id = reader.u32()?;
                        self.require_new(id)?;
                        let width = reader.u32()?;
                        let height = reader.u32()?;
                        let mip_level_count = reader.u16()? as u32;
                        let sample_count = reader.u16()? as u32;
                        let format = texture_format(reader.u16()?)?;
                        if reader.u8()? != 1 {
                            bail!("unsupported texture dimension");
                        }
                        reader.zero(1)?;
                        let usage = wgpu::TextureUsages::from_bits(reader.u32()?)
                            .ok_or_else(|| anyhow!("invalid texture usage"))?;
                        texture_bytes(width, height, mip_level_count, sample_count)?;
                        if usage.is_empty() {
                            bail!("empty texture usage");
                        }
                        reader.finish()?;
                        let value = self.device.create_texture(&wgpu::TextureDescriptor {
                            label: None,
                            size: wgpu::Extent3d {
                                width,
                                height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count,
                            sample_count,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage,
                            view_formats: &[],
                        });
                        self.resources.insert(id, Resource::Texture(value));
                    }
                    GpuOpcode::WriteTexture => {
                        let id = reader.u32()?;
                        let mip_level = reader.u32()?;
                        let origin = wgpu::Origin3d {
                            x: reader.u32()?,
                            y: reader.u32()?,
                            z: reader.u32()?,
                        };
                        let size = wgpu::Extent3d {
                            width: reader.u32()?,
                            height: reader.u32()?,
                            depth_or_array_layers: reader.u32()?,
                        };
                        let bytes_per_row = reader.u32()?;
                        let rows_per_image = reader.u32()?;
                        let length = reader.u32()? as usize;
                        let data = reader.take(length)?;
                        reader.zero_remaining()?;
                        let texture = self.texture(id)?;
                        let row_bytes = validate_texture_upload(
                            texture,
                            mip_level,
                            origin,
                            size,
                            rows_per_image,
                        )?;
                        // Account for backend row padding, not only guest data:
                        // repeated narrow uploads must not accumulate large staging buffers.
                        let staged = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                        add_budget(
                            &mut budget.upload_bytes,
                            staged as usize * size.height as usize,
                            gpu_wire::MAX_GPU_UPLOAD_BYTES_PER_TICK,
                        )?;
                        let (data, bytes_per_row) =
                            prepare_texture_upload(data, row_bytes, size.height, bytes_per_row)?;
                        self.queue.write_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture,
                                mip_level,
                                origin,
                                aspect: wgpu::TextureAspect::All,
                            },
                            &data,
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(bytes_per_row),
                                rows_per_image: Some(rows_per_image),
                            },
                            size,
                        );
                    }
                    GpuOpcode::CreateSampler => {
                        let id = reader.u32()?;
                        self.require_new(id)?;
                        let address_mode_u = address_mode(reader.u8()?)?;
                        let address_mode_v = address_mode(reader.u8()?)?;
                        let address_mode_w = address_mode(reader.u8()?)?;
                        let mag_filter = filter_mode(reader.u8()?)?;
                        let min_filter = filter_mode(reader.u8()?)?;
                        let mipmap_filter = filter_mode(reader.u8()?)?;
                        let compare_id = reader.u8()?;
                        let max_anisotropy = reader.u8()? as u16;
                        let lod_min_clamp = reader.f32()?;
                        let lod_max_clamp = reader.f32()?;
                        reader.zero(4)?;
                        reader.finish()?;
                        let value = self.device.create_sampler(&wgpu::SamplerDescriptor {
                            label: None,
                            address_mode_u,
                            address_mode_v,
                            address_mode_w,
                            mag_filter,
                            min_filter,
                            mipmap_filter,
                            lod_min_clamp,
                            lod_max_clamp,
                            compare: if compare_id == 0 {
                                None
                            } else {
                                Some(compare(compare_id)?)
                            },
                            anisotropy_clamp: max_anisotropy,
                            border_color: None,
                        });
                        self.resources.insert(id, Resource::Sampler(value));
                    }
                    GpuOpcode::CreateShaderWgsl => {
                        let id = reader.u32()?;
                        self.require_new(id)?;
                        let length = reader.u32()? as usize;
                        if length == 0 || length > gpu_wire::MAX_GPU_WGSL_BYTES {
                            bail!("WGSL source exceeds negotiated limits");
                        }
                        let source = std::str::from_utf8(reader.take(length)?)
                            .context("WGSL is not UTF-8")?;
                        reader.zero_remaining()?;
                        let value =
                            self.device
                                .create_shader_module(wgpu::ShaderModuleDescriptor {
                                    label: None,
                                    source: wgpu::ShaderSource::Wgsl(source.into()),
                                });
                        self.resources.insert(id, Resource::Shader(value));
                    }
                    GpuOpcode::CreateBindGroupLayout => {
                        self.create_bind_group_layout(&mut reader)?
                    }
                    GpuOpcode::CreatePipelineLayout => self.create_pipeline_layout(&mut reader)?,
                    GpuOpcode::CreateBindGroup => self.create_bind_group(&mut reader)?,
                    GpuOpcode::CreateRenderPipeline => self.create_render_pipeline(&mut reader)?,
                    GpuOpcode::DestroyResource => {
                        let id = reader.u32()?;
                        reader.finish()?;
                        let resource = self
                            .resources
                            .remove(&id)
                            .ok_or_else(|| anyhow!("unknown resource {id}"))?;
                        if let Some(charge) = self.charges.remove(&id) {
                            retained.push(charge);
                        }
                        match resource {
                            Resource::Buffer { value, .. } => value.destroy(),
                            Resource::Texture(value) => value.destroy(),
                            _ => {}
                        }
                    }
                    GpuOpcode::BeginRenderPass => {
                        add_budget(
                            &mut budget.renders,
                            1,
                            gpu_wire::MAX_GPU_RENDER_PASSES_PER_BATCH,
                        )?;
                        if pending_pass.is_some() || pending_compute_pass.is_some() {
                            bail!("nested render pass");
                        }
                        let color_view = reader.u32()?;
                        let depth_view = reader.u32()?;
                        let generation = reader.u32()?;
                        let flags = reader.u32()?;
                        let clear_color = wgpu::Color {
                            r: reader.f32()? as f64,
                            g: reader.f32()? as f64,
                            b: reader.f32()? as f64,
                            a: reader.f32()? as f64,
                        };
                        let clear_depth = reader.f32()?;
                        reader.finish()?;
                        if color_view != 0 || generation != self.generation {
                            bail!("stale or non-surface render attachment");
                        }
                        pending_pass = Some(PendingPass {
                            depth_view,
                            flags,
                            clear_color,
                            clear_depth,
                            operations: Vec::new(),
                        });
                    }
                    GpuOpcode::SetPipeline => pending(&mut pending_pass)?
                        .operations
                        .push(RenderOperation::Pipeline(reader.one_u32()?)),
                    GpuOpcode::SetVertexBuffer => {
                        let op = RenderOperation::VertexBuffer {
                            slot: reader.u32()?,
                            buffer: reader.u32()?,
                            offset: reader.u64()?,
                            size: reader.u64()?,
                        };
                        reader.finish()?;
                        pending(&mut pending_pass)?.operations.push(op);
                    }
                    GpuOpcode::SetIndexBuffer => {
                        let buffer = reader.u32()?;
                        let format = index_format(
                            u8::try_from(reader.u32()?).context("invalid index format")?,
                        )?;
                        let offset = reader.u64()?;
                        let size = reader.u64()?;
                        reader.finish()?;
                        pending(&mut pending_pass)?
                            .operations
                            .push(RenderOperation::IndexBuffer {
                                buffer,
                                format,
                                offset,
                                size,
                            });
                    }
                    GpuOpcode::SetBindGroup => {
                        let slot = reader.u32()?;
                        let bind_group = reader.u32()?;
                        let count = reader.u32()? as usize;
                        reader.count(count, 4, gpu_wire::MAX_GPU_BINDINGS_PER_GROUP)?;
                        let offsets = (0..count)
                            .map(|_| reader.u32())
                            .collect::<Result<Vec<_>>>()?;
                        reader.finish()?;
                        pending(&mut pending_pass)?
                            .operations
                            .push(RenderOperation::BindGroup {
                                slot,
                                bind_group,
                                offsets,
                            });
                    }
                    GpuOpcode::SetViewport => {
                        let values = reader.f32_array::<6>()?;
                        reader.finish()?;
                        pending(&mut pending_pass)?
                            .operations
                            .push(RenderOperation::Viewport(values));
                    }
                    GpuOpcode::SetScissorRect => {
                        let values = reader.u32_array::<4>()?;
                        reader.finish()?;
                        pending(&mut pending_pass)?
                            .operations
                            .push(RenderOperation::Scissor(values));
                    }
                    GpuOpcode::Draw => {
                        add_budget(&mut budget.draws, 1, gpu_wire::MAX_GPU_DRAWS_PER_BATCH)?;
                        let values = reader.u32_array::<4>()?;
                        reader.finish()?;
                        pending(&mut pending_pass)?
                            .operations
                            .push(RenderOperation::Draw(values));
                    }
                    GpuOpcode::DrawIndexed => {
                        add_budget(&mut budget.draws, 1, gpu_wire::MAX_GPU_DRAWS_PER_BATCH)?;
                        let op = RenderOperation::DrawIndexed {
                            indices: reader.u32()?,
                            instances: reader.u32()?,
                            first_index: reader.u32()?,
                            base_vertex: reader.i32()?,
                            first_instance: reader.u32()?,
                        };
                        reader.finish()?;
                        pending(&mut pending_pass)?.operations.push(op);
                    }
                    GpuOpcode::EndRenderPass => {
                        reader.finish()?;
                        let pass = pending_pass
                            .take()
                            .ok_or_else(|| anyhow!("render pass is not active"))?;
                        let command_encoder = encoder.get_or_insert_with(|| {
                            self.device.create_command_encoder(&Default::default())
                        });
                        self.encode_render_pass(command_encoder, pass)?;
                        presented = true;
                    }
                    GpuOpcode::CopyBufferToBuffer => {
                        let source = reader.u32()?;
                        let destination = reader.u32()?;
                        let source_offset = reader.u64()?;
                        let destination_offset = reader.u64()?;
                        let size = reader.u64()?;
                        reader.finish()?;
                        if pending_pass.is_some() || pending_compute_pass.is_some() {
                            bail!("buffer copy inside GPU pass");
                        }
                        checked_buffer_end(source_offset, size, self.buffer(source)?.1)?;
                        checked_buffer_end(destination_offset, size, self.buffer(destination)?.1)?;
                        let command_encoder = encoder.get_or_insert_with(|| {
                            self.device.create_command_encoder(&Default::default())
                        });
                        command_encoder.copy_buffer_to_buffer(
                            self.buffer(source)?.0,
                            source_offset,
                            self.buffer(destination)?.0,
                            destination_offset,
                            size,
                        );
                    }
                    GpuOpcode::CreateTextureView => self.create_texture_view(&mut reader)?,
                    GpuOpcode::CreateComputePipeline => {
                        let id = reader.u32()?;
                        self.require_new(id)?;
                        let layout_id = reader.u32()?;
                        let shader_id = reader.u32()?;
                        reader.zero(4)?;
                        reader.finish()?;
                        let value =
                            self.device
                                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                                    label: None,
                                    layout: Some(self.pipeline_layout(layout_id)?),
                                    module: self.shader(shader_id)?,
                                    entry_point: Some("cs_main"),
                                    compilation_options: Default::default(),
                                    cache: None,
                                });
                        self.resources.insert(id, Resource::ComputePipeline(value));
                    }
                    GpuOpcode::BeginComputePass => {
                        add_budget(
                            &mut budget.computes,
                            1,
                            gpu_wire::MAX_GPU_COMPUTE_PASSES_PER_BATCH,
                        )?;
                        reader.finish()?;
                        if pending_pass.is_some() || pending_compute_pass.is_some() {
                            bail!("nested GPU pass");
                        }
                        pending_compute_pass = Some(PendingComputePass {
                            operations: Vec::new(),
                        });
                    }
                    GpuOpcode::SetComputePipeline => pending_compute(&mut pending_compute_pass)?
                        .operations
                        .push(ComputeOperation::Pipeline(reader.one_u32()?)),
                    GpuOpcode::SetComputeBindGroup => {
                        let slot = reader.u32()?;
                        let bind_group = reader.u32()?;
                        let count = reader.u32()? as usize;
                        reader.count(count, 4, gpu_wire::MAX_GPU_BINDINGS_PER_GROUP)?;
                        let offsets = (0..count)
                            .map(|_| reader.u32())
                            .collect::<Result<Vec<_>>>()?;
                        reader.finish()?;
                        pending_compute(&mut pending_compute_pass)?.operations.push(
                            ComputeOperation::BindGroup {
                                slot,
                                bind_group,
                                offsets,
                            },
                        );
                    }
                    GpuOpcode::DispatchWorkgroups => {
                        let values = reader.u32_array::<3>()?;
                        reader.finish()?;
                        validate_compute_dispatch(values, &mut compute_dispatches)?;
                        pending_compute(&mut pending_compute_pass)?
                            .operations
                            .push(ComputeOperation::Dispatch(values));
                    }
                    GpuOpcode::EndComputePass => {
                        reader.finish()?;
                        let pass = pending_compute_pass
                            .take()
                            .ok_or_else(|| anyhow!("compute pass is not active"))?;
                        let command_encoder = encoder.get_or_insert_with(|| {
                            self.device.create_command_encoder(&Default::default())
                        });
                        self.encode_compute_pass(command_encoder, pass)?;
                    }
                }
                Ok(())
            })();
            let backend = pop_error_scopes(&self.device);
            if let Err(error) = result.and(backend) {
                if let Some((id, _)) = creation {
                    self.resources.remove(&id);
                }
                return Err(error);
            }
            if let Some((id, charge)) = creation {
                self.charges.insert(id, charge);
            }
        }
        if pending_pass.is_some() {
            bail!("render pass was not ended");
        }
        if pending_compute_pass.is_some() {
            bail!("compute pass was not ended");
        }
        let Some(mut encoder) = encoder else {
            return Ok(None);
        };
        if presented {
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.surface,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &self.readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.padded_row_bytes),
                        rows_per_image: Some(self.height),
                    },
                },
                wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.submit([encoder.finish()]);
        // Bound in-flight allocations even for batches that do not read a frame.
        self.device.poll(wgpu::Maintain::Wait);
        if presented {
            self.read_frame().map(Some)
        } else {
            Ok(None)
        }
    }

    fn reserve_creation(
        &self,
        opcode: GpuOpcode,
        payload: &[u8],
        batch: &mut BatchBudget,
    ) -> Result<Option<(u32, Arc<Charge>)>> {
        let kind = match opcode {
            GpuOpcode::CreateBuffer => 0,
            GpuOpcode::CreateTexture => 1,
            GpuOpcode::CreateTextureView => 2,
            GpuOpcode::CreateSampler => 3,
            GpuOpcode::CreateShaderWgsl => 4,
            GpuOpcode::CreateBindGroupLayout => 5,
            GpuOpcode::CreatePipelineLayout => 6,
            GpuOpcode::CreateBindGroup => 7,
            GpuOpcode::CreateRenderPipeline => 8,
            GpuOpcode::CreateComputePipeline => 9,
            _ => return Ok(None),
        };
        let mut reader = Reader::new(payload);
        let id = reader.u32()?;
        self.require_new(id)?;
        let mut dependencies = Vec::new();
        let mut depend = |id| -> Result<()> {
            dependencies.push(Arc::clone(
                self.charges
                    .get(&id)
                    .ok_or_else(|| anyhow!("unknown dependency {id}"))?,
            ));
            Ok(())
        };
        let bytes = match kind {
            0 => {
                let usage = wgpu::BufferUsages::from_bits(reader.u32()?)
                    .ok_or_else(|| anyhow!("invalid buffer usage"))?;
                let size = reader.u64()?;
                validate_buffer_descriptor(size, usage)?;
                size as usize
            }
            1 => texture_bytes(
                reader.u32()?,
                reader.u32()?,
                reader.u16()? as u32,
                reader.u16()? as u32,
            )?,
            2 => {
                depend(reader.u32()?)?;
                0
            }
            6 => {
                let count = reader.u32()? as usize;
                reader.count(count, 4, gpu_wire::MAX_GPU_BIND_GROUPS_PER_PIPELINE)?;
                for _ in 0..count {
                    depend(reader.u32()?)?;
                }
                0
            }
            7 => {
                depend(reader.u32()?)?;
                let count = reader.u32()? as usize;
                reader.count(count, 32, gpu_wire::MAX_GPU_BINDINGS_PER_GROUP)?;
                for _ in 0..count {
                    reader.u32()?;
                    depend(reader.u32()?)?;
                    reader.take(24)?;
                }
                0
            }
            8 | 9 => {
                depend(reader.u32()?)?;
                depend(reader.u32()?)?;
                0
            }
            _ => 0,
        };
        Ok(Some((
            id,
            self.quotas.reserve(kind, bytes, dependencies, batch)?,
        )))
    }

    fn require_new(&self, id: u32) -> Result<()> {
        if id == 0 || self.resources.contains_key(&id) {
            bail!("invalid new resource {id}");
        }
        Ok(())
    }

    fn buffer(&self, id: u32) -> Result<(&wgpu::Buffer, u64)> {
        match self.resources.get(&id) {
            Some(Resource::Buffer { value, size }) => Ok((value, *size)),
            _ => bail!("invalid buffer {id}"),
        }
    }
    fn texture(&self, id: u32) -> Result<&wgpu::Texture> {
        match self.resources.get(&id) {
            Some(Resource::Texture(value)) => Ok(value),
            _ => bail!("invalid texture {id}"),
        }
    }
    fn texture_view(&self, id: u32) -> Result<&wgpu::TextureView> {
        match self.resources.get(&id) {
            Some(Resource::TextureView(value)) => Ok(value),
            _ => bail!("invalid texture view {id}"),
        }
    }
    fn sampler(&self, id: u32) -> Result<&wgpu::Sampler> {
        match self.resources.get(&id) {
            Some(Resource::Sampler(value)) => Ok(value),
            _ => bail!("invalid sampler {id}"),
        }
    }
    fn shader(&self, id: u32) -> Result<&wgpu::ShaderModule> {
        match self.resources.get(&id) {
            Some(Resource::Shader(value)) => Ok(value),
            _ => bail!("invalid shader {id}"),
        }
    }
    fn bind_group_layout(&self, id: u32) -> Result<&wgpu::BindGroupLayout> {
        match self.resources.get(&id) {
            Some(Resource::BindGroupLayout(value)) => Ok(value),
            _ => bail!("invalid bind group layout {id}"),
        }
    }
    fn pipeline_layout(&self, id: u32) -> Result<&wgpu::PipelineLayout> {
        match self.resources.get(&id) {
            Some(Resource::PipelineLayout(value)) => Ok(value),
            _ => bail!("invalid pipeline layout {id}"),
        }
    }
    fn bind_group(&self, id: u32) -> Result<&wgpu::BindGroup> {
        match self.resources.get(&id) {
            Some(Resource::BindGroup(value)) => Ok(value),
            _ => bail!("invalid bind group {id}"),
        }
    }
    fn render_pipeline(&self, id: u32) -> Result<&wgpu::RenderPipeline> {
        match self.resources.get(&id) {
            Some(Resource::RenderPipeline(value)) => Ok(value),
            _ => bail!("invalid render pipeline {id}"),
        }
    }

    fn compute_pipeline(&self, id: u32) -> Result<&wgpu::ComputePipeline> {
        match self.resources.get(&id) {
            Some(Resource::ComputePipeline(value)) => Ok(value),
            _ => bail!("invalid compute pipeline {id}"),
        }
    }

    fn create_bind_group_layout(&mut self, reader: &mut Reader<'_>) -> Result<()> {
        let id = reader.u32()?;
        self.require_new(id)?;
        let count = reader.u32()? as usize;
        reader.count(count, 32, gpu_wire::MAX_GPU_BINDINGS_PER_GROUP)?;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let binding = reader.u32()?;
            let visibility = wgpu::ShaderStages::from_bits(reader.u32()?)
                .ok_or_else(|| anyhow!("invalid shader visibility"))?;
            let kind = reader.u16()?;
            let flags = reader.u16()?;
            reader.zero(4)?;
            let min = reader.u64()?;
            let p0 = reader.u32()?;
            let p1 = reader.u32()?;
            validate_binding_visibility(kind, visibility)?;
            let ty = match kind {
                1 | 4 | 5 => wgpu::BindingType::Buffer {
                    ty: buffer_binding_type(kind, flags, p0, p1)?,
                    has_dynamic_offset: flags & 1 != 0,
                    min_binding_size: NonZeroU64::new(min),
                },
                2 => wgpu::BindingType::Sampler(match p0 {
                    1 => wgpu::SamplerBindingType::Filtering,
                    2 => wgpu::SamplerBindingType::NonFiltering,
                    3 => wgpu::SamplerBindingType::Comparison,
                    _ => bail!("invalid sampler binding type"),
                }),
                3 => wgpu::BindingType::Texture {
                    sample_type: match p0 {
                        1 => wgpu::TextureSampleType::Float { filterable: true },
                        2 => wgpu::TextureSampleType::Float { filterable: false },
                        3 => wgpu::TextureSampleType::Depth,
                        4 => wgpu::TextureSampleType::Sint,
                        5 => wgpu::TextureSampleType::Uint,
                        _ => bail!("invalid texture sample type"),
                    },
                    view_dimension: if p1 == 1 {
                        wgpu::TextureViewDimension::D2
                    } else {
                        bail!("invalid texture view dimension")
                    },
                    multisampled: false,
                },
                _ => bail!("invalid bind group layout kind"),
            };
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility,
                ty,
                count: None,
            });
        }
        reader.finish()?;
        let value = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries,
            });
        self.resources.insert(id, Resource::BindGroupLayout(value));
        Ok(())
    }

    fn create_pipeline_layout(&mut self, reader: &mut Reader<'_>) -> Result<()> {
        let id = reader.u32()?;
        self.require_new(id)?;
        let count = reader.u32()? as usize;
        reader.count(count, 4, gpu_wire::MAX_GPU_BIND_GROUPS_PER_PIPELINE)?;
        let ids = (0..count)
            .map(|_| reader.u32())
            .collect::<Result<Vec<_>>>()?;
        reader.finish()?;
        let layouts = ids
            .iter()
            .map(|id| self.bind_group_layout(*id))
            .collect::<Result<Vec<_>>>()?;
        let value = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &layouts,
                push_constant_ranges: &[],
            });
        self.resources.insert(id, Resource::PipelineLayout(value));
        Ok(())
    }

    fn create_bind_group(&mut self, reader: &mut Reader<'_>) -> Result<()> {
        let id = reader.u32()?;
        self.require_new(id)?;
        let layout_id = reader.u32()?;
        let count = reader.u32()? as usize;
        reader.count(count, 32, gpu_wire::MAX_GPU_BINDINGS_PER_GROUP)?;
        enum EntrySpec {
            Buffer {
                binding: u32,
                id: u32,
                offset: u64,
                size: u64,
            },
            Sampler {
                binding: u32,
                id: u32,
            },
            Texture {
                binding: u32,
                id: u32,
            },
        }
        let mut specs = Vec::with_capacity(count);
        for _ in 0..count {
            let binding = reader.u32()?;
            let resource = reader.u32()?;
            let kind = reader.u16()?;
            reader.zero(2)?;
            reader.zero(4)?;
            let offset = reader.u64()?;
            let size = reader.u64()?;
            specs.push(match kind {
                1 | 4 | 5 => EntrySpec::Buffer {
                    binding,
                    id: resource,
                    offset,
                    size,
                },
                2 => EntrySpec::Sampler {
                    binding,
                    id: resource,
                },
                3 => EntrySpec::Texture {
                    binding,
                    id: resource,
                },
                _ => bail!("invalid bind group resource kind"),
            });
        }
        reader.finish()?;
        let mut entries = Vec::with_capacity(count);
        for spec in &specs {
            entries.push(match *spec {
                EntrySpec::Buffer {
                    binding,
                    id,
                    offset,
                    size,
                } => {
                    let (buffer, total) = self.buffer(id)?;
                    let size = binding_size(offset, size, total)?;
                    wgpu::BindGroupEntry {
                        binding,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer,
                            offset,
                            size: NonZeroU64::new(size),
                        }),
                    }
                }
                EntrySpec::Sampler { binding, id } => wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::Sampler(self.sampler(id)?),
                },
                EntrySpec::Texture { binding, id } => wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(self.texture_view(id)?),
                },
            });
        }
        let value = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: self.bind_group_layout(layout_id)?,
            entries: &entries,
        });
        self.resources.insert(id, Resource::BindGroup(value));
        Ok(())
    }

    fn create_render_pipeline(&mut self, reader: &mut Reader<'_>) -> Result<()> {
        let id = reader.u32()?;
        self.require_new(id)?;
        let layout_id = reader.u32()?;
        let shader_id = reader.u32()?;
        let layout_count = reader.u16()? as usize;
        let attribute_count = reader.u16()? as usize;
        let target_count = reader.u16()? as usize;
        let flags = reader.u16()?;
        let depth_format_id = reader.u16()?;
        let sample_count = reader.u16()? as u32;
        let topology = topology(reader.u8()?)?;
        let front_face = front_face(reader.u8()?)?;
        let cull_mode = cull_mode(reader.u8()?)?;
        let strip_id = reader.u8()?;
        let depth_compare_id = reader.u8()?;
        reader.zero(11)?;
        reader.count(layout_count, 16, gpu_wire::MAX_GPU_VERTEX_BUFFERS)?;
        reader.count(attribute_count, 16, gpu_wire::MAX_GPU_VERTEX_ATTRIBUTES)?;
        reader.count(target_count, 16, gpu_wire::MAX_GPU_COLOR_ATTACHMENTS)?;
        let mut layout_specs = Vec::with_capacity(layout_count);
        for _ in 0..layout_count {
            let stride = reader.u64()?;
            let step = vertex_step(reader.u8()?)?;
            reader.zero(3)?;
            let first = reader.u16()? as usize;
            let count = reader.u16()? as usize;
            layout_specs.push((stride, step, first, count));
        }
        let mut attributes = Vec::with_capacity(attribute_count);
        let max_attribute_stride = self.device.limits().max_vertex_buffer_array_stride as u64;
        for _ in 0..attribute_count {
            let format = vertex_format(reader.u16()?)?;
            let shader_location = reader.u16()? as u32;
            let offset = reader.u64()?;
            if offset
                .checked_add(format.size())
                .is_none_or(|end| end > max_attribute_stride)
            {
                bail!("vertex attribute offset exceeds limits");
            }
            attributes.push(wgpu::VertexAttribute {
                format,
                shader_location,
                offset,
            });
            reader.zero(4)?;
        }
        let mut targets = Vec::with_capacity(target_count);
        for _ in 0..target_count {
            let format = texture_format(reader.u16()?)?;
            let write_mask = wgpu::ColorWrites::from_bits(reader.u16()? as u32)
                .ok_or_else(|| anyhow!("invalid color write mask"))?;
            let color = wgpu::BlendComponent {
                operation: blend_operation(reader.u8()?)?,
                src_factor: blend_factor(reader.u8()?)?,
                dst_factor: blend_factor(reader.u8()?)?,
            };
            let alpha = wgpu::BlendComponent {
                operation: blend_operation(reader.u8()?)?,
                src_factor: blend_factor(reader.u8()?)?,
                dst_factor: blend_factor(reader.u8()?)?,
            };
            reader.zero(6)?;
            targets.push(Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState { color, alpha }),
                write_mask,
            }));
        }
        reader.finish()?;
        let buffers = layout_specs
            .iter()
            .map(|(stride, step, first, count)| {
                let end = first
                    .checked_add(*count)
                    .ok_or_else(|| anyhow!("vertex attribute range overflow"))?;
                let slice = attributes
                    .get(*first..end)
                    .ok_or_else(|| anyhow!("vertex attribute range is invalid"))?;
                Ok(wgpu::VertexBufferLayout {
                    array_stride: *stride,
                    step_mode: *step,
                    attributes: slice,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let depth_stencil = if depth_format_id == 0 {
            None
        } else {
            Some(wgpu::DepthStencilState {
                format: texture_format(depth_format_id)?,
                depth_write_enabled: flags & 1 != 0,
                depth_compare: compare(depth_compare_id)?,
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let value = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(self.pipeline_layout(layout_id)?),
                vertex: wgpu::VertexState {
                    module: self.shader(shader_id)?,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: self.shader(shader_id)?,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: if strip_id == 0 {
                        None
                    } else {
                        Some(index_format(strip_id)?)
                    },
                    front_face,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                multiview: None,
                cache: None,
            });
        self.resources.insert(id, Resource::RenderPipeline(value));
        Ok(())
    }

    fn create_texture_view(&mut self, reader: &mut Reader<'_>) -> Result<()> {
        let id = reader.u32()?;
        self.require_new(id)?;
        let texture_id = reader.u32()?;
        let format = texture_format(reader.u16()?)?;
        if reader.u8()? != 1 {
            bail!("unsupported texture view dimension");
        }
        let aspect = texture_aspect(reader.u8()?)?;
        let base_mip_level = reader.u16()? as u32;
        let mip_level_count = reader.u16()? as u32;
        let base_array_layer = reader.u16()? as u32;
        let array_layer_count = reader.u16()? as u32;
        reader.finish()?;
        let value = self
            .texture(texture_id)?
            .create_view(&wgpu::TextureViewDescriptor {
                label: None,
                format: Some(format),
                dimension: Some(wgpu::TextureViewDimension::D2),
                usage: None,
                aspect,
                base_mip_level,
                mip_level_count: Some(mip_level_count),
                base_array_layer,
                array_layer_count: Some(array_layer_count),
            });
        self.resources.insert(id, Resource::TextureView(value));
        Ok(())
    }

    fn encode_render_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pending: PendingPass,
    ) -> Result<()> {
        let surface_view = self.surface.create_view(&Default::default());
        let depth_view = if pending.depth_view == 0 {
            None
        } else {
            Some(self.texture_view(pending.depth_view)?)
        };
        let color_attachment = Some(wgpu::RenderPassColorAttachment {
            view: &surface_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if pending.flags & 1 != 0 {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(pending.clear_color)
                },
                store: if pending.flags & 2 != 0 {
                    wgpu::StoreOp::Store
                } else {
                    wgpu::StoreOp::Discard
                },
            },
        });
        let depth_attachment = depth_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: if pending.flags & 4 != 0 {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(pending.clear_depth)
                },
                store: if pending.flags & 8 != 0 {
                    wgpu::StoreOp::Store
                } else {
                    wgpu::StoreOp::Discard
                },
            }),
            stencil_ops: None,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[color_attachment],
            depth_stencil_attachment: depth_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        for operation in pending.operations {
            match operation {
                RenderOperation::Pipeline(id) => pass.set_pipeline(self.render_pipeline(id)?),
                RenderOperation::VertexBuffer {
                    slot,
                    buffer,
                    offset,
                    size,
                } => {
                    let (value, total) = self.buffer(buffer)?;
                    pass.set_vertex_buffer(slot, value.slice(buffer_range(offset, size, total)?));
                }
                RenderOperation::IndexBuffer {
                    buffer,
                    format,
                    offset,
                    size,
                } => {
                    let (value, total) = self.buffer(buffer)?;
                    pass.set_index_buffer(value.slice(buffer_range(offset, size, total)?), format);
                }
                RenderOperation::BindGroup {
                    slot,
                    bind_group,
                    offsets,
                } => pass.set_bind_group(slot, self.bind_group(bind_group)?, &offsets),
                RenderOperation::Viewport(values) => pass.set_viewport(
                    values[0], values[1], values[2], values[3], values[4], values[5],
                ),
                RenderOperation::Scissor(values) => {
                    checked_buffer_end(values[0] as u64, values[2] as u64, self.width as u64)?;
                    checked_buffer_end(values[1] as u64, values[3] as u64, self.height as u64)?;
                    pass.set_scissor_rect(values[0], values[1], values[2], values[3])
                }
                RenderOperation::Draw(values) => pass.draw(
                    draw_range(values[2], values[0])?,
                    draw_range(values[3], values[1])?,
                ),
                RenderOperation::DrawIndexed {
                    indices,
                    instances,
                    first_index,
                    base_vertex,
                    first_instance,
                } => pass.draw_indexed(
                    draw_range(first_index, indices)?,
                    base_vertex,
                    draw_range(first_instance, instances)?,
                ),
            }
        }
        Ok(())
    }

    fn encode_compute_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pending: PendingComputePass,
    ) -> Result<()> {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        for operation in pending.operations {
            match operation {
                ComputeOperation::Pipeline(id) => pass.set_pipeline(self.compute_pipeline(id)?),
                ComputeOperation::BindGroup {
                    slot,
                    bind_group,
                    offsets,
                } => pass.set_bind_group(slot, self.bind_group(bind_group)?, &offsets),
                ComputeOperation::Dispatch(values) => {
                    pass.dispatch_workgroups(values[0], values[1], values[2]);
                }
            }
        }
        Ok(())
    }

    fn read_frame(&self) -> Result<NativeGpuFrame> {
        let slice = self.readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        receiver.recv().context("wait for native GPU readback")??;
        let mapped = slice.get_mapped_range();
        let row_bytes = self.width as usize * 4;
        let mut rgba = vec![0; row_bytes * self.height as usize];
        for row in 0..self.height as usize {
            rgba[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(
                &mapped[row * self.padded_row_bytes as usize
                    ..row * self.padded_row_bytes as usize + row_bytes],
            );
        }
        drop(mapped);
        self.readback.unmap();
        Ok(NativeGpuFrame {
            width: self.width,
            height: self.height,
            rgba,
        })
    }
}

fn push_error_scopes(device: &wgpu::Device) {
    device.push_error_scope(wgpu::ErrorFilter::Internal);
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    device.push_error_scope(wgpu::ErrorFilter::Validation);
}

fn pop_error_scopes(device: &wgpu::Device) -> Result<()> {
    // Always pop every scope, including when an earlier one contains an error.
    let validation = pollster::block_on(device.pop_error_scope());
    let memory = pollster::block_on(device.pop_error_scope());
    let internal = pollster::block_on(device.pop_error_scope());
    if let Some(error) = validation.or(memory).or(internal) {
        bail!("native GPU: {error}");
    }
    Ok(())
}

fn validate_buffer_descriptor(size: u64, usage: wgpu::BufferUsages) -> Result<()> {
    if size == 0 || size > gpu_wire::MAX_GPU_BUFFER_BYTES as u64 || usage.is_empty() {
        bail!("invalid buffer size or usage");
    }
    if (usage.contains(wgpu::BufferUsages::MAP_READ)
        && !(usage - (wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST)).is_empty())
        || (usage.contains(wgpu::BufferUsages::MAP_WRITE)
            && !(usage - (wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC)).is_empty())
    {
        bail!("invalid mapped buffer usage");
    }
    Ok(())
}

fn texture_bytes(mut width: u32, mut height: u32, mips: u32, samples: u32) -> Result<usize> {
    if width == 0
        || height == 0
        || width > gpu_wire::MAX_GPU_TEXTURE_DIMENSION_2D
        || height > gpu_wire::MAX_GPU_TEXTURE_DIMENSION_2D
        || samples != gpu_wire::MAX_GPU_TEXTURE_SAMPLE_COUNT
        || mips == 0
        || mips > gpu_wire::MAX_GPU_TEXTURE_MIP_LEVELS
        || mips > u32::BITS - width.max(height).leading_zeros()
    {
        bail!("invalid texture dimensions, samples or mip count");
    }
    let mut bytes = 0;
    for _ in 0..mips {
        // Conservatively charge four bytes even for R8, as the browser does.
        add_budget(
            &mut bytes,
            width as usize * height as usize * 4,
            gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES,
        )?;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Ok(bytes)
}

fn buffer_range(offset: u64, size: u64, total: u64) -> Result<std::ops::Range<u64>> {
    let end = offset
        .checked_add(size)
        .filter(|end| *end <= total && size != 0)
        .ok_or_else(|| anyhow!("GPU buffer slice is empty or out of bounds"))?;
    Ok(offset..end)
}

fn draw_range(first: u32, count: u32) -> Result<std::ops::Range<u32>> {
    Ok(first
        ..first
            .checked_add(count)
            .ok_or_else(|| anyhow!("GPU draw range overflow"))?)
}

fn checked_buffer_end(offset: u64, size: u64, total: u64) -> Result<u64> {
    offset
        .checked_add(size)
        .filter(|end| *end <= total)
        .ok_or_else(|| anyhow!("GPU resource range exceeds bounds"))
}

fn binding_size(offset: u64, size: u64, total: u64) -> Result<u64> {
    let size = if size == 0 {
        total
            .checked_sub(offset)
            .ok_or_else(|| anyhow!("GPU binding offset exceeds buffer"))?
    } else {
        size
    };
    buffer_range(offset, size, total)?;
    Ok(size)
}

fn validate_texture_upload(
    texture: &wgpu::Texture,
    mip: u32,
    origin: wgpu::Origin3d,
    size: wgpu::Extent3d,
    rows_per_image: u32,
) -> Result<u32> {
    if mip >= texture.mip_level_count()
        || size.width == 0
        || size.height == 0
        || size.depth_or_array_layers != 1
        || origin.z != 0
    {
        bail!("invalid texture upload extent");
    }
    checked_buffer_end(
        origin.x as u64,
        size.width as u64,
        (texture.width() >> mip).max(1) as u64,
    )?;
    checked_buffer_end(
        origin.y as u64,
        size.height as u64,
        (texture.height() >> mip).max(1) as u64,
    )?;
    // Negotiated depth formats cannot be copy destinations in WebGPU.
    if texture.format().is_depth_stencil_format() {
        bail!("depth texture uploads are unsupported");
    }
    let texel_bytes = texture
        .format()
        .block_copy_size(None)
        .ok_or_else(|| anyhow!("texture format cannot be copied"))?;
    if rows_per_image < size.height {
        bail!("invalid texture upload row layout");
    }
    Ok(size.width * texel_bytes)
}

fn prepare_texture_upload(
    data: &[u8],
    row_bytes: u32,
    height: u32,
    stride: u32,
) -> Result<(Cow<'_, [u8]>, u32)> {
    if height == 0 || row_bytes == 0 || stride < row_bytes {
        bail!("invalid texture upload row layout");
    }
    let required = (height as u64 - 1) * stride as u64 + row_bytes as u64;
    if required > data.len() as u64 {
        bail!("texture upload is truncated");
    }
    if height == 1 {
        // The sole row has no stride; avoid allocating guest-controlled padding.
        return Ok((Cow::Borrowed(data), row_bytes));
    }
    // wgpu-core 24's chunked staging copy reads min(staging pitch, source
    // stride) on every row, including the last. WebGPU only requires texel
    // bytes on that final row. Supply trailing padding without altering pixels.
    let padded = height as u64 * stride as u64;
    if padded > gpu_wire::MAX_GPU_UPLOAD_BYTES_PER_TICK as u64 {
        bail!("texture upload staging budget exceeded");
    }
    if padded <= data.len() as u64 {
        return Ok((Cow::Borrowed(data), stride));
    }
    let mut padded_data = vec![0; padded as usize];
    padded_data[..data.len()].copy_from_slice(data);
    Ok((Cow::Owned(padded_data), stride))
}

fn encode_capabilities(width: u32, height: u32, generation: u32) -> Vec<u8> {
    let limits = [
        (1u16, gpu_wire::MAX_GPU_TEXTURE_DIMENSION_2D as u64),
        (2, gpu_wire::MAX_GPU_BUFFER_BYTES as u64),
        (3, gpu_wire::MAX_GPU_BINDINGS_PER_GROUP as u64),
        (4, gpu_wire::MAX_GPU_BIND_GROUPS_PER_PIPELINE as u64),
        (5, gpu_wire::MAX_GPU_VERTEX_BUFFERS as u64),
        (6, gpu_wire::MAX_GPU_VERTEX_ATTRIBUTES as u64),
        (7, gpu_wire::MAX_GPU_COLOR_ATTACHMENTS as u64),
        (8, gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES as u64),
        (9, gpu_wire::MAX_GPU_TOTAL_BUFFER_BYTES as u64),
        (10, gpu_wire::MAX_GPU_DRAWS_PER_BATCH as u64),
        (11, gpu_wire::MAX_GPU_BATCH_BYTES as u64),
        (12, gpu_wire::MAX_GPU_UPLOAD_BYTES_PER_TICK as u64),
        (13, gpu_wire::MAX_GPU_BUFFER_BYTES as u64),
        (14, MAX_STORAGE_BUFFERS_PER_SHADER_STAGE as u64),
        (15, MAX_COMPUTE_WORKGROUP_STORAGE_SIZE as u64),
        (16, 256),
        (17, 256),
        (18, 256),
        (19, 64),
        (20, MAX_COMPUTE_WORKGROUPS_PER_DIMENSION as u64),
        (21, gpu_wire::MAX_GPU_DISPATCHES_PER_BATCH as u64),
    ];
    let mut bytes = vec![0; 56 + limits.len() * 16];
    bytes[..4].copy_from_slice(&gpu_wire::GPU_CAPABILITIES_MAGIC);
    bytes[4..6].copy_from_slice(&gpu_wire::GPU_WIRE_VERSION.to_le_bytes());
    let byte_len = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&byte_len.to_le_bytes());
    bytes[12..14].copy_from_slice(&FORMAT_RGBA8_UNORM.to_le_bytes());
    for (offset, value) in [
        (16, width),
        (20, height),
        (24, width),
        (28, height),
        (36, generation),
        (40, 1),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[32..36].copy_from_slice(&1.0f32.to_le_bytes());
    bytes[44..48].copy_from_slice(&(limits.len() as u32).to_le_bytes());
    for (index, (key, value)) in limits.into_iter().enumerate() {
        let offset = 56 + index * 16;
        bytes[offset..offset + 2].copy_from_slice(&key.to_le_bytes());
        bytes[offset + 4..offset + 12].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn native_required_limits() -> wgpu::Limits {
    wgpu::Limits {
        max_texture_dimension_2d: gpu_wire::MAX_GPU_TEXTURE_DIMENSION_2D,
        max_storage_buffers_per_shader_stage: MAX_STORAGE_BUFFERS_PER_SHADER_STAGE,
        max_compute_workgroup_storage_size: MAX_COMPUTE_WORKGROUP_STORAGE_SIZE,
        ..wgpu::Limits::downlevel_defaults()
    }
}

fn validate_compute_dispatch(values: [u32; 3], dispatches: &mut usize) -> Result<()> {
    if values
        .iter()
        .any(|value| *value == 0 || *value > MAX_COMPUTE_WORKGROUPS_PER_DIMENSION)
    {
        bail!("compute dispatch dimension outside negotiated limits");
    }
    *dispatches = dispatches
        .checked_add(1)
        .ok_or_else(|| anyhow!("compute dispatch count overflow"))?;
    if *dispatches > gpu_wire::MAX_GPU_DISPATCHES_PER_BATCH {
        bail!("too many compute dispatches");
    }
    Ok(())
}

fn validate_binding_visibility(kind: u16, visibility: wgpu::ShaderStages) -> Result<()> {
    if kind == 5 && visibility.contains(wgpu::ShaderStages::VERTEX) {
        bail!("writable storage binding visible to the vertex stage");
    }
    Ok(())
}

fn create_surface(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> Result<(wgpu::Texture, wgpu::Buffer, u32)> {
    push_error_scopes(device);
    let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let surface = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("PolkaVM output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("PolkaVM readback"),
        size: padded as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    pop_error_scopes(device)?;
    Ok((surface, readback, padded))
}

fn pending(pass: &mut Option<PendingPass>) -> Result<&mut PendingPass> {
    pass.as_mut()
        .ok_or_else(|| anyhow!("render pass is not active"))
}

fn pending_compute(pass: &mut Option<PendingComputePass>) -> Result<&mut PendingComputePass> {
    pass.as_mut()
        .ok_or_else(|| anyhow!("compute pass is not active"))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn count(&self, count: usize, stride: usize, limit: usize) -> Result<()> {
        if count > limit || count > (self.bytes.len() - self.offset) / stride {
            bail!("GPU descriptor array exceeds bounds");
        }
        Ok(())
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| anyhow!("GPU payload range overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| anyhow!("GPU payload is truncated"))?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        let value = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        if !value.is_finite() {
            bail!("GPU float is not finite");
        }
        Ok(value)
    }
    fn zero(&mut self, length: usize) -> Result<()> {
        if self.take(length)?.iter().any(|byte| *byte != 0) {
            bail!("GPU reserved bytes are nonzero");
        }
        Ok(())
    }
    fn zero_remaining(&mut self) -> Result<()> {
        self.zero(self.bytes.len() - self.offset)
    }
    fn finish(&self) -> Result<()> {
        if self.offset != self.bytes.len() {
            bail!("GPU payload has trailing bytes");
        }
        Ok(())
    }
    fn one_u32(&mut self) -> Result<u32> {
        let value = self.u32()?;
        self.finish()?;
        Ok(value)
    }
    fn u32_array<const N: usize>(&mut self) -> Result<[u32; N]> {
        let mut values = [0; N];
        for value in &mut values {
            *value = self.u32()?;
        }
        Ok(values)
    }
    fn f32_array<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32()?;
        }
        Ok(values)
    }
}

fn texture_format(id: u16) -> Result<wgpu::TextureFormat> {
    Ok(match id {
        1 => wgpu::TextureFormat::Rgba8Unorm,
        2 => wgpu::TextureFormat::Rgba8UnormSrgb,
        3 => wgpu::TextureFormat::Bgra8Unorm,
        4 => wgpu::TextureFormat::Bgra8UnormSrgb,
        5 => wgpu::TextureFormat::Depth24Plus,
        6 => wgpu::TextureFormat::Depth32Float,
        7 => wgpu::TextureFormat::R8Unorm,
        _ => bail!("invalid texture format"),
    })
}
fn buffer_binding_type(
    id: u16,
    flags: u16,
    parameter_0: u32,
    parameter_1: u32,
) -> Result<wgpu::BufferBindingType> {
    if flags & !1 != 0 || parameter_0 != 0 || parameter_1 != 0 {
        bail!("invalid buffer binding layout");
    }
    Ok(match id {
        1 => wgpu::BufferBindingType::Uniform,
        4 => wgpu::BufferBindingType::Storage { read_only: true },
        5 => wgpu::BufferBindingType::Storage { read_only: false },
        _ => bail!("invalid buffer binding type"),
    })
}
fn address_mode(id: u8) -> Result<wgpu::AddressMode> {
    Ok(match id {
        1 => wgpu::AddressMode::ClampToEdge,
        2 => wgpu::AddressMode::Repeat,
        3 => wgpu::AddressMode::MirrorRepeat,
        _ => bail!("invalid address mode"),
    })
}
fn filter_mode(id: u8) -> Result<wgpu::FilterMode> {
    Ok(match id {
        1 => wgpu::FilterMode::Nearest,
        2 => wgpu::FilterMode::Linear,
        _ => bail!("invalid filter mode"),
    })
}
fn compare(id: u8) -> Result<wgpu::CompareFunction> {
    Ok(match id {
        1 => wgpu::CompareFunction::Never,
        2 => wgpu::CompareFunction::Less,
        3 => wgpu::CompareFunction::Equal,
        4 => wgpu::CompareFunction::LessEqual,
        5 => wgpu::CompareFunction::Greater,
        6 => wgpu::CompareFunction::NotEqual,
        7 => wgpu::CompareFunction::GreaterEqual,
        8 => wgpu::CompareFunction::Always,
        _ => bail!("invalid compare function"),
    })
}
fn index_format(id: u8) -> Result<wgpu::IndexFormat> {
    Ok(match id {
        1 => wgpu::IndexFormat::Uint16,
        2 => wgpu::IndexFormat::Uint32,
        _ => bail!("invalid index format"),
    })
}
fn topology(id: u8) -> Result<wgpu::PrimitiveTopology> {
    Ok(match id {
        1 => wgpu::PrimitiveTopology::PointList,
        2 => wgpu::PrimitiveTopology::LineList,
        3 => wgpu::PrimitiveTopology::LineStrip,
        4 => wgpu::PrimitiveTopology::TriangleList,
        5 => wgpu::PrimitiveTopology::TriangleStrip,
        _ => bail!("invalid primitive topology"),
    })
}
fn front_face(id: u8) -> Result<wgpu::FrontFace> {
    Ok(match id {
        1 => wgpu::FrontFace::Ccw,
        2 => wgpu::FrontFace::Cw,
        _ => bail!("invalid front face"),
    })
}
fn cull_mode(id: u8) -> Result<Option<wgpu::Face>> {
    Ok(match id {
        0 => None,
        1 => Some(wgpu::Face::Front),
        2 => Some(wgpu::Face::Back),
        _ => bail!("invalid cull mode"),
    })
}
fn vertex_step(id: u8) -> Result<wgpu::VertexStepMode> {
    Ok(match id {
        1 => wgpu::VertexStepMode::Vertex,
        2 => wgpu::VertexStepMode::Instance,
        _ => bail!("invalid vertex step mode"),
    })
}
fn vertex_format(id: u16) -> Result<wgpu::VertexFormat> {
    Ok(match id {
        1 => wgpu::VertexFormat::Float32,
        2 => wgpu::VertexFormat::Float32x2,
        3 => wgpu::VertexFormat::Float32x3,
        4 => wgpu::VertexFormat::Float32x4,
        5 => wgpu::VertexFormat::Uint32,
        6 => wgpu::VertexFormat::Uint32x2,
        7 => wgpu::VertexFormat::Uint32x4,
        8 => wgpu::VertexFormat::Unorm8x2,
        9 => wgpu::VertexFormat::Unorm8x4,
        10 => wgpu::VertexFormat::Snorm8x2,
        11 => wgpu::VertexFormat::Snorm8x4,
        _ => bail!("invalid vertex format"),
    })
}
fn blend_operation(id: u8) -> Result<wgpu::BlendOperation> {
    Ok(match id {
        1 => wgpu::BlendOperation::Add,
        2 => wgpu::BlendOperation::Subtract,
        3 => wgpu::BlendOperation::ReverseSubtract,
        4 => wgpu::BlendOperation::Min,
        5 => wgpu::BlendOperation::Max,
        _ => bail!("invalid blend operation"),
    })
}
fn blend_factor(id: u8) -> Result<wgpu::BlendFactor> {
    Ok(match id {
        1 => wgpu::BlendFactor::Zero,
        2 => wgpu::BlendFactor::One,
        3 => wgpu::BlendFactor::Src,
        4 => wgpu::BlendFactor::OneMinusSrc,
        5 => wgpu::BlendFactor::SrcAlpha,
        6 => wgpu::BlendFactor::OneMinusSrcAlpha,
        7 => wgpu::BlendFactor::Dst,
        8 => wgpu::BlendFactor::OneMinusDst,
        9 => wgpu::BlendFactor::DstAlpha,
        10 => wgpu::BlendFactor::OneMinusDstAlpha,
        11 => wgpu::BlendFactor::SrcAlphaSaturated,
        12 => wgpu::BlendFactor::Constant,
        13 => wgpu::BlendFactor::OneMinusConstant,
        _ => bail!("invalid blend factor"),
    })
}
fn texture_aspect(id: u8) -> Result<wgpu::TextureAspect> {
    Ok(match id {
        1 => wgpu::TextureAspect::All,
        2 => wgpu::TextureAspect::DepthOnly,
        _ => bail!("invalid texture aspect"),
    })
}

fn submission_complete(sequence: u64) -> Vec<u8> {
    let mut bytes = vec![0; EVENT_HEADER_BYTES];
    bytes[..4].copy_from_slice(&gpu_wire::GPU_EVENT_MAGIC);
    bytes[4..6].copy_from_slice(&gpu_wire::GPU_WIRE_VERSION.to_le_bytes());
    bytes[6..8].copy_from_slice(&(gpu_wire::GpuEventType::SubmissionComplete as u16).to_le_bytes());
    bytes[8..12].copy_from_slice(&(EVENT_HEADER_BYTES as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&sequence.to_le_bytes());
    bytes
}
fn batch_rejected(sequence: u64, message: &str) -> Vec<u8> {
    let mut end = message.len().min(gpu_wire::MAX_GPU_DIAGNOSTIC_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    let text = &message.as_bytes()[..end];
    let padded = text.len().div_ceil(4) * 4;
    let mut bytes = vec![0; EVENT_HEADER_BYTES + 16 + padded];
    bytes[..4].copy_from_slice(&gpu_wire::GPU_EVENT_MAGIC);
    bytes[4..6].copy_from_slice(&gpu_wire::GPU_WIRE_VERSION.to_le_bytes());
    bytes[6..8].copy_from_slice(&(gpu_wire::GpuEventType::BatchRejected as u16).to_le_bytes());
    let len = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&len.to_le_bytes());
    bytes[16..24].copy_from_slice(&sequence.to_le_bytes());
    bytes[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&(text.len() as u32).to_le_bytes());
    bytes[36..40].copy_from_slice(&u32::from(message.len() > text.len()).to_le_bytes());
    bytes[40..40 + text.len()].copy_from_slice(text);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_overflowing_and_empty_resource_ranges() {
        assert!(buffer_range(u64::MAX, 2, 64).is_err());
        assert!(buffer_range(64, 0, 64).is_err());
        assert!(binding_size(65, 0, 64).is_err());
        assert!(binding_size(8, u64::MAX, 64).is_err());
        assert_eq!(binding_size(8, 0, 64).unwrap(), 56);
        assert_eq!(buffer_range(4, 60, 64).unwrap(), 4..64);
        assert!(checked_buffer_end(u64::MAX, 4, 64).is_err());
        assert!(draw_range(u32::MAX, 1).is_err());
        assert_eq!(draw_range(7, 0).unwrap(), 7..7);
        assert!(Reader::new(&[0; 4])
            .count(u32::MAX as usize, 32, 16)
            .is_err());
        assert!(Reader::new(&[0; 4]).count(2, 4, 16).is_err());
    }

    #[test]
    fn bounds_descriptors_before_backend_allocations() {
        assert!(validate_buffer_descriptor(64, wgpu::BufferUsages::empty()).is_err());
        assert!(validate_buffer_descriptor(u64::MAX, wgpu::BufferUsages::COPY_DST).is_err());
        assert!(validate_buffer_descriptor(
            4,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::VERTEX
        )
        .is_err());
        validate_buffer_descriptor(4, wgpu::BufferUsages::COPY_DST).unwrap();
        assert!(texture_bytes(0, 1, 1, 1).is_err());
        assert!(texture_bytes(u32::MAX, 1, 1, 1).is_err());
        assert!(texture_bytes(1, 1, 13, 1).is_err());
        assert!(texture_bytes(4, 4, 1, 4).is_err());
        assert_eq!(texture_bytes(4, 2, 3, 1).unwrap(), 44);
    }

    #[test]
    fn preserves_texture_pixels_and_bounds_final_row_padding() {
        let tight = [1, 2, 3, 4, 5, 6, 7, 8];
        let (data, stride) = prepare_texture_upload(&tight, 4, 2, 4).unwrap();
        assert_eq!(data.as_ref(), tight);
        assert_eq!(stride, 4);
        let short_last_row = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8];
        let (data, stride) = prepare_texture_upload(&short_last_row, 4, 2, 8).unwrap();
        assert_eq!(
            data.as_ref(),
            &[1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0]
        );
        assert_eq!(stride, 8);
        let (data, stride) = prepare_texture_upload(&tight[..4], 4, 1, u32::MAX).unwrap();
        assert_eq!(data.as_ref(), &tight[..4]);
        assert_eq!(stride, 4);
        assert!(prepare_texture_upload(&tight, 4, 2, u32::MAX).is_err());
        assert!(prepare_texture_upload(&tight, 4, 2, 3).is_err());
    }

    #[test]
    fn charges_follow_dependencies_and_failed_reservations_do_not_leak() {
        for kind in 0..RESOURCE_LIMITS.len() {
            let quotas = Arc::new(Quotas::default());
            let mut batch = BatchBudget::default();
            let bytes = if kind < 2 { 4 } else { 0 };
            let root = quotas.reserve(kind, bytes, vec![], &mut batch).unwrap();
            let middle = quotas
                .reserve(7, 0, vec![Arc::clone(&root)], &mut batch)
                .unwrap();
            let leaf = quotas
                .reserve(6, 0, vec![Arc::clone(&middle)], &mut batch)
                .unwrap();
            drop(root);
            drop(middle);
            assert_eq!(
                quotas.counts[kind].load(Ordering::Relaxed),
                1 + usize::from(kind == 7) + usize::from(kind == 6)
            );
            drop(leaf);
            assert!(quotas
                .counts
                .iter()
                .all(|count| count.load(Ordering::Relaxed) == 0));
            assert_eq!(quotas.buffer_bytes.load(Ordering::Relaxed), 0);
            assert_eq!(quotas.texture_bytes.load(Ordering::Relaxed), 0);
        }
        for (kind, limit) in [
            (0, gpu_wire::MAX_GPU_TOTAL_BUFFER_BYTES),
            (1, gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES),
        ] {
            let quotas = Arc::new(Quotas::default());
            let mut batch = BatchBudget::default();
            let root = quotas.reserve(kind, limit, vec![], &mut batch).unwrap();
            assert!(quotas
                .reserve(kind, 1, vec![], &mut BatchBudget::default())
                .is_err());
            assert_eq!(quotas.counts[kind].load(Ordering::Relaxed), 1);
            drop(root);
            // Destroying a handle does not reset the per-batch allocation budget.
            assert!(quotas.reserve(kind, 1, vec![], &mut batch).is_err());
            let next = quotas
                .reserve(kind, 4, vec![], &mut BatchBudget::default())
                .unwrap();
            drop(next);
            assert_eq!(quotas.buffer_bytes.load(Ordering::Relaxed), 0);
            assert_eq!(quotas.texture_bytes.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn lifetime_count_and_compilation_budgets_survive_handle_churn() {
        for (kind, limit) in RESOURCE_LIMITS.iter().copied().enumerate() {
            let quotas = Arc::new(Quotas::default());
            let resources: Vec<_> = (0..limit)
                .map(|_| {
                    quotas
                        .reserve(kind, 0, vec![], &mut BatchBudget::default())
                        .unwrap()
                })
                .collect();
            assert!(quotas
                .reserve(kind, 0, vec![], &mut BatchBudget::default())
                .is_err());
            drop(resources);
            assert_eq!(quotas.counts[kind].load(Ordering::Relaxed), 0);
        }
        let quotas = Arc::new(Quotas::default());
        let mut batch = BatchBudget::default();
        for _ in 0..gpu_wire::MAX_GPU_COMPILATIONS {
            drop(quotas.reserve(4, 0, vec![], &mut batch).unwrap());
        }
        assert!(quotas.reserve(8, 0, vec![], &mut batch).is_err());
        assert_eq!(quotas.counts[8].load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rejection_diagnostic_is_bounded_valid_utf8() {
        let message = format!("{}é", "x".repeat(gpu_wire::MAX_GPU_DIAGNOSTIC_BYTES - 1));
        let event = batch_rejected(7, &message);
        let length = u32::from_le_bytes(event[32..36].try_into().unwrap()) as usize;
        assert_eq!(length, gpu_wire::MAX_GPU_DIAGNOSTIC_BYTES - 1);
        assert_eq!(u32::from_le_bytes(event[36..40].try_into().unwrap()), 1);
        assert_eq!(
            std::str::from_utf8(&event[40..40 + length]).unwrap(),
            &message[..length]
        );
    }

    fn wire_batch(commands: &[(GpuOpcode, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = vec![0; 24];
        bytes[..4].copy_from_slice(b"EPG1");
        bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&(commands.len() as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&1u64.to_le_bytes());
        for (opcode, payload) in commands {
            bytes.extend_from_slice(&(*opcode as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&((payload.len() + 8) as u32).to_le_bytes());
            bytes.extend_from_slice(payload);
        }
        let length = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&length.to_le_bytes());
        gpu_wire::decode_gpu_batch(&bytes).unwrap();
        bytes
    }

    fn buffer_command(id: u32, usage: u32) -> (GpuOpcode, Vec<u8>) {
        let mut payload = id.to_le_bytes().to_vec();
        payload.extend_from_slice(&usage.to_le_bytes());
        payload.extend_from_slice(&4u64.to_le_bytes());
        (GpuOpcode::CreateBuffer, payload)
    }

    fn assert_event(output: &NativeGpuOutput, kind: gpu_wire::GpuEventType) {
        assert_eq!(
            u16::from_le_bytes(output.events[0][6..8].try_into().unwrap()),
            kind as u16,
            "event: {:?}",
            String::from_utf8_lossy(&output.events[0])
        );
    }

    // Run explicitly with a real adapter (software Vulkan is sufficient):
    // cargo test -p pvm-runtime --features native-gpu native_gpu::tests::gpu_ -- --ignored
    #[test]
    #[ignore = "requires a native WebGPU adapter"]
    fn gpu_rejects_bad_descriptors_and_wgsl_then_renders() {
        let mut renderer = NativeGpuRenderer::new(1, 1).unwrap();
        let mut malformed = buffer_command(1, 0);
        malformed.1[8..16].copy_from_slice(&64u64.to_le_bytes());
        assert_event(
            &renderer.execute(&wire_batch(&[malformed])),
            gpu_wire::GpuEventType::BatchRejected,
        );
        let mut shader = 2u32.to_le_bytes().to_vec();
        shader.extend_from_slice(&8u32.to_le_bytes());
        shader.extend_from_slice(b"not WGSL");
        assert_event(
            &renderer.execute(&wire_batch(&[
                buffer_command(3, wgpu::BufferUsages::VERTEX.bits()),
                (GpuOpcode::CreateShaderWgsl, shader),
            ])),
            gpu_wire::GpuEventType::BatchRejected,
        );
        assert!(renderer.resources.contains_key(&3));
        assert_eq!(renderer.quotas.counts[0].load(Ordering::Relaxed), 1);
        assert!(!renderer.resources.contains_key(&2));
        assert_eq!(renderer.quotas.counts[4].load(Ordering::Relaxed), 0);
        assert_event(
            &renderer.execute(&wire_batch(&[buffer_command(
                1,
                wgpu::BufferUsages::COPY_DST.bits(),
            )])),
            gpu_wire::GpuEventType::SubmissionComplete,
        );
        let mut pass = vec![0; 36];
        pass[8..12].copy_from_slice(&1u32.to_le_bytes());
        pass[12..16].copy_from_slice(&2u32.to_le_bytes());
        pass[16..20].copy_from_slice(&1f32.to_le_bytes());
        pass[28..32].copy_from_slice(&1f32.to_le_bytes());
        let mut bad_slice = vec![0; 24];
        bad_slice[4..8].copy_from_slice(&3u32.to_le_bytes());
        bad_slice[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        bad_slice[16..24].copy_from_slice(&1u64.to_le_bytes());
        let mut bad_draw = vec![0; 16];
        bad_draw[..4].copy_from_slice(&1u32.to_le_bytes());
        bad_draw[4..8].copy_from_slice(&1u32.to_le_bytes());
        bad_draw[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        for operation in [
            (GpuOpcode::SetVertexBuffer, bad_slice),
            (GpuOpcode::Draw, bad_draw),
        ] {
            assert_event(
                &renderer.execute(&wire_batch(&[
                    (GpuOpcode::BeginRenderPass, pass.clone()),
                    operation,
                    (GpuOpcode::EndRenderPass, vec![]),
                ])),
                gpu_wire::GpuEventType::BatchRejected,
            );
        }
        let output = renderer.execute(&wire_batch(&[
            (GpuOpcode::BeginRenderPass, pass),
            (GpuOpcode::EndRenderPass, vec![]),
        ]));
        assert_event(&output, gpu_wire::GpuEventType::SubmissionComplete);
        assert_eq!(output.frame.unwrap().rgba, [255, 0, 0, 255]);
    }

    #[test]
    #[ignore = "requires a native WebGPU adapter"]
    fn gpu_retained_buffers_cannot_evade_lifetime_quota() {
        let mut renderer = NativeGpuRenderer::new(1, 1).unwrap();
        let commands: Vec<_> = (1..=gpu_wire::MAX_GPU_BUFFERS as u32)
            .map(|id| buffer_command(id, wgpu::BufferUsages::UNIFORM.bits()))
            .collect();
        assert_event(
            &renderer.execute(&wire_batch(&commands)),
            gpu_wire::GpuEventType::SubmissionComplete,
        );
        let mut layout = vec![0; 40];
        layout[..4].copy_from_slice(&5000u32.to_le_bytes());
        layout[4..8].copy_from_slice(&1u32.to_le_bytes());
        layout[12..16].copy_from_slice(&wgpu::ShaderStages::COMPUTE.bits().to_le_bytes());
        layout[16..18].copy_from_slice(&1u16.to_le_bytes());
        layout[24..32].copy_from_slice(&4u64.to_le_bytes());
        let mut group = vec![0; 44];
        group[..4].copy_from_slice(&5001u32.to_le_bytes());
        group[4..8].copy_from_slice(&5000u32.to_le_bytes());
        group[8..12].copy_from_slice(&1u32.to_le_bytes());
        group[16..20].copy_from_slice(&1u32.to_le_bytes());
        group[20..22].copy_from_slice(&1u16.to_le_bytes());
        group[36..44].copy_from_slice(&4u64.to_le_bytes());
        assert_event(
            &renderer.execute(&wire_batch(&[
                (GpuOpcode::CreateBindGroupLayout, layout),
                (GpuOpcode::CreateBindGroup, group),
                (GpuOpcode::DestroyResource, 1u32.to_le_bytes().to_vec()),
            ])),
            gpu_wire::GpuEventType::SubmissionComplete,
        );
        assert_event(
            &renderer.execute(&wire_batch(&[buffer_command(
                4097,
                wgpu::BufferUsages::UNIFORM.bits(),
            )])),
            gpu_wire::GpuEventType::BatchRejected,
        );
        assert_event(
            &renderer.execute(&wire_batch(&[(
                GpuOpcode::DestroyResource,
                5001u32.to_le_bytes().to_vec(),
            )])),
            gpu_wire::GpuEventType::SubmissionComplete,
        );
        assert_event(
            &renderer.execute(&wire_batch(&[buffer_command(
                4097,
                wgpu::BufferUsages::UNIFORM.bits(),
            )])),
            gpu_wire::GpuEventType::SubmissionComplete,
        );
    }

    #[test]
    #[ignore = "requires a native WebGPU adapter"]
    fn gpu_texture_upload_preserves_pixels_with_short_final_row() {
        let mut renderer = NativeGpuRenderer::new(1, 2).unwrap();
        let mut texture = vec![0; 24];
        texture[..4].copy_from_slice(&1u32.to_le_bytes());
        texture[4..8].copy_from_slice(&1u32.to_le_bytes());
        texture[8..12].copy_from_slice(&2u32.to_le_bytes());
        texture[12..14].copy_from_slice(&1u16.to_le_bytes());
        texture[14..16].copy_from_slice(&1u16.to_le_bytes());
        texture[16..18].copy_from_slice(&1u16.to_le_bytes());
        texture[18] = 1;
        texture[20..24].copy_from_slice(
            &(wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC)
                .bits()
                .to_le_bytes(),
        );
        let mut upload = vec![0; 44];
        for (offset, value) in [
            (0, 1u32),
            (20, 1),
            (24, 2),
            (28, 1),
            (32, 8),
            (36, 2),
            (40, 12),
        ] {
            upload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        upload.extend_from_slice(&[255, 0, 0, 255, 0, 0, 0, 0, 0, 255, 0, 255]);
        assert_event(
            &renderer.execute(&wire_batch(&[
                (GpuOpcode::CreateTexture, texture),
                (GpuOpcode::WriteTexture, upload),
            ])),
            gpu_wire::GpuEventType::SubmissionComplete,
        );
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: renderer.texture(1).unwrap(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &renderer.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(renderer.padded_row_bytes),
                    rows_per_image: Some(2),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 2,
                depth_or_array_layers: 1,
            },
        );
        renderer.queue.submit([encoder.finish()]);
        assert_eq!(
            renderer.read_frame().unwrap().rgba,
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
    }

    #[test]
    fn capabilities_record_matches_runtime_contract() {
        let bytes = encode_capabilities(800, 600, 7);

        crate::validate_gpu_capabilities(&bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[44..48].try_into().unwrap()), 21);
        assert_eq!(u16::from_le_bytes(bytes[56..58].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(bytes[60..68].try_into().unwrap()), 4096);
        assert_eq!(
            u16::from_le_bytes(bytes[376..378].try_into().unwrap()),
            gpu_wire::GpuCapabilityKey::MaxDispatchesPerBatch as u16
        );
        assert_eq!(
            u64::from_le_bytes(bytes[380..388].try_into().unwrap()),
            gpu_wire::MAX_GPU_DISPATCHES_PER_BATCH as u64
        );
    }

    #[test]
    fn native_required_limits_cover_advertised_contract() {
        let limits = native_required_limits();

        assert!(limits.max_texture_dimension_2d >= gpu_wire::MAX_GPU_TEXTURE_DIMENSION_2D);
        assert!(limits.max_buffer_size >= gpu_wire::MAX_GPU_BUFFER_BYTES as u64);
        assert!(limits.max_storage_buffer_binding_size >= gpu_wire::MAX_GPU_BUFFER_BYTES as u32);
        assert!(
            limits.max_storage_buffers_per_shader_stage >= MAX_STORAGE_BUFFERS_PER_SHADER_STAGE
        );
        assert!(limits.max_compute_workgroup_storage_size >= MAX_COMPUTE_WORKGROUP_STORAGE_SIZE);
        assert!(
            limits.max_compute_workgroups_per_dimension >= MAX_COMPUTE_WORKGROUPS_PER_DIMENSION
        );
    }

    #[test]
    fn rejects_compute_dispatches_outside_native_contract() {
        let mut dispatches = 0;
        validate_compute_dispatch([1, 1, 1], &mut dispatches).unwrap();
        assert_eq!(dispatches, 1);

        assert!(validate_compute_dispatch([0, 1, 1], &mut dispatches).is_err());
        assert!(validate_compute_dispatch(
            [MAX_COMPUTE_WORKGROUPS_PER_DIMENSION + 1, 1, 1],
            &mut dispatches
        )
        .is_err());

        let mut saturated = gpu_wire::MAX_GPU_DISPATCHES_PER_BATCH;
        assert!(validate_compute_dispatch([1, 1, 1], &mut saturated).is_err());
    }

    #[test]
    fn rejects_vertex_visible_writable_storage_layouts() {
        assert!(validate_binding_visibility(5, wgpu::ShaderStages::VERTEX).is_err());
        validate_binding_visibility(5, wgpu::ShaderStages::COMPUTE).unwrap();
        validate_binding_visibility(4, wgpu::ShaderStages::VERTEX).unwrap();
    }

    #[test]
    fn maps_extended_gpu_contract_values() {
        assert_eq!(texture_format(7).unwrap(), wgpu::TextureFormat::R8Unorm);
        assert_eq!(
            buffer_binding_type(4, 0, 0, 0).unwrap(),
            wgpu::BufferBindingType::Storage { read_only: true }
        );
        assert_eq!(
            buffer_binding_type(5, 0, 0, 0).unwrap(),
            wgpu::BufferBindingType::Storage { read_only: false }
        );
        assert_eq!(
            buffer_binding_type(4, 2, 0, 0).unwrap_err().to_string(),
            "invalid buffer binding layout"
        );
    }
}
