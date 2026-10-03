/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Bounded little-endian GPU command encoding shared by guests and hosts.
#![no_std]

#[cfg(feature = "tri2d-validation")]
extern crate std;

/// Tri2D command encoding and optional stateful validation.
pub mod tri2d;

use core::fmt;

/// Batch discriminator, preceding the version and length fields.
pub const GPU_WIRE_MAGIC: [u8; 4] = *b"EPG1";
/// Supported GPU protocol version.
pub const GPU_WIRE_VERSION: u16 = 1;
/// Fixed batch header size in bytes.
pub const GPU_BATCH_HEADER_BYTES: usize = 24;
/// Fixed command header size in bytes, included in command lengths.
pub const GPU_COMMAND_HEADER_BYTES: usize = 8;
/// Maximum encoded size of one batch, including headers, in bytes.
pub const MAX_GPU_BATCH_BYTES: usize = 4 * 1024 * 1024;
/// Maximum commands encoded in one batch.
pub const MAX_GPU_COMMANDS: u32 = 16_384;
/// Discriminator for a host capabilities record.
pub const GPU_CAPABILITIES_MAGIC: [u8; 4] = *b"EGC1";
/// Fixed capabilities header size in bytes.
pub const GPU_CAPABILITIES_HEADER_BYTES: usize = 56;
/// Size in bytes of each key/value capabilities entry.
pub const GPU_CAPABILITY_ENTRY_BYTES: usize = 16;
/// Discriminator for a host-to-guest event record.
pub const GPU_EVENT_MAGIC: [u8; 4] = *b"EGE1";
/// Fixed event header size in bytes.
pub const GPU_EVENT_HEADER_BYTES: usize = 24;
/// Maximum encoded event size in bytes.
pub const MAX_GPU_EVENT_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 diagnostic payload size in bytes.
pub const MAX_GPU_DIAGNOSTIC_BYTES: usize = 8 * 1024;
/// Low-order bits reserved for the resource slot in a packed handle.
pub const GPU_HANDLE_SLOT_BITS: u32 = 20;
/// Mask extracting a packed handle's resource slot.
pub const GPU_HANDLE_SLOT_MASK: u32 = (1 << GPU_HANDLE_SLOT_BITS) - 1;
/// Largest generation representable in a packed 32-bit handle.
pub const GPU_HANDLE_MAX_GENERATION: u32 = (1 << (32 - GPU_HANDLE_SLOT_BITS)) - 1;
/// Maximum accepted submissions per guest update.
pub const MAX_GPU_SUBMITS_PER_TICK: u32 = 8;
/// Maximum resource upload bytes per guest update.
pub const MAX_GPU_UPLOAD_BYTES_PER_TICK: usize = 16 * 1024 * 1024;
/// Maximum batches awaiting host execution.
pub const MAX_GPU_QUEUED_BATCHES: usize = 4;
/// Maximum events awaiting guest consumption.
pub const MAX_GPU_QUEUED_EVENTS: usize = 256;
/// Maximum live buffer resources.
pub const MAX_GPU_BUFFERS: usize = 4_096;
/// Maximum bytes allocated to one buffer.
pub const MAX_GPU_BUFFER_BYTES: usize = 16 * 1024 * 1024;
/// Maximum bytes allocated across all live buffers.
pub const MAX_GPU_TOTAL_BUFFER_BYTES: usize = 64 * 1024 * 1024;
/// Maximum live texture resources.
pub const MAX_GPU_TEXTURES: usize = 512;
/// Maximum bytes allocated across all live textures.
pub const MAX_GPU_TOTAL_TEXTURE_BYTES: usize = 256 * 1024 * 1024;
/// Maximum width or height of a 2D texture, in texels.
pub const MAX_GPU_TEXTURE_DIMENSION_2D: u32 = 4_096;
/// Maximum samples per texture texel.
pub const MAX_GPU_TEXTURE_SAMPLE_COUNT: u32 = 1;
/// Maximum mip levels in one texture.
pub const MAX_GPU_TEXTURE_MIP_LEVELS: u32 = 13;
/// Maximum live texture view resources.
pub const MAX_GPU_TEXTURE_VIEWS: usize = 1_024;
/// Maximum live sampler resources.
pub const MAX_GPU_SAMPLERS: usize = 128;
/// Maximum live shader module resources.
pub const MAX_GPU_SHADER_MODULES: usize = 128;
/// Maximum UTF-8 WGSL source bytes in one shader module.
pub const MAX_GPU_WGSL_BYTES: usize = 1024 * 1024;
/// Maximum shader and pipeline compilation operations per batch.
pub const MAX_GPU_COMPILATIONS: usize = 32;
/// Maximum live bind group layouts.
pub const MAX_GPU_BIND_GROUP_LAYOUTS: usize = 128;
/// Maximum live pipeline layouts.
pub const MAX_GPU_PIPELINE_LAYOUTS: usize = 64;
/// Maximum live bind groups.
pub const MAX_GPU_BIND_GROUPS: usize = 512;
/// Maximum live render pipelines.
pub const MAX_GPU_RENDER_PIPELINES: usize = 256;
/// Maximum bind group slots in one pipeline layout.
pub const MAX_GPU_BIND_GROUPS_PER_PIPELINE: usize = 4;
/// Maximum bindings in one bind group.
pub const MAX_GPU_BINDINGS_PER_GROUP: usize = 16;
/// Maximum vertex buffer slots in one pipeline.
pub const MAX_GPU_VERTEX_BUFFERS: usize = 8;
/// Maximum vertex attributes in one pipeline.
pub const MAX_GPU_VERTEX_ATTRIBUTES: usize = 16;
/// Maximum color attachment slots in one render pass.
pub const MAX_GPU_COLOR_ATTACHMENTS: usize = 4;
/// Maximum render passes encoded in one batch.
pub const MAX_GPU_RENDER_PASSES_PER_BATCH: usize = 16;
/// Maximum draw commands encoded in one batch.
pub const MAX_GPU_DRAWS_PER_BATCH: usize = 8_192;
/// Maximum compute passes encoded in one batch.
pub const MAX_GPU_COMPUTE_PASSES_PER_BATCH: usize = 64;
/// Maximum compute dispatch commands encoded in one batch.
pub const MAX_GPU_DISPATCHES_PER_BATCH: usize = 8_192;

