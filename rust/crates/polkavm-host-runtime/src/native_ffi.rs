/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{
    keyboard_insets_records, safe_area_insets_records, ApplicationRuntime, AudioChunk, Frame,
    GpuBatch, HostFrameResponseError, InputEvent, InputEventType, PresentationProfile,
    TextInputKind, Tri2dFrame, UiOutputFrame, UiSemanticsFrame, INPUT_EVENT_BYTES,
};
#[cfg(feature = "native-gpu")]
use crate::{NativeGpuFrame, NativeGpuRenderer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Foreign-language presentation profile matching the manifest contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmPresentationProfile {
    /// CPU-rendered packed framebuffer pixels.
    Framebuffer,
    /// Validated textured-triangle command streams.
    Tri2d,
    /// WebGPU raster commands without compute.
    WebGpuRaster,
    /// WebGPU raster and compute commands.
    WebGpu,
}

impl From<NativePolkaVmPresentationProfile> for PresentationProfile {
    fn from(value: NativePolkaVmPresentationProfile) -> Self {
        match value {
            NativePolkaVmPresentationProfile::Framebuffer => Self::Framebuffer,
            NativePolkaVmPresentationProfile::Tri2d => Self::Tri2d,
            NativePolkaVmPresentationProfile::WebGpuRaster => Self::WebGpuRaster,
            NativePolkaVmPresentationProfile::WebGpu => Self::WebGpu,
        }
    }
}

/// Fixed input event kind accepted by the native binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmInputEventType {
    /// Press a USB HID keyboard usage code.
    KeyDown,
    /// Release a USB HID keyboard usage code.
    KeyUp,
    /// Press a pointer button.
    ButtonDown,
    /// Release a pointer button.
    ButtonUp,
    /// Set absolute surface pointer coordinates.
    PointerMove,
    /// Report signed i16 pointer displacement encoded in u16 fields.
    PointerDelta,
    /// Report surface width and height through the coordinate fields.
    SurfaceMetrics,
    /// Begin a touch contact identified by the event code.
    TouchStart,
    /// Move an existing touch contact.
    TouchMove,
    /// End a touch contact normally.
    TouchEnd,
    /// Cancel a touch contact without completing its gesture.
    TouchCancel,
}

impl From<NativePolkaVmInputEventType> for InputEventType {
    fn from(value: NativePolkaVmInputEventType) -> Self {
        match value {
            NativePolkaVmInputEventType::KeyDown => Self::KeyDown,
            NativePolkaVmInputEventType::KeyUp => Self::KeyUp,
            NativePolkaVmInputEventType::ButtonDown => Self::ButtonDown,
            NativePolkaVmInputEventType::ButtonUp => Self::ButtonUp,
            NativePolkaVmInputEventType::PointerMove => Self::PointerMove,
            NativePolkaVmInputEventType::PointerDelta => Self::PointerDelta,
            NativePolkaVmInputEventType::SurfaceMetrics => Self::SurfaceMetrics,
            NativePolkaVmInputEventType::TouchStart => Self::TouchStart,
            NativePolkaVmInputEventType::TouchMove => Self::TouchMove,
            NativePolkaVmInputEventType::TouchEnd => Self::TouchEnd,
            NativePolkaVmInputEventType::TouchCancel => Self::TouchCancel,
        }
    }
}

/// Text operation accepted by the native binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmTextInputKind {
    /// Insert committed text outside IME composition.
    Text,
    /// Replace the current uncommitted IME composition.
    ImePreedit,
    /// Commit the current IME composition.
    ImeCommit,
}

impl From<NativePolkaVmTextInputKind> for TextInputKind {
    fn from(value: NativePolkaVmTextInputKind) -> Self {
        match value {
            NativePolkaVmTextInputKind::Text => Self::Text,
            NativePolkaVmTextInputKind::ImePreedit => Self::ImePreedit,
            NativePolkaVmTextInputKind::ImeCommit => Self::ImeCommit,
        }
    }
}

