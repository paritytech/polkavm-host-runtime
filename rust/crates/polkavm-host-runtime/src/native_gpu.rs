use crate::gpu_wire::{self, GpuOpcode};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc};

const FORMAT_RGBA8_UNORM: u16 = 1;
const EVENT_HEADER_BYTES: usize = 24;
const MAX_COMPUTE_WORKGROUP_STORAGE_SIZE: u32 = 16 * 1024;
const MAX_STORAGE_BUFFERS_PER_SHADER_STAGE: u32 = 8;
const MAX_COMPUTE_WORKGROUPS_PER_DIMENSION: u32 = 65_535;

#[derive(Debug)]
pub struct NativeGpuFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub struct NativeGpuOutput {
    pub events: Vec<Vec<u8>>,
    pub frame: Option<NativeGpuFrame>,
}

enum Resource {
    Buffer {
        value: wgpu::Buffer,
        size: u64,
    },
    Texture {
        value: wgpu::Texture,
        bytes: usize,
    },
    TextureView(NativeTextureView),
    Sampler(wgpu::Sampler),
    Shader(wgpu::ShaderModule),
    BindGroupLayout {
        value: wgpu::BindGroupLayout,
        entries: Vec<wgpu::BindGroupLayoutEntry>,
    },
    PipelineLayout(wgpu::PipelineLayout),
    BindGroup(wgpu::BindGroup),
    RenderPipeline(wgpu::RenderPipeline),
    ComputePipeline(wgpu::ComputePipeline),
}

struct NativeTextureView {
    value: wgpu::TextureView,
    dimension: wgpu::TextureViewDimension,
    format: wgpu::TextureFormat,
    aspect: wgpu::TextureAspect,
    usage: wgpu::TextureUsages,
    mip_level_count: u32,
}

struct PendingPass {
    color_view: u32,
    depth_view: u32,
    flags: u32,
    clear_color: wgpu::Color,
    clear_depth: f32,
    clear_stencil: u32,
    operations: Vec<RenderOperation>,
    occlusion: Option<PassOcclusion>,
}

/// Occlusion queries declared by `BeginRenderPass`, validated as recorded.
struct PassOcclusion {
    count: u32,
    token: u32,
    used: Vec<bool>,
    open: bool,
}

/// A resolved occlusion pass whose results are copied into `buffer`.
struct OcclusionReadback {
    token: u32,
    count: u32,
    buffer: wgpu::Buffer,
}

const OCCLUSION_PENDING: u8 = 0;
const OCCLUSION_MAPPED: u8 = 1;
const OCCLUSION_FAILED: u8 = 2;

/// A submitted readback; `state` is set by the `map_async` callback.
struct PendingOcclusion {
    sequence: u64,
    readback: OcclusionReadback,
    state: Arc<AtomicU8>,
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
    StencilReference(u32),
    BlendConstant([f32; 4]),
    BeginOcclusionQuery(u32),
    EndOcclusionQuery,
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

pub struct NativeGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resources: HashMap<u32, Resource>,
    texture_bytes: usize,
    texture_count: usize,
    surface: wgpu::Texture,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row_bytes: u32,
    generation: u32,
    occlusion_results: VecDeque<PendingOcclusion>,
}