/// Submission status: the batch was accepted.
pub const GPU_SUBMIT_ACCEPTED: i32 = 0;
/// Submission status: retry after the host drains its queue.
pub const GPU_SUBMIT_BUSY: i32 = 1;
/// Submission failure: guest memory range is inaccessible.
pub const GPU_ERROR_INVALID_GUEST_RANGE: i32 = -1;
/// Submission failure: batch encoding is invalid.
pub const GPU_ERROR_MALFORMED_BATCH: i32 = -2;
/// Submission failure: a resource or per-update quota was exceeded.
pub const GPU_ERROR_QUOTA_EXCEEDED: i32 = -3;
/// Submission failure: resource handle is unknown or stale.
pub const GPU_ERROR_INVALID_HANDLE: i32 = -4;
/// Submission failure: command is incompatible with current GPU state.
pub const GPU_ERROR_INVALID_STATE: i32 = -5;
/// Submission failure: runtime has stopped.
pub const GPU_ERROR_STOPPED: i32 = -6;

/// Batch rejection reason: surface generation no longer matches.
pub const GPU_BATCH_ERROR_STALE_SURFACE: u32 = 4;

/// Keys for host-advertised limits; values are unsigned integers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum GpuCapabilityKey {
    /// Maximum texture width or height, in texels.
    MaxTextureDimension2d = 1,
    /// Maximum allocation for one buffer, in bytes.
    MaxBufferSize = 2,
    /// Maximum binding entries in a bind group.
    MaxBindingsPerBindGroup = 3,
    /// Maximum bind group slots in a pipeline layout.
    MaxBindGroups = 4,
    /// Maximum vertex buffer slots in a pipeline.
    MaxVertexBuffers = 5,
    /// Maximum vertex attributes in a pipeline.
    MaxVertexAttributes = 6,
    /// Maximum color attachments in a render pass.
    MaxColorAttachments = 7,
    /// Maximum aggregate texture allocation, in bytes.
    MaxTextureBytes = 8,
    /// Maximum aggregate buffer allocation, in bytes.
    MaxBufferBytes = 9,
    /// Maximum draw commands per batch.
    MaxDrawsPerBatch = 10,
    /// Maximum encoded batch length, in bytes.
    MaxBatchBytes = 11,
    /// Maximum upload payload bytes per guest update.
    MaxUploadBytesPerTick = 12,
    /// Maximum storage buffer binding range, in bytes.
    MaxStorageBufferBindingSize = 13,
    /// Maximum storage buffer bindings visible to one shader stage.
    MaxStorageBuffersPerShaderStage = 14,
    /// Maximum shared storage per compute workgroup, in bytes.
    MaxComputeWorkgroupStorageSize = 15,
    /// Maximum total invocations in one compute workgroup.
    MaxComputeInvocationsPerWorkgroup = 16,
    /// Maximum local workgroup size along the X axis.
    MaxComputeWorkgroupSizeX = 17,
    /// Maximum local workgroup size along the Y axis.
    MaxComputeWorkgroupSizeY = 18,
    /// Maximum local workgroup size along the Z axis.
    MaxComputeWorkgroupSizeZ = 19,
    /// Maximum dispatched workgroups along each axis.
    MaxComputeWorkgroupsPerDimension = 20,
    /// Maximum dispatch commands per batch.
    MaxDispatchesPerBatch = 21,
}

/// Discriminants for asynchronous host-to-guest GPU events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum GpuEventType {
    /// The referenced batch was not executed.
    BatchRejected = 1,
    /// Shader compilation produced a diagnostic.
    ShaderDiagnostic = 2,
    /// Resource creation failed.
    ResourceFailed = 3,
    /// A device error was not associated with a specific request.
    UncapturedError = 4,
    /// The referenced submission finished executing.
    SubmissionComplete = 5,
    /// Surface dimensions or generation changed.
    SurfaceChanged = 6,
    /// The GPU device can no longer accept work.
    DeviceLost = 7,
    /// A replacement GPU device is ready for resource recreation.
    DeviceRestored = 8,
}

/// Command discriminants in the GPU batch wire protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum GpuOpcode {
    /// Allocate a buffer with declared size and usage.
    CreateBuffer = 1,
    /// Upload inline bytes into a buffer range.
    WriteBuffer = 2,
    /// Allocate a texture with declared format and dimensions.
    CreateTexture = 3,
    /// Upload inline texels into a texture region.
    WriteTexture = 4,
    /// Create texture sampling state.
    CreateSampler = 5,
    /// Compile inline UTF-8 WGSL source.
    CreateShaderWgsl = 6,
    /// Declare binding types and shader visibility.
    CreateBindGroupLayout = 7,
    /// Combine bind group layouts into pipeline state.
    CreatePipelineLayout = 8,
    /// Associate resources with binding slots.
    CreateBindGroup = 9,
    /// Create a raster pipeline from shader and fixed-function state.
    CreateRenderPipeline = 10,
    /// Release the resource named by a packed handle.
    DestroyResource = 11,
    /// Begin a render pass with attachment load/store operations.
    BeginRenderPass = 12,
    /// Select the active render pipeline.
    SetPipeline = 13,
    /// Bind a buffer range to a vertex input slot.
    SetVertexBuffer = 14,
    /// Bind a buffer range and format for indexed draws.
    SetIndexBuffer = 15,
    /// Bind render resources, including dynamic offsets.
    SetBindGroup = 16,
    /// Set the raster viewport and depth range.
    SetViewport = 17,
    /// Set the pixel rectangle permitted to receive fragments.
    SetScissorRect = 18,
    /// Issue a non-indexed, optionally instanced draw.
    Draw = 19,
    /// Issue an indexed, optionally instanced draw.
    DrawIndexed = 20,
    /// End the active render pass.
    EndRenderPass = 21,
    /// Copy bytes between buffer ranges.
    CopyBufferToBuffer = 22,
    /// Select a texture's mip range and aspect for binding.
    CreateTextureView = 23,
    /// Create a compute pipeline from a shader entry point.
    CreateComputePipeline = 24,
    /// Begin a compute pass.
    BeginComputePass = 25,
    /// Select the active compute pipeline.
    SetComputePipeline = 26,
    /// Bind compute resources, including dynamic offsets.
    SetComputeBindGroup = 27,
    /// Dispatch workgroups along three axes.
    DispatchWorkgroups = 28,
    /// End the active compute pass.
    EndComputePass = 29,
}