/// Host permission and availability state for motion input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmMotionAvailability {
    /// No sensor or fallback source exists.
    Unavailable,
    /// Motion is supported, though a new sample may not yet exist.
    Available,
    /// Platform or user denied motion access.
    PermissionDenied,
}

impl From<NativePolkaVmMotionAvailability> for crate::motion_wire::MotionAvailability {
    fn from(value: NativePolkaVmMotionAvailability) -> Self {
        match value {
            NativePolkaVmMotionAvailability::Unavailable => Self::Unavailable,
            NativePolkaVmMotionAvailability::Available => Self::Available,
            NativePolkaVmMotionAvailability::PermissionDenied => Self::PermissionDenied,
        }
    }
}

/// Immutable launch asset transferred into the runtime.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmAsset {
    /// Validated relative path visible to the guest.
    pub path: String,
    /// Complete file contents, subject to launch asset quotas.
    pub bytes: Vec<u8>,
}

/// CPU framebuffer returned across the foreign-language boundary.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmFrame {
    /// Surface width in pixels.
    pub width: u32,
    /// Surface height in pixels.
    pub height: u32,
    /// Packed 0xAARRGGBB pixels, represented as BGRA bytes on little-endian guests.
    pub argb: Vec<u8>,
}

impl From<Frame> for NativePolkaVmFrame {
    fn from(frame: Frame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            argb: frame.argb,
        }
    }
}

/// Accessibility snapshot returned across the foreign-language boundary.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmUiSemanticsFrame {
    /// Complete validated UTF-8 semantic JSON.
    pub bytes: Vec<u8>,
}

impl From<UiSemanticsFrame> for NativePolkaVmUiSemanticsFrame {
    fn from(frame: UiSemanticsFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

/// Guest UI platform requests returned to the native host.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmUiOutputFrame {
    /// Complete validated UI output wire stream.
    pub bytes: Vec<u8>,
}

impl From<UiOutputFrame> for NativePolkaVmUiOutputFrame {
    fn from(frame: UiOutputFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

/// Validated Tri2D stream with aggregate frame metadata.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmTri2dFrame {
    /// Target surface width in pixels.
    pub width: u32,
    /// Target surface height in pixels.
    pub height: u32,
    /// Number of draw commands.
    pub draw_count: u32,
    /// Total vertices across all draws.
    pub vertex_count: u32,
    /// Total indices across all draws.
    pub index_count: u32,
    /// Complete encoded stream, including texture updates and presentation.
    pub bytes: Vec<u8>,
}

impl From<Tri2dFrame> for NativePolkaVmTri2dFrame {
    fn from(frame: Tri2dFrame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            draw_count: frame.draw_count,
            vertex_count: frame.vertex_count,
            index_count: frame.index_count,
            bytes: frame.bytes,
        }
    }
}

/// Interleaved PCM audio awaiting native playback.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmAudioChunk {
    /// Signed 16-bit sample values interleaved by channel.
    pub samples: Vec<i16>,
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u32,
}

impl From<AudioChunk> for NativePolkaVmAudioChunk {
    fn from(chunk: AudioChunk) -> Self {
        Self {
            samples: chunk.samples,
            sample_rate: chunk.sample_rate,
            channels: chunk.channels,
        }
    }
}

/// GPU commands awaiting execution by the native host.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmGpuBatch {
    /// Complete validated batch encoding.
    pub bytes: Vec<u8>,
}

impl From<GpuBatch> for NativePolkaVmGpuBatch {
    fn from(batch: GpuBatch) -> Self {
        Self { bytes: batch.bytes }
    }
}

/// Read-back pixels from the native GPU renderer.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmGpuFrame {
    /// Surface width in pixels.
    pub width: u32,
    /// Surface height in pixels.
    pub height: u32,
    /// Tightly packed 8-bit RGBA pixels in row order.
    pub rgba: Vec<u8>,
}

#[cfg(feature = "native-gpu")]
impl From<NativeGpuFrame> for NativePolkaVmGpuFrame {
    fn from(frame: NativeGpuFrame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
        }
    }
}