impl NativeGpuRenderer {
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
        let (surface, readback, padded_row_bytes) = create_surface(&device, width, height);
        Ok(Self {
            device,
            queue,
            resources: HashMap::new(),
            texture_bytes: 0,
            texture_count: 0,
            surface,
            readback,
            width,
            height,
            padded_row_bytes,
            generation: 1,
            occlusion_results: VecDeque::new(),
        })
    }

    pub fn capabilities(&self) -> Vec<u8> {
        encode_capabilities(self.width, self.height, self.generation)
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            bail!("invalid native GPU surface dimensions");
        }
        if self.width == width && self.height == height {
            return Ok(());
        }
        let (surface, readback, padded) = create_surface(&self.device, width, height);
        self.surface = surface;
        self.readback = readback;
        self.width = width;
        self.height = height;
        self.padded_row_bytes = padded;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("GPU surface generation overflow"))?;
        Ok(())
    }

    /// Executes one batch. Its completion event comes first, followed by any
    /// occlusion results that have become readable, in submission order.
    pub fn execute(&mut self, batch_bytes: &[u8]) -> NativeGpuOutput {
        let sequence = gpu_wire::decode_gpu_batch(batch_bytes)
            .map(|batch| batch.sequence())
            .unwrap_or(0);
        let mut output = match self.execute_inner(batch_bytes) {
            Ok(frame) => NativeGpuOutput {
                events: vec![submission_complete(sequence)],
                frame,
            },
            Err(error) => NativeGpuOutput {
                events: vec![batch_rejected(sequence, &format!("{error:#}"))],
                frame: None,
            },
        };
        output.events.extend(self.poll_events());
        output
    }

    /// Returns occlusion-result events whose readback has completed, without
    /// waiting for the GPU. Hosts that stop submitting batches call this to
    /// receive outstanding results.
    pub fn poll_events(&mut self) -> Vec<Vec<u8>> {
        self.device.poll(wgpu::Maintain::Poll);
        let mut events = Vec::new();
        while let Some(pending) = self.occlusion_results.front() {
            let state = pending.state.load(Ordering::Acquire);
            if state == OCCLUSION_PENDING {
                break;
            }
            let pending = self.occlusion_results.pop_front().unwrap();
            // A failed mapping means the device was lost with the submission.
            if state != OCCLUSION_MAPPED {
                continue;
            }
            let buffer = &pending.readback.buffer;
            events.push(occlusion_results(
                pending.sequence,
                pending.readback.token,
                pending.readback.count,
                &buffer.slice(..).get_mapped_range(),
            ));
            buffer.unmap();
        }
        events
    }

    fn execute_inner(&mut self, batch_bytes: &[u8]) -> Result<Option<NativeGpuFrame>> {
        let batch = gpu_wire::decode_gpu_batch(batch_bytes).context("decode native GPU batch")?;
        let mut encoder: Option<wgpu::CommandEncoder> = None;
        let mut pending_pass: Option<PendingPass> = None;
        let mut pending_compute_pass: Option<PendingComputePass> = None;
        let mut presented = false;
        let mut compute_dispatches = 0usize;
        let mut created_texture_bytes = 0usize;
        let mut occlusion_queries = 0u32;
        let mut occlusion_readbacks = Vec::new();
        for (index, command) in batch.commands().enumerate() {
            let mut reader = Reader::new(command.payload);
            match command.opcode {
                GpuOpcode::CreateBuffer => {
                    let id = reader.u32()?;
                    self.require_new(id)?;
                    let usage = wgpu::BufferUsages::from_bits(reader.u32()?)
                        .ok_or_else(|| anyhow!("invalid buffer usage"))?;
                    let size = reader.u64()?;
                    reader.finish()?;
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
                    self.queue.write_buffer(buffer, offset, data);
                }
                GpuOpcode::CreateTexture => {
                    let (id, descriptor, bytes) = read_texture_descriptor(&mut reader)?;
                    self.require_new(id)?;
                    let total = self
                        .texture_bytes
                        .checked_add(bytes)
                        .ok_or_else(|| anyhow!("texture memory quota overflow"))?;
                    let created = created_texture_bytes
                        .checked_add(bytes)
                        .ok_or_else(|| anyhow!("texture allocation budget overflow"))?;
                    if self.texture_count >= gpu_wire::MAX_GPU_TEXTURES
                        || total > gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES
                        || created > gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES
                    {
                        bail!("texture allocation quota exceeded");
                    }
                    let value = self.device.create_texture(&descriptor);
                    self.resources
                        .insert(id, Resource::Texture { value, bytes });
                    self.texture_bytes = total;
                    self.texture_count += 1;
                    created_texture_bytes = created;
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
                    validate_texture_write(
                        texture,
                        mip_level,
                        origin,
                        size,
                        bytes_per_row,
                        rows_per_image,
                        data.len(),
                    )?;
                    self.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture,
                            mip_level,
                            origin,
                            aspect: wgpu::TextureAspect::All,
                        },
                        data,
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
                    let source =
                        std::str::from_utf8(reader.take(length)?).context("WGSL is not UTF-8")?;
                    reader.zero_remaining()?;
                    let value = self
                        .device
                        .create_shader_module(wgpu::ShaderModuleDescriptor {
                            label: None,
                            source: wgpu::ShaderSource::Wgsl(source.into()),
                        });
                    self.resources.insert(id, Resource::Shader(value));
                }
                GpuOpcode::CreateBindGroupLayout => self.create_bind_group_layout(&mut reader)?,
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
                    match resource {
                        Resource::Buffer { value, .. } => value.destroy(),
                        Resource::Texture { value, bytes } => {
                            value.destroy();
                            self.texture_bytes -= bytes;
                            self.texture_count -= 1;
                        }
                        _ => {}
                    }
                }
                GpuOpcode::BeginRenderPass => {
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
                    let clear_stencil = if flags & gpu_wire::GPU_RENDER_PASS_HAS_STENCIL_CLEAR != 0
                    {
                        reader.u32()?
                    } else {
                        0
                    };
                    let occlusion = if flags & gpu_wire::GPU_RENDER_PASS_HAS_OCCLUSION_QUERIES != 0
                    {
                        let count = reader.u32()?;
                        let token = reader.u32()?;
                        if count == 0 {
                            bail!("occlusion query count must be nonzero");
                        }
                        occlusion_queries = occlusion_queries.saturating_add(count);
                        if occlusion_queries > gpu_wire::MAX_GPU_OCCLUSION_QUERIES_PER_BATCH {
                            bail!("occlusion query count exceeds the batch limit");
                        }
                        Some(PassOcclusion {
                            count,
                            token,
                            used: vec![false; count as usize],
                            open: false,
                        })
                    } else {
                        None
                    };
                    if occlusion.is_some()
                        && occlusion_readbacks.len() >= gpu_wire::MAX_GPU_RENDER_PASSES_PER_BATCH
                    {
                        bail!("too many occlusion query passes");
                    }
                    reader.finish()?;
                    if generation != self.generation {
                        bail!("stale render attachment");
                    }
                    if color_view != 0 {
                        self.texture_view(color_view)?;
                    }
                    let has_stencil = if depth_view != 0 {
                        has_stencil_aspect(self.texture_view(depth_view)?.format)
                    } else {
                        false
                    };
                    validate_stencil_pass(flags, clear_stencil, has_stencil)?;
                    pending_pass = Some(PendingPass {
                        color_view,
                        depth_view,
                        flags,
                        clear_color,
                        clear_depth,
                        clear_stencil,
                        operations: Vec::new(),
                        occlusion,
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
                    let format = index_format(reader.u32()? as u8)?;
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
                    let values = reader.u32_array::<4>()?;
                    reader.finish()?;
                    pending(&mut pending_pass)?
                        .operations
                        .push(RenderOperation::Draw(values));
                }
                GpuOpcode::SetStencilReference => {
                    let reference = reader.one_u32()?;
                    if reference > gpu_wire::MAX_GPU_STENCIL_VALUE {
                        bail!("stencil reference exceeds 255");
                    }
                    pending(&mut pending_pass)?
                        .operations
                        .push(RenderOperation::StencilReference(reference));
                }
                GpuOpcode::SetBlendConstant => {
                    let rgba = reader.f32_array::<4>()?;
                    reader.finish()?;
                    pending(&mut pending_pass)?
                        .operations
                        .push(RenderOperation::BlendConstant(rgba));
                }
                GpuOpcode::BeginOcclusionQuery => {
                    let query = reader.one_u32()?;
                    let occlusion = pending(&mut pending_pass)?
                        .occlusion
                        .as_mut()
                        .ok_or_else(|| anyhow!("render pass has no occlusion queries"))?;
                    if occlusion.open {
                        bail!("occlusion queries cannot nest");
                    }
                    if query >= occlusion.count {
                        bail!("occlusion query index exceeds the pass query count");
                    }
                    if std::mem::replace(&mut occlusion.used[query as usize], true) {
                        bail!("occlusion query index is reused within its pass");
                    }
                    occlusion.open = true;
                    pending(&mut pending_pass)?
                        .operations
                        .push(RenderOperation::BeginOcclusionQuery(query));
                }
                GpuOpcode::EndOcclusionQuery => {
                    reader.finish()?;
                    let pass = pending(&mut pending_pass)?;
                    match pass.occlusion.as_mut() {
                        Some(occlusion) if occlusion.open => occlusion.open = false,
                        _ => bail!("occlusion query is not active"),
                    }
                    pass.operations.push(RenderOperation::EndOcclusionQuery);
                }
                GpuOpcode::DrawIndexed => {
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
                    if pass
                        .occlusion
                        .as_ref()
                        .is_some_and(|occlusion| occlusion.open)
                    {
                        bail!("render pass ended with an open occlusion query");
                    }
                    let renders_to_surface = pass.color_view == 0;
                    let command_encoder = encoder.get_or_insert_with(|| {
                        self.device.create_command_encoder(&Default::default())
                    });
                    occlusion_readbacks.extend(self.encode_render_pass(command_encoder, pass)?);
                    presented |= renders_to_surface;
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
            let _ = index;
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
        // Results are delivered by later polls; mapping never blocks the batch.
        for readback in occlusion_readbacks {
            let state = Arc::new(AtomicU8::new(OCCLUSION_PENDING));
            let signal = Arc::clone(&state);
            readback
                .buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let value = if result.is_ok() {
                        OCCLUSION_MAPPED
                    } else {
                        OCCLUSION_FAILED
                    };
                    signal.store(value, Ordering::Release);
                });
            self.occlusion_results.push_back(PendingOcclusion {
                sequence: batch.sequence(),
                readback,
                state,
            });
        }
        if presented {
            self.read_frame().map(Some)
        } else {
            Ok(None)
        }
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
            Some(Resource::Texture { value, .. }) => Ok(value),
            _ => bail!("invalid texture {id}"),
        }
    }
    fn texture_view(&self, id: u32) -> Result<&NativeTextureView> {
        match self.resources.get(&id) {
            Some(Resource::TextureView(value)) => Ok(value),
            _ => bail!("invalid texture view {id}"),
        }
    }

    fn validate_texture_binding(
        &self,
        layout_id: u32,
        binding: u32,
        view: &NativeTextureView,
    ) -> Result<()> {
        let Some(Resource::BindGroupLayout { entries, .. }) = self.resources.get(&layout_id) else {
            bail!("invalid bind group layout {layout_id}");
        };
        let entry = entries
            .iter()
            .find(|entry| entry.binding == binding)
            .ok_or_else(|| anyhow!("texture binding is absent from layout"))?;
        let wgpu::BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled,
        } = &entry.ty
        else {
            bail!("texture binding kind does not match layout");
        };
        let sample_matches = match sample_type {
            wgpu::TextureSampleType::Depth => is_depth_format(view.format),
            wgpu::TextureSampleType::Float { filterable } => {
                !is_depth_format(view.format) || !*filterable
            }
            _ => false,
        };
        if *view_dimension != view.dimension
            || *multisampled
            || !sample_matches
            || (has_stencil_aspect(view.format) && view.aspect != wgpu::TextureAspect::DepthOnly)
            || !view.usage.contains(wgpu::TextureUsages::TEXTURE_BINDING)
        {
            bail!("texture view does not match binding layout");
        }
        Ok(())
    }

    fn render_texture_view(&self, id: u32, depth: bool) -> Result<&wgpu::TextureView> {
        let view = self.texture_view(id)?;
        if view.dimension != wgpu::TextureViewDimension::D2
            || view.mip_level_count != 1
            || is_depth_format(view.format) != depth
            || (has_stencil_aspect(view.format) && view.aspect != wgpu::TextureAspect::All)
            || !view.usage.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            bail!("texture view is not a compatible render attachment");
        }
        Ok(&view.value)
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
            Some(Resource::BindGroupLayout { value, .. }) => Ok(value),
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
                    view_dimension: texture_view_dimension(p1)?,
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
        self.resources
            .insert(id, Resource::BindGroupLayout { value, entries });
        Ok(())
    }

    fn create_pipeline_layout(&mut self, reader: &mut Reader<'_>) -> Result<()> {
        let id = reader.u32()?;
        self.require_new(id)?;
        let count = reader.u32()? as usize;
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
                } => wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: self.buffer(id)?.0,
                        offset,
                        size: NonZeroU64::new(size),
                    }),
                },
                EntrySpec::Sampler { binding, id } => wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::Sampler(self.sampler(id)?),
                },
                EntrySpec::Texture { binding, id } => {
                    let view = self.texture_view(id)?;
                    self.validate_texture_binding(layout_id, binding, view)?;
                    wgpu::BindGroupEntry {
                        binding,
                        resource: wgpu::BindingResource::TextureView(&view.value),
                    }
                }
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
        for _ in 0..attribute_count {
            attributes.push(wgpu::VertexAttribute {
                format: vertex_format(reader.u16()?)?,
                shader_location: reader.u16()? as u32,
                offset: reader.u64()?,
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
        let (stencil, bias) = if flags & gpu_wire::GPU_PIPELINE_STENCIL_DEPTH_BIAS != 0 {
            read_stencil_depth_bias(reader, topology)?
        } else {
            (
                wgpu::StencilState::default(),
                wgpu::DepthBiasState::default(),
            )
        };
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
            if flags & gpu_wire::GPU_PIPELINE_STENCIL_DEPTH_BIAS != 0 {
                bail!("depth, stencil or bias state without a depth format");
            }
            None
        } else {
            let format = texture_format(depth_format_id)?;
            if !is_depth_format(format) {
                bail!("pipeline depth format is not a depth format");
            }
            if !stencil_is_default(&stencil) && !has_stencil_aspect(format) {
                bail!("pipeline stencil state needs a stencil format");
            }
            Some(wgpu::DepthStencilState {
                format,
                depth_write_enabled: flags & gpu_wire::GPU_PIPELINE_DEPTH_WRITE != 0,
                depth_compare: compare(depth_compare_id)?,
                stencil,
                bias,
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
        let dimension = texture_view_dimension(reader.u8()? as u32)?;
        let aspect = texture_aspect(reader.u8()?)?;
        let base_mip_level = reader.u16()? as u32;
        let mip_level_count = reader.u16()? as u32;
        let base_array_layer = reader.u16()? as u32;
        let array_layer_count = reader.u16()? as u32;
        reader.finish()?;
        let texture = self.texture(texture_id)?;
        validate_texture_view(
            texture,
            format,
            dimension,
            aspect,
            (base_mip_level, mip_level_count),
            (base_array_layer, array_layer_count),
        )?;
        let value = texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: Some(format),
            dimension: Some(dimension),
            usage: None,
            aspect,
            base_mip_level,
            mip_level_count: Some(mip_level_count),
            base_array_layer,
            array_layer_count: (dimension != wgpu::TextureViewDimension::D3)
                .then_some(array_layer_count),
        });
        let view = NativeTextureView {
            value,
            dimension,
            aspect,
            format,
            usage: texture.usage(),
            mip_level_count,
        };
        self.resources.insert(id, Resource::TextureView(view));
        Ok(())
    }

    fn encode_render_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pending: PendingPass,
    ) -> Result<Option<OcclusionReadback>> {
        let surface_view;
        let color_view = if pending.color_view == 0 {
            surface_view = self.surface.create_view(&Default::default());
            &surface_view
        } else {
            self.render_texture_view(pending.color_view, false)?
        };
        let depth_view = if pending.depth_view == 0 {
            None
        } else {
            Some((
                self.render_texture_view(pending.depth_view, true)?,
                has_stencil_aspect(self.texture_view(pending.depth_view)?.format),
            ))
        };
        let color_attachment = Some(wgpu::RenderPassColorAttachment {
            view: color_view,
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
        let depth_attachment =
            depth_view.map(
                |(view, has_stencil)| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: if pending.flags & gpu_wire::GPU_RENDER_PASS_DEPTH_LOAD != 0 {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(pending.clear_depth)
                        },
                        store: if pending.flags & gpu_wire::GPU_RENDER_PASS_DEPTH_STORE != 0 {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: has_stencil.then_some(wgpu::Operations {
                        load: if pending.flags & gpu_wire::GPU_RENDER_PASS_STENCIL_LOAD != 0 {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(pending.clear_stencil)
                        },
                        store: if pending.flags & gpu_wire::GPU_RENDER_PASS_STENCIL_STORE != 0 {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                },
            );
        let occlusion = pending.occlusion.map(|occlusion| {
            let query_set = self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Occlusion,
                count: occlusion.count,
            });
            (query_set, occlusion)
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[color_attachment],
            depth_stencil_attachment: depth_attachment,
            timestamp_writes: None,
            occlusion_query_set: occlusion.as_ref().map(|(query_set, _)| query_set),
        });
        // WebGPU starts every pass with blend constant 0; wgpu instead rejects
        // constant-factor draws until a constant is set.
        pass.set_blend_constant(wgpu::Color::TRANSPARENT);
        for operation in pending.operations {
            match operation {
                RenderOperation::Pipeline(id) => pass.set_pipeline(self.render_pipeline(id)?),
                RenderOperation::VertexBuffer {
                    slot,
                    buffer,
                    offset,
                    size,
                } => pass
                    .set_vertex_buffer(slot, self.buffer(buffer)?.0.slice(offset..offset + size)),
                RenderOperation::IndexBuffer {
                    buffer,
                    format,
                    offset,
                    size,
                } => pass
                    .set_index_buffer(self.buffer(buffer)?.0.slice(offset..offset + size), format),
                RenderOperation::BindGroup {
                    slot,
                    bind_group,
                    offsets,
                } => pass.set_bind_group(slot, self.bind_group(bind_group)?, &offsets),
                RenderOperation::Viewport(values) => pass.set_viewport(
                    values[0], values[1], values[2], values[3], values[4], values[5],
                ),
                RenderOperation::Scissor(values) => {
                    pass.set_scissor_rect(values[0], values[1], values[2], values[3])
                }
                RenderOperation::Draw(values) => pass.draw(
                    values[2]..values[2] + values[0],
                    values[3]..values[3] + values[1],
                ),
                RenderOperation::StencilReference(reference) => {
                    pass.set_stencil_reference(reference)
                }
                RenderOperation::BlendConstant([r, g, b, a]) => {
                    pass.set_blend_constant(wgpu::Color {
                        r: r.into(),
                        g: g.into(),
                        b: b.into(),
                        a: a.into(),
                    })
                }
                RenderOperation::DrawIndexed {
                    indices,
                    instances,
                    first_index,
                    base_vertex,
                    first_instance,
                } => pass.draw_indexed(
                    first_index..first_index + indices,
                    base_vertex,
                    first_instance..first_instance + instances,
                ),
                RenderOperation::BeginOcclusionQuery(query) => pass.begin_occlusion_query(query),
                RenderOperation::EndOcclusionQuery => pass.end_occlusion_query(),
            }
        }
        let Some((query_set, occlusion)) = &occlusion else {
            return Ok(None);
        };
        // Unused indices run an empty query, so every result is defined (zero)
        // without relying on how a backend resolves never-written queries.
        for (query, used) in occlusion.used.iter().enumerate() {
            if !used {
                pass.begin_occlusion_query(query as u32);
                pass.end_occlusion_query();
            }
        }
        drop(pass);
        let bytes = u64::from(occlusion.count) * 8;
        let resolved = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.resolve_query_set(query_set, 0..occlusion.count, &resolved, 0);
        encoder.copy_buffer_to_buffer(&resolved, 0, &buffer, 0, bytes);
        Ok(Some(OcclusionReadback {
            token: occlusion.token,
            count: occlusion.count,
            buffer,
        }))
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

fn is_depth_format(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Depth24Plus
            | wgpu::TextureFormat::Depth32Float
            | wgpu::TextureFormat::Depth24PlusStencil8
    )
}

fn has_stencil_aspect(format: wgpu::TextureFormat) -> bool {
    format == wgpu::TextureFormat::Depth24PlusStencil8
}

/// Stencil flags only describe a stencil attachment; the clear value is one
/// stencil byte and replaces, never accompanies, a load.
fn validate_stencil_pass(flags: u32, clear_stencil: u32, has_stencil: bool) -> Result<()> {
    let stencil_flags = gpu_wire::GPU_RENDER_PASS_STENCIL_LOAD
        | gpu_wire::GPU_RENDER_PASS_STENCIL_STORE
        | gpu_wire::GPU_RENDER_PASS_HAS_STENCIL_CLEAR;
    if flags & stencil_flags != 0 && !has_stencil {
        bail!("stencil pass operations without a stencil attachment");
    }
    if flags & gpu_wire::GPU_RENDER_PASS_STENCIL_LOAD != 0
        && flags & gpu_wire::GPU_RENDER_PASS_HAS_STENCIL_CLEAR != 0
    {
        bail!("stencil attachment both loads and clears");
    }
    if clear_stencil > gpu_wire::MAX_GPU_STENCIL_VALUE {
        bail!("stencil clear value exceeds 255");
    }
    Ok(())
}

fn stencil_is_default(state: &wgpu::StencilState) -> bool {
    state.front == wgpu::StencilFaceState::IGNORE && state.back == wgpu::StencilFaceState::IGNORE
}

/// The CreateRenderPipeline trailer selected by
/// `GPU_PIPELINE_STENCIL_DEPTH_BIAS`: front and back faces as (compare, fail,
/// depth-fail, pass), read/write masks, then constant/slope/clamp depth bias.
fn read_stencil_depth_bias(
    reader: &mut Reader<'_>,
    topology: wgpu::PrimitiveTopology,
) -> Result<(wgpu::StencilState, wgpu::DepthBiasState)> {
    let mut faces = [wgpu::StencilFaceState::IGNORE; 2];
    for face in &mut faces {
        *face = wgpu::StencilFaceState {
            compare: compare(reader.u8()?)?,
            fail_op: stencil_operation(reader.u8()?)?,
            depth_fail_op: stencil_operation(reader.u8()?)?,
            pass_op: stencil_operation(reader.u8()?)?,
        };
    }
    let read_mask = reader.u8()? as u32;
    let write_mask = reader.u8()? as u32;
    reader.zero(2)?;
    let bias = wgpu::DepthBiasState {
        constant: reader.i32()?,
        slope_scale: reader.f32()?,
        clamp: reader.f32()?,
    };
    if (bias.constant != 0 || bias.slope_scale != 0.0 || bias.clamp != 0.0)
        && !matches!(
            topology,
            wgpu::PrimitiveTopology::TriangleList | wgpu::PrimitiveTopology::TriangleStrip
        )
    {
        bail!("depth bias applies to triangle topologies only");
    }
    Ok((
        wgpu::StencilState {
            front: faces[0],
            back: faces[1],
            read_mask,
            write_mask,
        },
        bias,
    ))
}

fn texture_view_dimension(value: u32) -> Result<wgpu::TextureViewDimension> {
    Ok(match value {
        1 => wgpu::TextureViewDimension::D2,
        2 => wgpu::TextureViewDimension::D2Array,
        3 => wgpu::TextureViewDimension::Cube,
        4 => wgpu::TextureViewDimension::CubeArray,
        5 => wgpu::TextureViewDimension::D3,
        _ => bail!("invalid texture view dimension"),
    })
}

fn read_texture_descriptor(
    reader: &mut Reader<'_>,
) -> Result<(u32, wgpu::TextureDescriptor<'static>, usize)> {
    let id = reader.u32()?;
    let width = reader.u32()?;
    let height = reader.u32()?;
    let mip_level_count = reader.u16()? as u32;
    let sample_count = reader.u16()? as u32;
    let format = texture_format(reader.u16()?)?;
    let dimension = match reader.u8()? {
        1 => wgpu::TextureDimension::D2,
        2 => wgpu::TextureDimension::D3,
        _ => bail!("invalid texture dimension"),
    };
    let flags = reader.u8()?;
    if flags & !gpu_wire::GPU_TEXTURE_HAS_DEPTH_OR_ARRAY_LAYERS != 0
        || (dimension == wgpu::TextureDimension::D3 && flags == 0)
    {
        bail!("invalid texture dimension flags");
    }
    let usage_bits = reader.u32()?;
    if usage_bits == 0 || usage_bits & !0x17 != 0 {
        bail!("invalid texture usage");
    }
    let usage = wgpu::TextureUsages::from_bits_retain(usage_bits);
    let depth_or_array_layers = if flags != 0 { reader.u32()? } else { 1 };
    reader.finish()?;
    let (max_xy, max_depth) = if dimension == wgpu::TextureDimension::D3 {
        (
            gpu_wire::MAX_GPU_TEXTURE_DIMENSION_3D,
            gpu_wire::MAX_GPU_TEXTURE_DIMENSION_3D,
        )
    } else {
        (
            gpu_wire::MAX_GPU_TEXTURE_DIMENSION_2D,
            gpu_wire::MAX_GPU_TEXTURE_ARRAY_LAYERS,
        )
    };
    if width == 0
        || height == 0
        || depth_or_array_layers == 0
        || width > max_xy
        || height > max_xy
        || depth_or_array_layers > max_depth
        || sample_count != 1
    {
        bail!("texture size or samples exceed negotiated limits");
    }
    if dimension == wgpu::TextureDimension::D3
        && (is_depth_format(format) || usage.contains(wgpu::TextureUsages::RENDER_ATTACHMENT))
    {
        bail!("3D textures support color sampling, not depth or render attachments");
    }
    let max_axis = width
        .max(height)
        .max(if dimension == wgpu::TextureDimension::D3 {
            depth_or_array_layers
        } else {
            1
        });
    if mip_level_count == 0
        || mip_level_count > gpu_wire::MAX_GPU_TEXTURE_MIP_LEVELS
        || mip_level_count > u32::BITS - max_axis.leading_zeros()
    {
        bail!("texture mip count exceeds dimensions");
    }
    // Match browser accounting: reserve four bytes per texel, including R8.
    let mut bytes = 0u64;
    for mip in 0..mip_level_count {
        let depth = if dimension == wgpu::TextureDimension::D3 {
            (depth_or_array_layers >> mip).max(1)
        } else {
            depth_or_array_layers
        };
        bytes += (width >> mip).max(1) as u64 * (height >> mip).max(1) as u64 * depth as u64 * 4;
    }
    if bytes > gpu_wire::MAX_GPU_TOTAL_TEXTURE_BYTES as u64 {
        bail!("texture exceeds memory quota");
    }
    Ok((
        id,
        wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers,
            },
            mip_level_count,
            sample_count,
            dimension,
            format,
            usage,
            view_formats: &[],
        },
        bytes as usize,
    ))
}

fn validate_texture_view(
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
    dimension: wgpu::TextureViewDimension,
    aspect: wgpu::TextureAspect,
    (base_mip, mip_count): (u32, u32),
    (base_layer, layer_count): (u32, u32),
) -> Result<()> {
    if format != texture.format()
        || (aspect == wgpu::TextureAspect::DepthOnly && !is_depth_format(format))
        || mip_count == 0
        || base_mip
            .checked_add(mip_count)
            .is_none_or(|end| end > texture.mip_level_count())
    {
        bail!("invalid texture view format, aspect or mip range");
    }
    if dimension == wgpu::TextureViewDimension::D3 {
        if texture.dimension() != wgpu::TextureDimension::D3 || base_layer != 0 || layer_count != 0
        {
            bail!("3D texture view cannot select array layers");
        }
        return Ok(());
    }
    if texture.dimension() != wgpu::TextureDimension::D2
        || layer_count == 0
        || base_layer
            .checked_add(layer_count)
            .is_none_or(|end| end > texture.depth_or_array_layers())
    {
        bail!("invalid texture view array range");
    }
    match dimension {
        wgpu::TextureViewDimension::D2 if layer_count != 1 => {
            bail!("2D texture view requires one layer");
        }
        wgpu::TextureViewDimension::Cube | wgpu::TextureViewDimension::CubeArray
            if (dimension == wgpu::TextureViewDimension::Cube && layer_count != 6)
                || !layer_count.is_multiple_of(6)
                || texture.width() != texture.height() =>
        {
            bail!("cube texture view requires a square texture and complete six-face cubes");
        }
        _ => {}
    }
    Ok(())
}

fn validate_texture_write(
    texture: &wgpu::Texture,
    mip: u32,
    origin: wgpu::Origin3d,
    size: wgpu::Extent3d,
    bytes_per_row: u32,
    rows_per_image: u32,
    data_length: usize,
) -> Result<()> {
    if mip >= texture.mip_level_count()
        || is_depth_format(texture.format())
        || !texture.usage().contains(wgpu::TextureUsages::COPY_DST)
    {
        bail!("texture does not permit this mip upload");
    }
    let depth = if texture.dimension() == wgpu::TextureDimension::D3 {
        (texture.depth_or_array_layers() >> mip).max(1)
    } else {
        texture.depth_or_array_layers()
    };
    for (offset, length, limit) in [
        (origin.x, size.width, (texture.width() >> mip).max(1)),
        (origin.y, size.height, (texture.height() >> mip).max(1)),
        (origin.z, size.depth_or_array_layers, depth),
    ] {
        if length == 0 || offset.checked_add(length).is_none_or(|end| end > limit) {
            bail!("texture upload exceeds subresource");
        }
    }
    let texel_bytes = if texture.format() == wgpu::TextureFormat::R8Unorm {
        1
    } else {
        4
    };
    let last_row = size.width as u64 * texel_bytes;
    if (bytes_per_row as u64) < last_row
        || !(bytes_per_row as u64).is_multiple_of(texel_bytes)
        || rows_per_image < size.height
    {
        bail!("texture upload strides do not cover its extent");
    }
    let required = (size.depth_or_array_layers as u64 - 1)
        .checked_mul(rows_per_image as u64)
        .and_then(|rows| rows.checked_add(size.height as u64 - 1))
        .and_then(|rows| rows.checked_mul(bytes_per_row as u64))
        .and_then(|bytes| bytes.checked_add(last_row))
        .ok_or_else(|| anyhow!("texture upload byte range overflow"))?;
    if required > data_length as u64 {
        bail!("texture upload data is shorter than its image strides");
    }
    Ok(())
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
        (
            22,
            gpu_wire::GPU_RASTER_FEATURE_LAYERED_TEXTURES
                | gpu_wire::GPU_RASTER_FEATURE_STENCIL_DEPTH_BIAS
                | gpu_wire::GPU_RASTER_FEATURE_BLEND_CONSTANT
                | gpu_wire::GPU_RASTER_FEATURE_OCCLUSION_QUERIES,
        ),
        (23, gpu_wire::MAX_GPU_TEXTURE_DIMENSION_3D as u64),
        (24, gpu_wire::MAX_GPU_TEXTURE_ARRAY_LAYERS as u64),
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
        max_texture_dimension_3d: gpu_wire::MAX_GPU_TEXTURE_DIMENSION_3D,
        max_texture_array_layers: gpu_wire::MAX_GPU_TEXTURE_ARRAY_LAYERS,
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
) -> (wgpu::Texture, wgpu::Buffer, u32) {
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
    (surface, readback, padded)
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
        8 => wgpu::TextureFormat::Depth24PlusStencil8,
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
fn stencil_operation(id: u8) -> Result<wgpu::StencilOperation> {
    Ok(match id {
        1 => wgpu::StencilOperation::Keep,
        2 => wgpu::StencilOperation::Zero,
        3 => wgpu::StencilOperation::Replace,
        4 => wgpu::StencilOperation::Invert,
        5 => wgpu::StencilOperation::IncrementClamp,
        6 => wgpu::StencilOperation::DecrementClamp,
        7 => wgpu::StencilOperation::IncrementWrap,
        8 => wgpu::StencilOperation::DecrementWrap,
        _ => bail!("invalid stencil operation"),
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
/// Event 9: guest token, query count, then one little-endian `u64` per query.
fn occlusion_results(sequence: u64, token: u32, count: u32, results: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; EVENT_HEADER_BYTES + gpu_wire::GPU_OCCLUSION_RESULTS_HEADER_BYTES];
    bytes[..4].copy_from_slice(&gpu_wire::GPU_EVENT_MAGIC);
    bytes[4..6].copy_from_slice(&gpu_wire::GPU_WIRE_VERSION.to_le_bytes());
    bytes[6..8].copy_from_slice(&(gpu_wire::GpuEventType::OcclusionResults as u16).to_le_bytes());
    bytes[16..24].copy_from_slice(&sequence.to_le_bytes());
    bytes[24..28].copy_from_slice(&token.to_le_bytes());
    bytes[28..32].copy_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(results);
    let len = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&len.to_le_bytes());
    bytes
}

fn batch_rejected(sequence: u64, message: &str) -> Vec<u8> {
    let text = message.as_bytes();
    let text = &text[..text.len().min(gpu_wire::MAX_GPU_DIAGNOSTIC_BYTES)];
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

    #[test]
    fn enforces_stencil_pass_and_depth_bias_rules() {
        use gpu_wire::{
            GPU_RENDER_PASS_HAS_STENCIL_CLEAR as CLEAR, GPU_RENDER_PASS_STENCIL_LOAD as LOAD,
            GPU_RENDER_PASS_STENCIL_STORE as STORE,
        };
        validate_stencil_pass(LOAD | STORE, 0, true).unwrap();
        validate_stencil_pass(CLEAR | STORE, 255, true).unwrap();
        assert!(validate_stencil_pass(STORE, 0, false).is_err());
        assert!(validate_stencil_pass(LOAD | CLEAR, 1, true).is_err());
        assert!(validate_stencil_pass(CLEAR, 256, true).is_err());

        let mut trailer = vec![3, 1, 1, 3, 3, 1, 1, 3, 0xff, 0x0f, 0, 0];
        trailer.extend_from_slice(&(-8i32).to_le_bytes());
        trailer.extend_from_slice(&(-2.0f32).to_le_bytes());
        trailer.extend_from_slice(&0.0f32.to_le_bytes());
        let (stencil, bias) = read_stencil_depth_bias(
            &mut Reader::new(&trailer),
            wgpu::PrimitiveTopology::TriangleStrip,
        )
        .unwrap();
        assert_eq!(stencil.front.compare, wgpu::CompareFunction::Equal);
        assert_eq!(stencil.back.pass_op, wgpu::StencilOperation::Replace);
        assert_eq!((stencil.read_mask, stencil.write_mask), (0xff, 0x0f));
        assert_eq!((bias.constant, bias.slope_scale), (-8, -2.0));
        assert!(read_stencil_depth_bias(
            &mut Reader::new(&trailer),
            wgpu::PrimitiveTopology::LineList
        )
        .is_err());
    }
}