impl TryFrom<u16> for GpuOpcode {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::CreateBuffer),
            2 => Ok(Self::WriteBuffer),
            3 => Ok(Self::CreateTexture),
            4 => Ok(Self::WriteTexture),
            5 => Ok(Self::CreateSampler),
            6 => Ok(Self::CreateShaderWgsl),
            7 => Ok(Self::CreateBindGroupLayout),
            8 => Ok(Self::CreatePipelineLayout),
            9 => Ok(Self::CreateBindGroup),
            10 => Ok(Self::CreateRenderPipeline),
            11 => Ok(Self::DestroyResource),
            12 => Ok(Self::BeginRenderPass),
            13 => Ok(Self::SetPipeline),
            14 => Ok(Self::SetVertexBuffer),
            15 => Ok(Self::SetIndexBuffer),
            16 => Ok(Self::SetBindGroup),
            17 => Ok(Self::SetViewport),
            18 => Ok(Self::SetScissorRect),
            19 => Ok(Self::Draw),
            20 => Ok(Self::DrawIndexed),
            21 => Ok(Self::EndRenderPass),
            22 => Ok(Self::CopyBufferToBuffer),
            23 => Ok(Self::CreateTextureView),
            24 => Ok(Self::CreateComputePipeline),
            25 => Ok(Self::BeginComputePass),
            26 => Ok(Self::SetComputePipeline),
            27 => Ok(Self::SetComputeBindGroup),
            28 => Ok(Self::DispatchWorkgroups),
            29 => Ok(Self::EndComputePass),
            _ => Err(()),
        }
    }
}

/// Texture formats represented by stable wire discriminants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum GpuTextureFormat {
    /// Four unsigned normalized 8-bit channels in RGBA order.
    Rgba8Unorm = 1,
    /// RGBA8 with sRGB color transfer.
    Rgba8UnormSrgb = 2,
    /// Four unsigned normalized 8-bit channels in BGRA order.
    Bgra8Unorm = 3,
    /// BGRA8 with sRGB color transfer.
    Bgra8UnormSrgb = 4,
    /// Depth format with at least 24 bits of precision.
    Depth24Plus = 5,
    /// One 32-bit floating-point depth component.
    Depth32Float = 6,
    /// One unsigned normalized 8-bit red component.
    R8Unorm = 7,
}

/// Vertex attribute storage formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum GpuVertexFormat {
    /// One 32-bit floating-point component.
    Float32 = 1,
    /// Two 32-bit floating-point components.
    Float32x2 = 2,
    /// Three 32-bit floating-point components.
    Float32x3 = 3,
    /// Four 32-bit floating-point components.
    Float32x4 = 4,
    /// One unsigned 32-bit integer component.
    Uint32 = 5,
    /// Two unsigned 32-bit integer components.
    Uint32x2 = 6,
    /// Four unsigned 32-bit integer components.
    Uint32x4 = 7,
    /// Two unsigned 8-bit components normalized to [0, 1].
    Unorm8x2 = 8,
    /// Four unsigned 8-bit components normalized to [0, 1].
    Unorm8x4 = 9,
    /// Two signed 8-bit components normalized to [-1, 1].
    Snorm8x2 = 10,
    /// Four signed 8-bit components normalized to [-1, 1].
    Snorm8x4 = 11,
}

/// Element width of an index buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuIndexFormat {
    /// Unsigned 16-bit indices.
    Uint16 = 1,
    /// Unsigned 32-bit indices.
    Uint32 = 2,
}

/// Sampling behavior for texture coordinates outside [0, 1].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuAddressMode {
    /// Use the nearest edge texel.
    ClampToEdge = 1,
    /// Wrap coordinates modulo one.
    Repeat = 2,
    /// Wrap coordinates with alternating reflection.
    MirrorRepeat = 3,
}

/// Texel selection when sampling between texel centers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuFilterMode {
    /// Select the nearest texel.
    Nearest = 1,
    /// Interpolate neighboring texels.
    Linear = 2,
}

/// Comparison of the incoming value against the stored/reference value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuCompareFunction {
    /// No comparison passes.
    Never = 1,
    /// Pass when incoming is less than stored.
    Less = 2,
    /// Pass when incoming equals stored.
    Equal = 3,
    /// Pass when incoming is less than or equal to stored.
    LessEqual = 4,
    /// Pass when incoming is greater than stored.
    Greater = 5,
    /// Pass when incoming differs from stored.
    NotEqual = 6,
    /// Pass when incoming is greater than or equal to stored.
    GreaterEqual = 7,
    /// Every comparison passes.
    Always = 8,
}

/// Operation combining factored source and destination colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuBlendOperation {
    /// Add source and destination.
    Add = 1,
    /// Subtract destination from source.
    Subtract = 2,
    /// Subtract source from destination.
    ReverseSubtract = 3,
    /// Select the component-wise minimum.
    Min = 4,
    /// Select the component-wise maximum.
    Max = 5,
}