/// Native runtime construction, execution, or synchronization failure.
#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum NativePolkaVmError {
    /// Runtime validation or execution rejected an operation.
    #[error("{detail}")]
    Runtime {
        /// Human-readable failure reason.
        detail: String,
    },
    /// Execution was permanently stopped and cannot accept this operation.
    #[error("PolkaVM runtime is stopped")]
    Stopped,
    /// The response queue is full; the host must retain and retry the response.
    #[error("host-frame response queue is full")]
    HostFrameResponseQueueFull,
    /// Launch assets contained the same path more than once.
    #[error("asset path appears more than once: {path}")]
    DuplicateAsset {
        /// Duplicate guest-visible asset path.
        path: String,
    },
    /// An earlier panic poisoned the runtime or renderer mutex.
    #[error("PolkaVM runtime mutex was poisoned")]
    RuntimePoisoned,
}

impl NativePolkaVmError {
    fn runtime(error: impl std::fmt::Display) -> Self {
        Self::Runtime {
            detail: error.to_string(),
        }
    }
}

/// Synchronized application runtime exposed through UniFFI.
#[derive(uniffi::Object)]
pub struct NativePolkaVmRuntime {
    runtime: Mutex<ApplicationRuntime>,
    #[cfg(feature = "native-gpu")]
    renderer: Mutex<Option<NativeGpuRenderer>>,
}

impl NativePolkaVmRuntime {
    fn lock(&self) -> Result<MutexGuard<'_, ApplicationRuntime>, NativePolkaVmError> {
        self.runtime
            .lock()
            .map_err(|_| NativePolkaVmError::RuntimePoisoned)
    }

    fn lock_running(&self) -> Result<MutexGuard<'_, ApplicationRuntime>, NativePolkaVmError> {
        let runtime = self.lock()?;
        if runtime.is_stopped() {
            return Err(NativePolkaVmError::Stopped);
        }
        Ok(runtime)
    }

    #[cfg(feature = "native-gpu")]
    fn renderer_lock(
        &self,
    ) -> Result<MutexGuard<'_, Option<NativeGpuRenderer>>, NativePolkaVmError> {
        self.renderer
            .lock()
            .map_err(|_| NativePolkaVmError::RuntimePoisoned)
    }
}

#[uniffi::export]
impl NativePolkaVmRuntime {
    /// Validate launch inputs and construct a runtime with a nonzero gas budget.
    ///
    /// Assets must have unique relative paths. Call `init` before updating.
    #[uniffi::constructor]
    pub fn new(
        program: Vec<u8>,
        assets: Vec<NativePolkaVmAsset>,
        presentation: NativePolkaVmPresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
    ) -> Result<Arc<Self>, NativePolkaVmError> {
        crate::validate_asset_count(assets.len()).map_err(NativePolkaVmError::runtime)?;
        let mut asset_map = HashMap::with_capacity(assets.len());
        for asset in assets {
            let path = asset.path;
            if asset_map.insert(path.clone(), asset.bytes).is_some() {
                return Err(NativePolkaVmError::DuplicateAsset { path });
            }
        }
        let runtime = ApplicationRuntime::new(
            &program,
            asset_map,
            presentation.into(),
            audio_enabled,
            max_gas_per_update,
        )
        .map_err(NativePolkaVmError::runtime)?;
        Ok(Arc::new(Self {
            runtime: Mutex::new(runtime),
            #[cfg(feature = "native-gpu")]
            renderer: Mutex::new(None),
        }))
    }

    /// Initialize the guest under its execution and host-call quotas.
    pub fn init(&self) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .init()
            .map_err(NativePolkaVmError::runtime)
    }

    /// Execute one bounded guest update.
    pub fn update(&self) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .update()
            .map_err(NativePolkaVmError::runtime)
    }

    /// Stop execution and discard pending execution work.
    pub fn stop(&self) -> Result<(), NativePolkaVmError> {
        self.lock()?.stop();
        Ok(())
    }

    /// Return the selected backend's lowercase debug name.
    pub fn backend(&self) -> Result<String, NativePolkaVmError> {
        Ok(format!("{:?}", self.lock()?.backend()).to_ascii_lowercase())
    }

    /// Whether the guest imports motion input.
    pub fn uses_motion(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_motion())
    }

    /// Whether the guest opts into host-scheduled updates.
    pub fn uses_update_scheduling(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_update_scheduling())
    }

    /// Requested delay after the latest update, or `None` to await host input.
    pub fn update_after_ms(&self) -> Result<Option<u32>, NativePolkaVmError> {
        Ok(self.lock()?.update_after_ms())
    }

    /// Gas consumed from the latest guest execution budget.
    pub fn last_gas_used(&self) -> Result<u64, NativePolkaVmError> {
        Ok(self.lock()?.last_gas_used())
    }

    /// Queue a fixed input event; coordinates follow the selected event's contract.
    pub fn send_input(
        &self,
        event_type: NativePolkaVmInputEventType,
        code: u8,
        x: u16,
        y: u16,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?.send_input(InputEvent {
            event_type: event_type.into(),
            code,
            x,
            y,
        });
        Ok(())
    }

    /// Validate and queue exactly eight encoded input bytes; unsupported for CoreVM.
    pub fn send_input_record(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        let mut runtime = self.lock_running()?;
        let record: [u8; INPUT_EVENT_BYTES] = bytes.try_into().map_err(|_| {
            NativePolkaVmError::runtime(format!(
                "input record must contain exactly {INPUT_EVENT_BYTES} bytes"
            ))
        })?;
        runtime
            .send_input_record(record)
            .map_err(NativePolkaVmError::runtime)
    }

    /// Reports the safe-area insets in physical surface pixels: the edges a
    /// cutout, status bar, or rounded corner covers.
    pub fn send_safe_area_insets(
        &self,
        left: u16,
        top: u16,
        right: u16,
        bottom: u16,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_input_records(&safe_area_insets_records(left, top, right, bottom))
            .map_err(NativePolkaVmError::runtime)
    }

    /// Reports the edge-connected occlusion of a Host-owned virtual keyboard,
    /// in physical surface pixels. A dismissed keyboard sends four zeros.
    pub fn send_keyboard_insets(
        &self,
        left: u16,
        top: u16,
        right: u16,
        bottom: u16,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_input_records(&keyboard_insets_records(left, top, right, bottom))
            .map_err(NativePolkaVmError::runtime)
    }

    /// Encode and queue a bounded UTF-8 text operation; unsupported for CoreVM.
    pub fn send_text_input(
        &self,
        kind: NativePolkaVmTextInputKind,
        text: String,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_text_input(kind.into(), &text)
            .map_err(NativePolkaVmError::runtime)
    }

    /// Update motion availability, clearing pending samples if access is lost.
    pub fn set_motion_availability(
        &self,
        availability: NativePolkaVmMotionAvailability,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_motion_availability(availability.into());
        Ok(())
    }

    /// Validate and replace the latest encoded motion sample.
    pub fn send_motion_sample(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_motion_sample(&bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    /// Whether the guest imports pointer-capture operations.
    pub fn uses_pointer_capture(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_pointer_capture())
    }

    /// Set whether the host can honor pointer-capture requests.
    pub fn set_pointer_capture_supported(&self, supported: bool) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_pointer_capture_supported(supported);
        Ok(())
    }

    /// Report the actual host pointer-capture state to the guest.
    pub fn set_pointer_capture_active(&self, active: bool) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_pointer_capture_active(active)
            .map_err(NativePolkaVmError::runtime)
    }

    /// Take the pending capture request: `true` acquires, `false` releases.
    pub fn take_pointer_capture_request(&self) -> Result<Option<bool>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_pointer_capture_request())
    }

    /// Whether no GPU capabilities prerequisite remains before execution.
    pub fn gpu_ready(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock_running()?.gpu_ready())
    }

    /// Validate and install encoded host capabilities for a GPU guest.
    pub fn set_gpu_capabilities(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_gpu_capabilities(bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    /// Validate and queue an encoded host GPU event.
    pub fn send_gpu_event(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_gpu_event(bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    /// Create a native GPU surface of the requested pixel dimensions and install capabilities.
    ///
    /// Fails when this build lacks the `native-gpu` feature.
    pub fn configure_native_gpu(&self, width: u32, height: u32) -> Result<(), NativePolkaVmError> {
        let mut runtime = self.lock_running()?;
        #[cfg(feature = "native-gpu")]
        {
            let renderer =
                NativeGpuRenderer::new(width, height).map_err(NativePolkaVmError::runtime)?;
            let capabilities = renderer.capabilities();
            runtime
                .set_gpu_capabilities(capabilities)
                .map_err(NativePolkaVmError::runtime)?;
            *self.renderer_lock()? = Some(renderer);
            Ok(())
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = (&mut runtime, width, height);
            Err(NativePolkaVmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    /// Resize the configured GPU surface in pixels and refresh guest capabilities.
    pub fn resize_native_gpu(&self, width: u32, height: u32) -> Result<(), NativePolkaVmError> {
        let mut runtime = self.lock_running()?;
        #[cfg(feature = "native-gpu")]
        {
            let mut renderer = self.renderer_lock()?;
            let renderer = renderer.as_mut().ok_or_else(|| {
                NativePolkaVmError::runtime("native GPU renderer is not configured")
            })?;
            renderer
                .resize(width, height)
                .map_err(NativePolkaVmError::runtime)?;
            runtime
                .set_gpu_capabilities(renderer.capabilities())
                .map_err(NativePolkaVmError::runtime)
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = (&mut runtime, width, height);
            Err(NativePolkaVmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    /// Drain pending GPU batches and return the newest rendered frame, if any.
    ///
    /// Execution events are queued back to the guest; requires a configured renderer.
    pub fn render_native_gpu(&self) -> Result<Option<NativePolkaVmGpuFrame>, NativePolkaVmError> {
        let mut runtime = self.lock_running()?;
        #[cfg(feature = "native-gpu")]
        {
            let mut renderer = self.renderer_lock()?;
            let renderer = renderer.as_mut().ok_or_else(|| {
                NativePolkaVmError::runtime("native GPU renderer is not configured")
            })?;
            let mut frame = None;
            while let Some(batch) = runtime.take_gpu_batch() {
                let rendered = renderer.execute(&batch.bytes);
                for event in rendered.events {
                    runtime
                        .send_gpu_event(event)
                        .map_err(NativePolkaVmError::runtime)?;
                }
                if let Some(rendered_frame) = rendered.frame {
                    frame = Some(rendered_frame.into());
                }
            }
            Ok(frame)
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = &mut runtime;
            Err(NativePolkaVmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    /// Take the newest CPU framebuffer.
    pub fn take_frame(&self) -> Result<Option<NativePolkaVmFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_frame().map(Into::into))
    }

    /// Take the pending Tri2D frame.
    pub fn take_tri2d(&self) -> Result<Option<NativePolkaVmTri2dFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_tri2d().map(Into::into))
    }

    /// Remove the oldest audio chunk.
    pub fn take_audio(&self) -> Result<Option<NativePolkaVmAudioChunk>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_audio().map(Into::into))
    }

    /// Remove the oldest GPU batch for execution by an external renderer.
    pub fn take_gpu_batch(&self) -> Result<Option<NativePolkaVmGpuBatch>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_gpu_batch().map(Into::into))
    }

    /// Remove the oldest pending guest host-service request.
    pub fn take_host_frame_request(&self) -> Result<Option<Vec<u8>>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_host_frame_request())
    }

    /// Queue a nonempty bounded host-service response; report backpressure without dropping it.
    pub fn send_host_frame_response(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        let mut runtime = self.lock_running()?;
        match runtime.send_host_frame_response(bytes) {
            Ok(()) => Ok(()),
            Err(HostFrameResponseError::InvalidFrame) => Err(NativePolkaVmError::runtime(
                HostFrameResponseError::InvalidFrame,
            )),
            Err(HostFrameResponseError::QueueFull) => {
                Err(NativePolkaVmError::HostFrameResponseQueueFull)
            }
            Err(HostFrameResponseError::RuntimeStopped) => Err(NativePolkaVmError::Stopped),
        }
    }

    /// Take the newest validated accessibility snapshot.
    pub fn take_ui_semantics(
        &self,
    ) -> Result<Option<NativePolkaVmUiSemanticsFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_ui_semantics().map(Into::into))
    }

    /// Take the newest UI platform-output snapshot.
    pub fn take_ui_output(&self) -> Result<Option<NativePolkaVmUiOutputFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_ui_output().map(Into::into))
    }

    /// Remove the oldest queued guest log.
    pub fn take_log(&self) -> Result<Option<String>, NativePolkaVmError> {
        Ok(self.lock()?.take_log())
    }

    /// Whether a CoreVM guest completed with exit status zero.
    pub fn is_exited(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.is_exited())
    }

    /// Take the latest guest save payload.
    pub fn take_save(&self) -> Result<Option<Vec<u8>>, NativePolkaVmError> {
        Ok(self.lock()?.take_save())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polkavm::Reg;
    use polkavm_common::abi::MemoryMapBuilder;
    use polkavm_common::program::{asm, InstructionSetKind};
    use polkavm_common::writer::ProgramBlobBuilder;

    const HOST_FRAME_PROGRAM: &[u8] =
        include_bytes!("../tests/fixtures/host-frame-roundtrip.polkavm");
    const HOST_FRAME_RESPONSE: &[u8] = b"host-frame-conformance-response-v1";
    const HOST_FRAME_SUCCESS: &[u8] = b"host-frame-roundtrip-ok";
    const PRESERVED_LOG: &str = "guest log survives runtime stop";

    fn host_frame_runtime() -> Arc<NativePolkaVmRuntime> {
        NativePolkaVmRuntime::new(
            HOST_FRAME_PROGRAM.to_vec(),
            Vec::new(),
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            10_000_000,
        )
        .expect("create native facade")
    }

    fn gas_exhausting_corevm_program() -> Vec<u8> {
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
        builder.set_stack_size(4 * 1024);
        builder.add_export_by_basic_block(0, b"_pvm_start");
        let mut code = (0..64)
            .map(|value| asm::load_imm(Reg::A0, value))
            .collect::<Vec<_>>();
        code.push(asm::ret());
        builder.set_code(&code, &[]);
        builder.into_vec().expect("build gas-exhausting guest")
    }

    fn logging_program() -> Vec<u8> {
        let stack_size = 4 * 1024;
        let memory = MemoryMapBuilder::new(64 * 1024)
            .ro_data_size(PRESERVED_LOG.len() as u32)
            .stack_size(stack_size)
            .build()
            .expect("build guest memory map");
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
        builder.set_ro_data_size(PRESERVED_LOG.len() as u32);
        builder.set_ro_data(PRESERVED_LOG.as_bytes().to_vec());
        builder.set_stack_size(stack_size);
        builder.add_import(b"host_log");
        builder.add_export_by_basic_block(0, b"init");
        builder.add_export_by_basic_block(0, b"update");
        builder.set_code(
            &[
                asm::load_imm(Reg::A0, memory.ro_data_address() as i32),
                asm::load_imm(Reg::A1, PRESERVED_LOG.len() as i32),
                asm::ecalli(0),
                asm::ret(),
            ],
            &[],
        );
        builder.into_vec().expect("build logging guest")
    }

    fn host_frame_polling_program() -> Vec<u8> {
        let stack_size = 4 * 1024;
        let memory = MemoryMapBuilder::new(64 * 1024)
            .rw_data_size(1)
            .stack_size(stack_size)
            .build()
            .expect("build guest memory map");
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
        builder.set_rw_data_size(1);
        builder.set_stack_size(stack_size);
        builder.add_import(b"host_frame_poll");
        builder.add_export_by_basic_block(0, b"init");
        builder.add_export_by_basic_block(0, b"update");
        builder.set_code(
            &[
                asm::load_imm(Reg::A0, memory.rw_data_address() as i32),
                asm::load_imm(Reg::A1, 1),
                asm::ecalli(0),
                asm::ret(),
            ],
            &[],
        );
        builder.into_vec().expect("build host-frame polling guest")
    }

    fn assert_stopped<T>(result: Result<T, NativePolkaVmError>) {
        assert!(matches!(result, Err(NativePolkaVmError::Stopped)));
    }

    #[test]
    fn duplicate_assets_are_rejected_before_program_parsing() {
        let result = NativePolkaVmRuntime::new(
            Vec::new(),
            vec![
                NativePolkaVmAsset {
                    path: "data.bin".into(),
                    bytes: vec![1],
                },
                NativePolkaVmAsset {
                    path: "data.bin".into(),
                    bytes: vec![2],
                },
            ],
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            1,
        );
        assert!(matches!(
            result,
            Err(NativePolkaVmError::DuplicateAsset { .. })
        ));
    }

    #[test]
    fn native_ffi_roundtrips_an_opaque_host_frame() {
        const PROGRAM: &[u8] = include_bytes!("../tests/fixtures/host-frame-roundtrip.polkavm");
        const REQUEST: &[u8] = b"host-frame-conformance-request-v1";
        const RESPONSE: &[u8] = b"host-frame-conformance-response-v1";
        const SUCCESS: &[u8] = b"host-frame-roundtrip-ok";

        let runtime = NativePolkaVmRuntime::new(
            PROGRAM.to_vec(),
            Vec::new(),
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            10_000_000,
        )
        .expect("create native facade");

        runtime.init().expect("initialize guest");
        assert_eq!(
            runtime
                .take_host_frame_request()
                .expect("take request")
                .as_deref(),
            Some(REQUEST)
        );
        assert_eq!(
            runtime
                .take_host_frame_request()
                .expect("empty request queue"),
            None
        );

        runtime
            .send_host_frame_response(RESPONSE.to_vec())
            .expect("queue response");
        runtime.update().expect("deliver response");
        assert_eq!(
            runtime.take_save().expect("take save").as_deref(),
            Some(SUCCESS)
        );
    }

    #[test]
    fn init_failure_is_terminal_and_rejects_further_input() {
        let runtime = NativePolkaVmRuntime::new(
            HOST_FRAME_PROGRAM.to_vec(),
            Vec::new(),
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            1,
        )
        .expect("create native facade");

        assert!(matches!(
            runtime.init(),
            Err(NativePolkaVmError::Runtime { .. })
        ));
        assert!(runtime.is_exited().expect("observe terminal state"));
        assert_stopped(runtime.init());
        assert_stopped(runtime.send_input(NativePolkaVmInputEventType::KeyDown, 1, 0, 0));
        assert_stopped(runtime.take_host_frame_request());
    }

    #[test]
    fn update_failure_is_terminal_and_rejects_further_mediation() {
        let runtime = NativePolkaVmRuntime::new(
            gas_exhausting_corevm_program(),
            Vec::new(),
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            1,
        )
        .expect("create native facade");

        runtime.init().expect("CoreVM initialization is host-side");
        assert!(matches!(
            runtime.update(),
            Err(NativePolkaVmError::Runtime { .. })
        ));
        assert!(runtime.is_exited().expect("observe terminal state"));
        assert_stopped(runtime.update());
        assert_stopped(runtime.take_host_frame_request());
        assert_stopped(runtime.send_host_frame_response(vec![1]));
        assert_stopped(runtime.send_input(NativePolkaVmInputEventType::KeyDown, 1, 0, 0));
    }

    #[test]
    fn explicit_stop_is_idempotent_and_clears_host_frame_queues() {
        let runtime = host_frame_runtime();
        runtime.init().expect("initialize guest");
        runtime
            .send_host_frame_response(HOST_FRAME_RESPONSE.to_vec())
            .expect("queue response");
        {
            let runtime = runtime.lock().expect("lock runtime");
            let ApplicationRuntime::Cooperative(runtime) = &*runtime else {
                panic!("fixture must use the cooperative ABI");
            };
            assert!(!runtime.host_frame_queues_are_empty());
        }

        runtime.stop().expect("stop runtime");
        runtime.stop().expect("stop runtime again");

        {
            let runtime = runtime.lock().expect("lock stopped runtime");
            let ApplicationRuntime::Cooperative(runtime) = &*runtime else {
                panic!("fixture must use the cooperative ABI");
            };
            assert!(runtime.host_frame_queues_are_empty());
        }
        assert!(runtime.is_exited().expect("observe stopped runtime"));
        assert_stopped(runtime.take_host_frame_request());
        assert_stopped(runtime.send_host_frame_response(HOST_FRAME_RESPONSE.to_vec()));
        assert_stopped(runtime.take_audio());
        assert_stopped(runtime.gpu_ready());
    }

    #[test]
    fn host_frame_response_backpressure_is_retryable() {
        let runtime = NativePolkaVmRuntime::new(
            host_frame_polling_program(),
            Vec::new(),
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            10_000_000,
        )
        .expect("create native facade");
        runtime.init().expect("initialize polling guest");

        for response in 0..crate::MAX_QUEUED_HOST_FRAMES {
            runtime
                .send_host_frame_response(vec![response as u8])
                .expect("fill bounded response queue");
        }
        assert!(matches!(
            runtime.send_host_frame_response(vec![255]),
            Err(NativePolkaVmError::HostFrameResponseQueueFull)
        ));
        assert!(!runtime.is_exited().expect("queue pressure is nonterminal"));

        runtime.update().expect("guest drains one response");
        runtime
            .send_host_frame_response(vec![255])
            .expect("retry response after guest drain");
        assert!(!runtime.is_exited().expect("retry keeps runtime alive"));
    }

    #[test]
    fn stopped_runtime_preserves_queued_guest_logs() {
        let runtime = NativePolkaVmRuntime::new(
            logging_program(),
            Vec::new(),
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            10_000_000,
        )
        .expect("create native facade");
        runtime.init().expect("initialize logging guest");
        runtime.stop().expect("stop runtime");

        assert_eq!(
            runtime.take_log().expect("drain log after stop").as_deref(),
            Some(PRESERVED_LOG)
        );
        assert_eq!(runtime.take_log().expect("log queue is drained"), None);
    }

    #[test]
    fn stopped_runtime_preserves_pending_save() {
        let runtime = host_frame_runtime();
        runtime.init().expect("initialize guest");
        runtime
            .send_host_frame_response(HOST_FRAME_RESPONSE.to_vec())
            .expect("queue response");
        runtime.update().expect("guest submits save");
        runtime.stop().expect("stop runtime");

        assert_eq!(
            runtime
                .take_save()
                .expect("drain save after stop")
                .as_deref(),
            Some(HOST_FRAME_SUCCESS)
        );
        assert_eq!(runtime.take_save().expect("save is drained"), None);
    }

    #[test]
    fn host_transport_failure_stops_and_clears_the_runtime() {
        let runtime = host_frame_runtime();
        runtime.init().expect("initialize guest");

        assert!(matches!(
            runtime.send_host_frame_response(Vec::new()),
            Err(NativePolkaVmError::Runtime { .. })
        ));
        assert!(runtime.is_exited().expect("observe terminal state"));
        assert_stopped(runtime.take_host_frame_request());
        assert_stopped(runtime.update());
    }

    #[test]
    fn text_input_kinds_match_the_runtime_contract() {
        assert_eq!(
            TextInputKind::from(NativePolkaVmTextInputKind::Text),
            TextInputKind::Text
        );
        assert_eq!(
            TextInputKind::from(NativePolkaVmTextInputKind::ImePreedit),
            TextInputKind::ImePreedit
        );
        assert_eq!(
            TextInputKind::from(NativePolkaVmTextInputKind::ImeCommit),
            TextInputKind::ImeCommit
        );
    }
}