/// Multipliers used in color or alpha blending.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuBlendFactor {
    /// Multiply by zero.
    Zero = 1,
    /// Multiply by one.
    One = 2,
    /// Use source color components.
    Src = 3,
    /// Use one minus source color components.
    OneMinusSrc = 4,
    /// Use source alpha for every component.
    SrcAlpha = 5,
    /// Use one minus source alpha.
    OneMinusSrcAlpha = 6,
    /// Use destination color components.
    Dst = 7,
    /// Use one minus destination color components.
    OneMinusDst = 8,
    /// Use destination alpha for every component.
    DstAlpha = 9,
    /// Use one minus destination alpha.
    OneMinusDstAlpha = 10,
    /// Use min(source alpha, one minus destination alpha) for color.
    SrcAlphaSaturated = 11,
    /// Use the constant blend color.
    Constant = 12,
    /// Use one minus the constant blend color.
    OneMinusConstant = 13,
}

/// Assembly of vertex sequences into raster primitives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuPrimitiveTopology {
    /// Each vertex forms one point.
    PointList = 1,
    /// Each pair of vertices forms one independent line.
    LineList = 2,
    /// Consecutive vertices form connected lines.
    LineStrip = 3,
    /// Each three vertices form one independent triangle.
    TriangleList = 4,
    /// Each vertex after the first two extends a triangle strip.
    TriangleStrip = 5,
}

/// Winding identifying a triangle's front side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuFrontFace {
    /// Counterclockwise vertices face front.
    Ccw = 1,
    /// Clockwise vertices face front.
    Cw = 2,
}

/// Triangle side discarded during rasterization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuCullMode {
    /// Discard front-facing triangles.
    Front = 1,
    /// Discard back-facing triangles.
    Back = 2,
}

/// Resource types permitted in a bind group layout entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum GpuBindingKind {
    /// Read-only uniform buffer binding.
    UniformBuffer = 1,
    /// Texture sampler binding.
    Sampler = 2,
    /// Sampled texture view binding.
    Texture = 3,
    /// Read-only storage buffer binding.
    StorageBuffer = 4,
    /// Read/write storage buffer binding.
    StorageBufferReadWrite = 5,
}

/// Sampler restrictions declared by a bind group layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GpuSamplerBindingType {
    /// Allows filtering texture samples.
    Filtering = 1,
    /// Only non-filtering texture samples.
    NonFiltering = 2,
    /// Compares sampled depth with a reference value.
    Comparison = 3,
}

/// Shader sample type required by a texture binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GpuTextureSampleType {
    /// Floating-point samples supporting filtering.
    FloatFilterable = 1,
    /// Floating-point samples without filtering.
    FloatUnfilterable = 2,
    /// Depth comparison samples.
    Depth = 3,
    /// Signed integer samples.
    Sint = 4,
    /// Unsigned integer samples.
    Uint = 5,
}

/// Rate at which a vertex buffer advances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuVertexStepMode {
    /// Advance once per vertex.
    Vertex = 1,
    /// Advance once per instance.
    Instance = 2,
}

/// Texture dimensionality supported by this protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuTextureDimension {
    /// Two-dimensional texture.
    D2 = 1,
}

/// Dimensionality exposed by a texture view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GpuTextureViewDimension {
    /// Two-dimensional texture view.
    D2 = 1,
}

/// Subset of a texture's components exposed by a view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GpuTextureAspect {
    /// All components of the texture format.
    All = 1,
    /// Only the depth component.
    DepthOnly = 2,
}

/// Buffer usage bit permitting copy-source operations.
pub const GPU_BUFFER_USAGE_COPY_SRC: u32 = 4;
/// Buffer usage bit permitting copy/upload destinations.
pub const GPU_BUFFER_USAGE_COPY_DST: u32 = 8;
/// Buffer usage bit permitting index input.
pub const GPU_BUFFER_USAGE_INDEX: u32 = 16;
/// Buffer usage bit permitting vertex input.
pub const GPU_BUFFER_USAGE_VERTEX: u32 = 32;
/// Buffer usage bit permitting uniform bindings.
pub const GPU_BUFFER_USAGE_UNIFORM: u32 = 64;
/// Buffer usage bit permitting storage bindings.
pub const GPU_BUFFER_USAGE_STORAGE: u32 = 128;
/// Texture usage bit permitting copy-source operations.
pub const GPU_TEXTURE_USAGE_COPY_SRC: u32 = 1;
/// Texture usage bit permitting copy/upload destinations.
pub const GPU_TEXTURE_USAGE_COPY_DST: u32 = 2;
/// Texture usage bit permitting shader sampling.
pub const GPU_TEXTURE_USAGE_TEXTURE_BINDING: u32 = 4;
/// Texture usage bit permitting render attachment use.
pub const GPU_TEXTURE_USAGE_RENDER_ATTACHMENT: u32 = 16;
/// Binding visibility bit for vertex shaders.
pub const GPU_SHADER_STAGE_VERTEX: u32 = 1;
/// Binding visibility bit for fragment shaders.
pub const GPU_SHADER_STAGE_FRAGMENT: u32 = 2;
/// Color write mask bit for the red channel.
pub const GPU_COLOR_WRITE_RED: u16 = 1;
/// Color write mask bit for the green channel.
pub const GPU_COLOR_WRITE_GREEN: u16 = 2;
/// Color write mask bit for the blue channel.
pub const GPU_COLOR_WRITE_BLUE: u16 = 4;
/// Color write mask bit for the alpha channel.
pub const GPU_COLOR_WRITE_ALPHA: u16 = 8;
/// Render pass bit loading existing color rather than clearing.
pub const GPU_RENDER_PASS_COLOR_LOAD: u32 = 1;
/// Render pass bit preserving color after the pass.
pub const GPU_RENDER_PASS_COLOR_STORE: u32 = 2;
/// Render pass bit loading existing depth rather than clearing.
pub const GPU_RENDER_PASS_DEPTH_LOAD: u32 = 4;
/// Render pass bit preserving depth after the pass.
pub const GPU_RENDER_PASS_DEPTH_STORE: u32 = 8;
/// Binding flag requiring a dynamic buffer offset at bind time.
pub const GPU_BINDING_HAS_DYNAMIC_OFFSET: u16 = 1;
/// Pipeline flag enabling depth-buffer writes.
pub const GPU_PIPELINE_DEPTH_WRITE: u16 = 1;

/// Structural batch validation failure, before device execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuWireError {
    /// Encoded batch exceeds the protocol byte limit.
    BatchTooLarge {
        /// Received byte length.
        actual: usize,
    },
    /// Input cannot contain the fixed batch header.
    TruncatedBatchHeader {
        /// Received byte length.
        actual: usize,
    },
    /// Batch discriminator is not `EPG1`.
    InvalidMagic,
    /// Batch version is not supported.
    UnsupportedVersion {
        /// Version found in the header.
        version: u16,
    },
    /// Reserved batch flag bits were nonzero.
    ReservedBatchFlags {
        /// Raw flag bits.
        flags: u16,
    },
    /// Header byte length differs from the supplied slice.
    BatchLengthMismatch {
        /// Byte length encoded in the header.
        declared: usize,
        /// Received byte length.
        actual: usize,
    },
    /// Sequence zero is reserved.
    EmptySequence,
    /// Declared command count exceeds the protocol limit.
    TooManyCommands {
        /// Declared number of commands.
        count: u32,
    },
    /// A command header extends beyond the batch.
    TruncatedCommandHeader {
        /// Zero-based command index.
        index: u32,
    },
    /// A command length is too short, unaligned, or out of bounds.
    InvalidCommandLength {
        /// Zero-based command index.
        index: u32,
        /// Declared command length in bytes, including its header.
        length: usize,
    },
    /// Reserved command flag bits were nonzero.
    ReservedCommandFlags {
        /// Zero-based command index.
        index: u32,
        /// Raw flag bits.
        flags: u16,
    },
    /// Command discriminant is not supported.
    UnknownOpcode {
        /// Zero-based command index.
        index: u32,
        /// Raw command discriminant.
        opcode: u16,
    },
    /// Payload size does not match the command layout.
    InvalidPayloadLength {
        /// Zero-based command index.
        index: u32,
        /// Command whose payload was rejected.
        opcode: GpuOpcode,
        /// Required payload size in bytes.
        expected: usize,
        /// Supplied payload size in bytes.
        actual: usize,
    },
    /// Shader source is not valid UTF-8.
    InvalidWgslUtf8 {
        /// Zero-based command index.
        index: u32,
    },
    /// Alignment padding contains nonzero bytes.
    NonZeroPadding {
        /// Zero-based command index.
        index: u32,
    },
    /// Bytes remain after the declared commands.
    TrailingCommandBytes {
        /// Number of trailing bytes.
        actual: usize,
    },
    /// A payload size calculation overflowed.
    IntegerOverflow {
        /// Zero-based command index.
        index: u32,
    },
}

impl fmt::Display for GpuWireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::BatchTooLarge { actual } => {
                write!(formatter, "GPU batch is {actual} bytes; maximum is {MAX_GPU_BATCH_BYTES}")
            }
            Self::TruncatedBatchHeader { actual } => write!(
                formatter,
                "GPU batch header is truncated: got {actual} bytes, need {GPU_BATCH_HEADER_BYTES}"
            ),
            Self::InvalidMagic => formatter.write_str("GPU batch has invalid magic"),
            Self::UnsupportedVersion { version } => {
                write!(formatter, "unsupported GPU wire version {version}")
            }
            Self::ReservedBatchFlags { flags } => {
                write!(formatter, "GPU batch has reserved flags 0x{flags:04x}")
            }
            Self::BatchLengthMismatch { declared, actual } => write!(
                formatter,
                "GPU batch declares {declared} bytes but contains {actual}"
            ),
            Self::EmptySequence => formatter.write_str("GPU batch sequence must be nonzero"),
            Self::TooManyCommands { count } => write!(
                formatter,
                "GPU batch declares {count} commands; maximum is {MAX_GPU_COMMANDS}"
            ),
            Self::TruncatedCommandHeader { index } => {
                write!(formatter, "GPU command {index} header is truncated")
            }
            Self::InvalidCommandLength { index, length } => write!(
                formatter,
                "GPU command {index} has invalid aligned length {length}"
            ),
            Self::ReservedCommandFlags { index, flags } => write!(
                formatter,
                "GPU command {index} has reserved flags 0x{flags:04x}"
            ),
            Self::UnknownOpcode { index, opcode } => {
                write!(formatter, "GPU command {index} uses unknown opcode {opcode}")
            }
            Self::InvalidPayloadLength {
                index,
                opcode,
                expected,
                actual,
            } => write!(
                formatter,
                "GPU command {index} ({opcode:?}) needs {expected} payload bytes but contains {actual}"
            ),
            Self::InvalidWgslUtf8 { index } => {
                write!(formatter, "GPU command {index} contains non-UTF-8 WGSL")
            }
            Self::NonZeroPadding { index } => {
                write!(formatter, "GPU command {index} has nonzero padding")
            }
            Self::TrailingCommandBytes { actual } => {
                write!(formatter, "GPU batch has {actual} trailing command bytes")
            }
            Self::IntegerOverflow { index } => {
                write!(formatter, "GPU command {index} payload length overflows")
            }
        }
    }
}

impl core::error::Error for GpuWireError {}

/// Borrowed batch with structurally validated command payloads.
#[derive(Clone, Copy, Debug)]
pub struct GpuBatch<'a> {
    sequence: u64,
    command_count: u32,
    command_bytes: &'a [u8],
}

impl<'a> GpuBatch<'a> {
    /// Nonzero guest-assigned sequence used to correlate events.
    pub fn sequence(self) -> u64 {
        self.sequence
    }

    /// Number of encoded commands.
    pub fn command_count(self) -> u32 {
        self.command_count
    }

    /// Iterate validated commands in submission order without copying.
    pub fn commands(self) -> GpuCommands<'a> {
        GpuCommands {
            bytes: self.command_bytes,
            remaining: self.command_count,
        }
    }
}

/// One command borrowed from a validated batch.
#[derive(Clone, Copy, Debug)]
pub struct GpuCommand<'a> {
    /// Command layout identifying the payload.
    pub opcode: GpuOpcode,
    /// Encoded payload bytes, excluding the command header.
    pub payload: &'a [u8],
}

/// Exact-size iterator over commands in a validated batch.
pub struct GpuCommands<'a> {
    bytes: &'a [u8],
    remaining: u32,
}

impl<'a> Iterator for GpuCommands<'a> {
    type Item = GpuCommand<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let opcode = GpuOpcode::try_from(u16_at(self.bytes, 0)).ok()?;
        let command_bytes = u32_at(self.bytes, 4) as usize;
        let payload = &self.bytes[GPU_COMMAND_HEADER_BYTES..command_bytes];
        self.bytes = &self.bytes[command_bytes..];
        self.remaining -= 1;
        Some(GpuCommand { opcode, payload })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for GpuCommands<'_> {}

/// Validate framing, payload sizes, UTF-8, and padding without allocating.
///
/// Resource validity and GPU execution state must still be checked by the host.
pub fn decode_gpu_batch(bytes: &[u8]) -> Result<GpuBatch<'_>, GpuWireError> {
    if bytes.len() > MAX_GPU_BATCH_BYTES {
        return Err(GpuWireError::BatchTooLarge {
            actual: bytes.len(),
        });
    }
    if bytes.len() < GPU_BATCH_HEADER_BYTES {
        return Err(GpuWireError::TruncatedBatchHeader {
            actual: bytes.len(),
        });
    }
    if bytes[..4] != GPU_WIRE_MAGIC {
        return Err(GpuWireError::InvalidMagic);
    }
    let version = u16_at(bytes, 4);
    if version != GPU_WIRE_VERSION {
        return Err(GpuWireError::UnsupportedVersion { version });
    }
    let flags = u16_at(bytes, 6);
    if flags != 0 {
        return Err(GpuWireError::ReservedBatchFlags { flags });
    }
    let declared_length = u32_at(bytes, 8) as usize;
    if declared_length != bytes.len() {
        return Err(GpuWireError::BatchLengthMismatch {
            declared: declared_length,
            actual: bytes.len(),
        });
    }
    let command_count = u32_at(bytes, 12);
    if command_count > MAX_GPU_COMMANDS {
        return Err(GpuWireError::TooManyCommands {
            count: command_count,
        });
    }
    let sequence = u64_at(bytes, 16);
    if sequence == 0 {
        return Err(GpuWireError::EmptySequence);
    }

    let mut command_bytes = &bytes[GPU_BATCH_HEADER_BYTES..];
    for index in 0..command_count {
        if command_bytes.len() < GPU_COMMAND_HEADER_BYTES {
            return Err(GpuWireError::TruncatedCommandHeader { index });
        }
        let opcode_value = u16_at(command_bytes, 0);
        let command_flags = u16_at(command_bytes, 2);
        if command_flags != 0 {
            return Err(GpuWireError::ReservedCommandFlags {
                index,
                flags: command_flags,
            });
        }
        let command_length = u32_at(command_bytes, 4) as usize;
        if command_length < GPU_COMMAND_HEADER_BYTES
            || !command_length.is_multiple_of(4)
            || command_length > command_bytes.len()
        {
            return Err(GpuWireError::InvalidCommandLength {
                index,
                length: command_length,
            });
        }
        let opcode =
            GpuOpcode::try_from(opcode_value).map_err(|()| GpuWireError::UnknownOpcode {
                index,
                opcode: opcode_value,
            })?;
        validate_payload(
            index,
            opcode,
            &command_bytes[GPU_COMMAND_HEADER_BYTES..command_length],
        )?;
        command_bytes = &command_bytes[command_length..];
    }
    if !command_bytes.is_empty() {
        return Err(GpuWireError::TrailingCommandBytes {
            actual: command_bytes.len(),
        });
    }

    Ok(GpuBatch {
        sequence,
        command_count,
        command_bytes: &bytes[GPU_BATCH_HEADER_BYTES..],
    })
}

fn validate_payload(index: u32, opcode: GpuOpcode, payload: &[u8]) -> Result<(), GpuWireError> {
    match opcode {
        GpuOpcode::CreateBuffer => exact_payload(index, opcode, payload, 16),
        GpuOpcode::WriteBuffer => inline_payload(index, opcode, payload, 24, 16, false),
        GpuOpcode::CreateTexture => exact_payload(index, opcode, payload, 24),
        GpuOpcode::WriteTexture => inline_payload(index, opcode, payload, 44, 40, false),
        GpuOpcode::CreateSampler => exact_payload(index, opcode, payload, 24),
        GpuOpcode::CreateShaderWgsl => inline_payload(index, opcode, payload, 8, 4, true),
        GpuOpcode::CreateBindGroupLayout => counted_payload(index, opcode, payload, 8, 4, 32),
        GpuOpcode::CreatePipelineLayout => counted_payload(index, opcode, payload, 8, 4, 4),
        GpuOpcode::CreateBindGroup => counted_payload(index, opcode, payload, 12, 8, 32),
        GpuOpcode::CreateRenderPipeline => pipeline_payload(index, opcode, payload),
        GpuOpcode::DestroyResource | GpuOpcode::SetPipeline => {
            exact_payload(index, opcode, payload, 4)
        }
        GpuOpcode::BeginRenderPass => exact_payload(index, opcode, payload, 36),
        GpuOpcode::CopyBufferToBuffer => exact_payload(index, opcode, payload, 32),
        GpuOpcode::SetVertexBuffer | GpuOpcode::SetIndexBuffer | GpuOpcode::SetViewport => {
            exact_payload(index, opcode, payload, 24)
        }
        GpuOpcode::SetBindGroup => counted_payload(index, opcode, payload, 12, 8, 4),
        GpuOpcode::SetScissorRect | GpuOpcode::Draw => exact_payload(index, opcode, payload, 16),
        GpuOpcode::DrawIndexed => exact_payload(index, opcode, payload, 20),
        GpuOpcode::EndRenderPass => exact_payload(index, opcode, payload, 0),
        GpuOpcode::CreateTextureView => exact_payload(index, opcode, payload, 20),
        GpuOpcode::CreateComputePipeline => exact_payload(index, opcode, payload, 16),
        GpuOpcode::BeginComputePass | GpuOpcode::EndComputePass => {
            exact_payload(index, opcode, payload, 0)
        }
        GpuOpcode::SetComputePipeline => exact_payload(index, opcode, payload, 4),
        GpuOpcode::SetComputeBindGroup => counted_payload(index, opcode, payload, 12, 8, 4),
        GpuOpcode::DispatchWorkgroups => exact_payload(index, opcode, payload, 12),
    }
}

fn exact_payload(
    index: u32,
    opcode: GpuOpcode,
    payload: &[u8],
    expected: usize,
) -> Result<(), GpuWireError> {
    if payload.len() != expected {
        return Err(GpuWireError::InvalidPayloadLength {
            index,
            opcode,
            expected,
            actual: payload.len(),
        });
    }
    Ok(())
}

fn inline_payload(
    index: u32,
    opcode: GpuOpcode,
    payload: &[u8],
    header_bytes: usize,
    length_offset: usize,
    utf8: bool,
) -> Result<(), GpuWireError> {
    if payload.len() < header_bytes {
        return Err(GpuWireError::InvalidPayloadLength {
            index,
            opcode,
            expected: header_bytes,
            actual: payload.len(),
        });
    }
    let inline_bytes = u32_at(payload, length_offset) as usize;
    let unpadded = header_bytes
        .checked_add(inline_bytes)
        .ok_or(GpuWireError::IntegerOverflow { index })?;
    let expected = align4(unpadded).ok_or(GpuWireError::IntegerOverflow { index })?;
    if payload.len() != expected {
        return Err(GpuWireError::InvalidPayloadLength {
            index,
            opcode,
            expected,
            actual: payload.len(),
        });
    }
    if utf8 && core::str::from_utf8(&payload[header_bytes..unpadded]).is_err() {
        return Err(GpuWireError::InvalidWgslUtf8 { index });
    }
    if payload[unpadded..].iter().any(|byte| *byte != 0) {
        return Err(GpuWireError::NonZeroPadding { index });
    }
    Ok(())
}

fn counted_payload(
    index: u32,
    opcode: GpuOpcode,
    payload: &[u8],
    header_bytes: usize,
    count_offset: usize,
    stride: usize,
) -> Result<(), GpuWireError> {
    if payload.len() < header_bytes {
        return Err(GpuWireError::InvalidPayloadLength {
            index,
            opcode,
            expected: header_bytes,
            actual: payload.len(),
        });
    }
    let count = u32_at(payload, count_offset) as usize;
    let expected = count
        .checked_mul(stride)
        .and_then(|entries| header_bytes.checked_add(entries))
        .ok_or(GpuWireError::IntegerOverflow { index })?;
    exact_payload(index, opcode, payload, expected)
}

fn pipeline_payload(index: u32, opcode: GpuOpcode, payload: &[u8]) -> Result<(), GpuWireError> {
    const HEADER_BYTES: usize = 40;
    if payload.len() < HEADER_BYTES {
        return Err(GpuWireError::InvalidPayloadLength {
            index,
            opcode,
            expected: HEADER_BYTES,
            actual: payload.len(),
        });
    }
    let vertex_layouts = u16_at(payload, 12) as usize;
    let vertex_attributes = u16_at(payload, 14) as usize;
    let color_targets = u16_at(payload, 16) as usize;
    let expected = vertex_layouts
        .checked_mul(16)
        .and_then(|value| {
            vertex_attributes
                .checked_mul(16)
                .and_then(|next| value.checked_add(next))
        })
        .and_then(|value| {
            color_targets
                .checked_mul(16)
                .and_then(|next| value.checked_add(next))
        })
        .and_then(|arrays| HEADER_BYTES.checked_add(arrays))
        .ok_or(GpuWireError::IntegerOverflow { index })?;
    exact_payload(index, opcode, payload, expected)
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|value| value & !3)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("validated field offset"),
    )
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated field offset"),
    )
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("validated field offset"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    use std::vec;
    use std::vec::Vec;

    const DESTROY_FIXTURE: [u8; 36] = [
        0x45, 0x50, 0x47, 0x31, 0x01, 0x00, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x00, 0x0c, 0x00,
        0x00, 0x00, 0x2a, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn decodes_golden_destroy_fixture_without_allocating() {
        let batch = decode_gpu_batch(&DESTROY_FIXTURE).expect("valid fixture");
        assert_eq!(batch.sequence(), 7);
        assert_eq!(batch.command_count(), 1);
        let commands = batch.commands().collect::<Vec<_>>();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].opcode, GpuOpcode::DestroyResource);
        assert_eq!(commands[0].payload, 42u32.to_le_bytes());
    }

    #[test]
    fn accepts_padded_utf8_shader_source() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&9u32.to_le_bytes());
        payload.extend_from_slice(&3u32.to_le_bytes());
        payload.extend_from_slice(b"abc");
        payload.push(0);
        let batch = single_command(GpuOpcode::CreateShaderWgsl, &payload);
        assert!(decode_gpu_batch(&batch).is_ok());
    }

    #[test]
    fn rejects_unknown_opcode() {
        let mut batch = DESTROY_FIXTURE;
        batch[24..26].copy_from_slice(&99u16.to_le_bytes());
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::UnknownOpcode {
                index: 0,
                opcode: 99,
            }
        );
    }

    #[test]
    fn rejects_declared_batch_length_mismatch() {
        let mut batch = DESTROY_FIXTURE;
        batch[8..12].copy_from_slice(&35u32.to_le_bytes());
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::BatchLengthMismatch {
                declared: 35,
                actual: 36,
            }
        );
    }

    #[test]
    fn rejects_extra_well_formed_command() {
        let mut batch = DESTROY_FIXTURE.to_vec();
        batch.extend_from_slice(&DESTROY_FIXTURE[24..]);
        let batch_length = batch.len() as u32;
        batch[8..12].copy_from_slice(&batch_length.to_le_bytes());
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::TrailingCommandBytes { actual: 12 }
        );
    }

    #[test]
    fn rejects_nonzero_inline_padding() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&9u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(b"x");
        payload.extend_from_slice(&[0, 1, 0]);
        let batch = single_command(GpuOpcode::CreateShaderWgsl, &payload);
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::NonZeroPadding { index: 0 }
        );
    }

    #[test]
    fn rejects_non_utf8_shader_source() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&9u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[0xff, 0, 0, 0]);
        let batch = single_command(GpuOpcode::CreateShaderWgsl, &payload);
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::InvalidWgslUtf8 { index: 0 }
        );
    }

    #[test]
    fn rejects_variable_payload_count_mismatch() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&7u32.to_le_bytes());
        let batch = single_command(GpuOpcode::CreatePipelineLayout, &payload);
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::InvalidPayloadLength {
                index: 0,
                opcode: GpuOpcode::CreatePipelineLayout,
                expected: 16,
                actual: 12,
            }
        );
    }

    #[test]
    fn accepts_every_v1_opcode_payload_shape() {
        let payloads = [
            (GpuOpcode::CreateBuffer, 16),
            (GpuOpcode::WriteBuffer, 24),
            (GpuOpcode::CreateTexture, 24),
            (GpuOpcode::WriteTexture, 44),
            (GpuOpcode::CreateSampler, 24),
            (GpuOpcode::CreateShaderWgsl, 8),
            (GpuOpcode::CreateBindGroupLayout, 8),
            (GpuOpcode::CreatePipelineLayout, 8),
            (GpuOpcode::CreateBindGroup, 12),
            (GpuOpcode::CreateRenderPipeline, 40),
            (GpuOpcode::DestroyResource, 4),
            (GpuOpcode::BeginRenderPass, 36),
            (GpuOpcode::SetPipeline, 4),
            (GpuOpcode::SetVertexBuffer, 24),
            (GpuOpcode::SetIndexBuffer, 24),
            (GpuOpcode::SetBindGroup, 12),
            (GpuOpcode::SetViewport, 24),
            (GpuOpcode::SetScissorRect, 16),
            (GpuOpcode::Draw, 16),
            (GpuOpcode::DrawIndexed, 20),
            (GpuOpcode::EndRenderPass, 0),
            (GpuOpcode::CopyBufferToBuffer, 32),
            (GpuOpcode::CreateTextureView, 20),
            (GpuOpcode::CreateComputePipeline, 16),
            (GpuOpcode::BeginComputePass, 0),
            (GpuOpcode::SetComputePipeline, 4),
            (GpuOpcode::SetComputeBindGroup, 12),
            (GpuOpcode::DispatchWorkgroups, 12),
            (GpuOpcode::EndComputePass, 0),
        ];
        for (opcode, payload_bytes) in payloads {
            let batch = single_command(opcode, &vec![0; payload_bytes]);
            decode_gpu_batch(&batch)
                .unwrap_or_else(|error| panic!("{opcode:?} fixture failed: {error}"));
        }
    }

    #[test]
    fn keeps_extended_gpu_ids_stable() {
        assert_eq!(GpuTextureFormat::R8Unorm as u16, 7);
        assert_eq!(GpuBindingKind::StorageBuffer as u16, 4);
        assert_eq!(GpuBindingKind::StorageBufferReadWrite as u16, 5);
        assert_eq!(GpuCapabilityKey::MaxDispatchesPerBatch as u16, 21);
        assert_eq!(GPU_BUFFER_USAGE_STORAGE, 128);
    }

    #[test]
    fn rejects_unaligned_command_length() {
        let mut batch = DESTROY_FIXTURE;
        batch[28..32].copy_from_slice(&10u32.to_le_bytes());
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::InvalidCommandLength {
                index: 0,
                length: 10,
            }
        );
    }

    #[test]
    fn rejects_reserved_batch_flags() {
        let mut batch = DESTROY_FIXTURE;
        batch[6..8].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::ReservedBatchFlags { flags: 1 }
        );
    }

    #[test]
    fn rejects_zero_sequence() {
        let mut batch = DESTROY_FIXTURE;
        batch[16..24].fill(0);
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::EmptySequence
        );
    }

    #[test]
    fn rejects_trailing_partial_command() {
        let mut batch = DESTROY_FIXTURE.to_vec();
        batch.push(0);
        let batch_length = batch.len() as u32;
        batch[8..12].copy_from_slice(&batch_length.to_le_bytes());
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::TrailingCommandBytes { actual: 1 }
        );
    }

    #[test]
    fn rejects_batches_above_the_byte_ceiling_before_decoding() {
        let batch = vec![0; MAX_GPU_BATCH_BYTES + 1];
        assert_eq!(
            decode_gpu_batch(&batch).unwrap_err(),
            GpuWireError::BatchTooLarge {
                actual: MAX_GPU_BATCH_BYTES + 1,
            }
        );
    }

    fn single_command(opcode: GpuOpcode, payload: &[u8]) -> Vec<u8> {
        assert_eq!(payload.len() % 4, 0);
        let command_length = GPU_COMMAND_HEADER_BYTES + payload.len();
        let batch_length = GPU_BATCH_HEADER_BYTES + command_length;
        let mut bytes = Vec::with_capacity(batch_length);
        bytes.extend_from_slice(&GPU_WIRE_MAGIC);
        bytes.extend_from_slice(&GPU_WIRE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(batch_length as u32).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&(opcode as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(command_length as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }
}
