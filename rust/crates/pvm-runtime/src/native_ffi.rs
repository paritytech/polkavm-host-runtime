/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{
    ApplicationRuntime, AudioChunk, Frame, GpuBatch, InputEvent, InputEventType,
    PresentationProfile, TextInputKind, Tri2dFrame, UiOutputFrame, UiSemanticsFrame,
    INPUT_EVENT_BYTES,
};
#[cfg(feature = "native-gpu")]
use crate::{NativeGpuFrame, NativeGpuRenderer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Foreign-language presentation profile matching the manifest contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmPresentationProfile {
    /// CPU-rendered packed framebuffer pixels.
    Framebuffer,
    /// Validated textured-triangle command streams.
    Tri2d,
    /// WebGPU raster commands without compute.
    WebGpuRaster,
    /// WebGPU raster and compute commands.
    WebGpu,
}

impl From<NativePvmPresentationProfile> for PresentationProfile {
    fn from(value: NativePvmPresentationProfile) -> Self {
        match value {
            NativePvmPresentationProfile::Framebuffer => Self::Framebuffer,
            NativePvmPresentationProfile::Tri2d => Self::Tri2d,
            NativePvmPresentationProfile::WebGpuRaster => Self::WebGpuRaster,
            NativePvmPresentationProfile::WebGpu => Self::WebGpu,
        }
    }
}

/// Fixed input event kind accepted by the native binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmInputEventType {
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
}

impl From<NativePvmInputEventType> for InputEventType {
    fn from(value: NativePvmInputEventType) -> Self {
        match value {
            NativePvmInputEventType::KeyDown => Self::KeyDown,
            NativePvmInputEventType::KeyUp => Self::KeyUp,
            NativePvmInputEventType::ButtonDown => Self::ButtonDown,
            NativePvmInputEventType::ButtonUp => Self::ButtonUp,
            NativePvmInputEventType::PointerMove => Self::PointerMove,
            NativePvmInputEventType::PointerDelta => Self::PointerDelta,
            NativePvmInputEventType::SurfaceMetrics => Self::SurfaceMetrics,
        }
    }
}

/// Text operation accepted by the native binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmTextInputKind {
    /// Insert committed text outside IME composition.
    Text,
    /// Replace the current uncommitted IME composition.
    ImePreedit,
    /// Commit the current IME composition.
    ImeCommit,
}

impl From<NativePvmTextInputKind> for TextInputKind {
    fn from(value: NativePvmTextInputKind) -> Self {
        match value {
            NativePvmTextInputKind::Text => Self::Text,
            NativePvmTextInputKind::ImePreedit => Self::ImePreedit,
            NativePvmTextInputKind::ImeCommit => Self::ImeCommit,
        }
    }
}

/// Host permission and availability state for motion input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmMotionAvailability {
    /// No sensor or fallback source exists.
    Unavailable,
    /// Motion is supported, though a new sample may not yet exist.
    Available,
    /// Platform or user denied motion access.
    PermissionDenied,
}

impl From<NativePvmMotionAvailability> for crate::motion_wire::MotionAvailability {
    fn from(value: NativePvmMotionAvailability) -> Self {
        match value {
            NativePvmMotionAvailability::Unavailable => Self::Unavailable,
            NativePvmMotionAvailability::Available => Self::Available,
            NativePvmMotionAvailability::PermissionDenied => Self::PermissionDenied,
        }
    }
}

/// Immutable launch asset transferred into the runtime.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmAsset {
    /// Validated relative path visible to the guest.
    pub path: String,
    /// Complete file contents, subject to launch asset quotas.
    pub bytes: Vec<u8>,
}

/// CPU framebuffer returned across the foreign-language boundary.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmFrame {
    /// Surface width in pixels.
    pub width: u32,
    /// Surface height in pixels.
    pub height: u32,
    /// Packed 0xAARRGGBB pixels, represented as BGRA bytes on little-endian guests.
    pub argb: Vec<u8>,
}

impl From<Frame> for NativePvmFrame {
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
pub struct NativePvmUiSemanticsFrame {
    /// Complete validated UTF-8 semantic JSON.
    pub bytes: Vec<u8>,
}

impl From<UiSemanticsFrame> for NativePvmUiSemanticsFrame {
    fn from(frame: UiSemanticsFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

/// Guest UI platform requests returned to the native host.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmUiOutputFrame {
    /// Complete validated UI output wire stream.
    pub bytes: Vec<u8>,
}

impl From<UiOutputFrame> for NativePvmUiOutputFrame {
    fn from(frame: UiOutputFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

/// Validated Tri2D stream with aggregate frame metadata.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmTri2dFrame {
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

impl From<Tri2dFrame> for NativePvmTri2dFrame {
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
pub struct NativePvmAudioChunk {
    /// Signed 16-bit sample values interleaved by channel.
    pub samples: Vec<i16>,
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u32,
}

impl From<AudioChunk> for NativePvmAudioChunk {
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
pub struct NativePvmGpuBatch {
    /// Complete validated batch encoding.
    pub bytes: Vec<u8>,
}

impl From<GpuBatch> for NativePvmGpuBatch {
    fn from(batch: GpuBatch) -> Self {
        Self { bytes: batch.bytes }
    }
}

/// Read-back pixels from the native GPU renderer.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmGpuFrame {
    /// Surface width in pixels.
    pub width: u32,
    /// Surface height in pixels.
    pub height: u32,
    /// Tightly packed 8-bit RGBA pixels in row order.
    pub rgba: Vec<u8>,
}

#[cfg(feature = "native-gpu")]
impl From<NativeGpuFrame> for NativePvmGpuFrame {
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
pub enum NativePvmError {
    /// Runtime validation or execution rejected an operation.
    #[error("{detail}")]
    Runtime {
        /// Human-readable failure reason.
        detail: String,
    },
    /// Launch assets contained the same path more than once.
    #[error("asset path appears more than once: {path}")]
    DuplicateAsset {
        /// Duplicate guest-visible asset path.
        path: String,
    },
    /// An earlier panic poisoned the runtime or renderer mutex.
    #[error("PVM runtime mutex was poisoned")]
    RuntimePoisoned,
}

impl NativePvmError {
    fn runtime(error: impl std::fmt::Display) -> Self {
        Self::Runtime {
            detail: error.to_string(),
        }
    }
}

/// Synchronized application runtime exposed through UniFFI.
#[derive(uniffi::Object)]
pub struct NativePvmRuntime {
    runtime: Mutex<ApplicationRuntime>,
    #[cfg(feature = "native-gpu")]
    renderer: Mutex<Option<NativeGpuRenderer>>,
}

impl NativePvmRuntime {
    fn lock(&self) -> Result<MutexGuard<'_, ApplicationRuntime>, NativePvmError> {
        self.runtime
            .lock()
            .map_err(|_| NativePvmError::RuntimePoisoned)
    }

    #[cfg(feature = "native-gpu")]
    fn renderer_lock(&self) -> Result<MutexGuard<'_, Option<NativeGpuRenderer>>, NativePvmError> {
        self.renderer
            .lock()
            .map_err(|_| NativePvmError::RuntimePoisoned)
    }
}

#[uniffi::export]
impl NativePvmRuntime {
    /// Validate launch inputs and construct a runtime with a nonzero gas budget.
    ///
    /// Assets must have unique relative paths. Call `init` before updating.
    #[uniffi::constructor]
    pub fn new(
        program: Vec<u8>,
        assets: Vec<NativePvmAsset>,
        presentation: NativePvmPresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
    ) -> Result<Arc<Self>, NativePvmError> {
        crate::validate_asset_count(assets.len()).map_err(NativePvmError::runtime)?;
        let mut asset_map = HashMap::with_capacity(assets.len());
        for asset in assets {
            let path = asset.path;
            if asset_map.insert(path.clone(), asset.bytes).is_some() {
                return Err(NativePvmError::DuplicateAsset { path });
            }
        }
        let runtime = ApplicationRuntime::new(
            &program,
            asset_map,
            presentation.into(),
            audio_enabled,
            max_gas_per_update,
        )
        .map_err(NativePvmError::runtime)?;
        Ok(Arc::new(Self {
            runtime: Mutex::new(runtime),
            #[cfg(feature = "native-gpu")]
            renderer: Mutex::new(None),
        }))
    }

    /// Initialize the guest under its execution and host-call quotas.
    pub fn init(&self) -> Result<(), NativePvmError> {
        self.lock()?.init().map_err(NativePvmError::runtime)
    }

    /// Execute one bounded guest update.
    pub fn update(&self) -> Result<(), NativePvmError> {
        self.lock()?.update().map_err(NativePvmError::runtime)
    }

    /// Return the selected backend's lowercase debug name.
    pub fn backend(&self) -> Result<String, NativePvmError> {
        Ok(format!("{:?}", self.lock()?.backend()).to_ascii_lowercase())
    }

    /// Whether the guest imports motion input.
    pub fn uses_motion(&self) -> Result<bool, NativePvmError> {
        Ok(self.lock()?.uses_motion())
    }

    /// Gas consumed from the latest guest execution budget.
    pub fn last_gas_used(&self) -> Result<u64, NativePvmError> {
        Ok(self.lock()?.last_gas_used())
    }

    /// Queue a fixed input event; coordinates follow the selected event's contract.
    pub fn send_input(
        &self,
        event_type: NativePvmInputEventType,
        code: u8,
        x: u16,
        y: u16,
    ) -> Result<(), NativePvmError> {
        self.lock()?.send_input(InputEvent {
            event_type: event_type.into(),
            code,
            x,
            y,
        });
        Ok(())
    }

    /// Validate and queue exactly eight encoded input bytes; unsupported for CoreVM.
    pub fn send_input_record(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        let record: [u8; INPUT_EVENT_BYTES] = bytes.try_into().map_err(|_| {
            NativePvmError::runtime(format!(
                "input record must contain exactly {INPUT_EVENT_BYTES} bytes"
            ))
        })?;
        self.lock()?
            .send_input_record(record)
            .map_err(NativePvmError::runtime)
    }

    /// Encode and queue a bounded UTF-8 text operation; unsupported for CoreVM.
    pub fn send_text_input(
        &self,
        kind: NativePvmTextInputKind,
        text: String,
    ) -> Result<(), NativePvmError> {
        self.lock()?
            .send_text_input(kind.into(), &text)
            .map_err(NativePvmError::runtime)
    }

    /// Update motion availability, clearing pending samples if access is lost.
    pub fn set_motion_availability(
        &self,
        availability: NativePvmMotionAvailability,
    ) -> Result<(), NativePvmError> {
        self.lock()?.set_motion_availability(availability.into());
        Ok(())
    }

    /// Validate and replace the latest encoded motion sample.
    pub fn send_motion_sample(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        self.lock()?
            .send_motion_sample(&bytes)
            .map_err(NativePvmError::runtime)
    }

    /// Whether no GPU capabilities prerequisite remains before execution.
    pub fn gpu_ready(&self) -> Result<bool, NativePvmError> {
        Ok(self.lock()?.gpu_ready())
    }

    /// Validate and install encoded host capabilities for a GPU guest.
    pub fn set_gpu_capabilities(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        self.lock()?
            .set_gpu_capabilities(bytes)
            .map_err(NativePvmError::runtime)
    }

    /// Validate and queue an encoded host GPU event.
    pub fn send_gpu_event(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        self.lock()?
            .send_gpu_event(bytes)
            .map_err(NativePvmError::runtime)
    }

    /// Create a native GPU surface of the requested pixel dimensions and install capabilities.
    ///
    /// Fails when this build lacks the `native-gpu` feature.
    pub fn configure_native_gpu(&self, width: u32, height: u32) -> Result<(), NativePvmError> {
        #[cfg(feature = "native-gpu")]
        {
            let renderer =
                NativeGpuRenderer::new(width, height).map_err(NativePvmError::runtime)?;
            let capabilities = renderer.capabilities();
            let mut runtime = self.lock()?;
            runtime
                .set_gpu_capabilities(capabilities)
                .map_err(NativePvmError::runtime)?;
            *self.renderer_lock()? = Some(renderer);
            Ok(())
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = (width, height);
            Err(NativePvmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    /// Resize the configured GPU surface in pixels and refresh guest capabilities.
    pub fn resize_native_gpu(&self, width: u32, height: u32) -> Result<(), NativePvmError> {
        #[cfg(feature = "native-gpu")]
        {
            let mut runtime = self.lock()?;
            let mut renderer = self.renderer_lock()?;
            let renderer = renderer
                .as_mut()
                .ok_or_else(|| NativePvmError::runtime("native GPU renderer is not configured"))?;
            renderer
                .resize(width, height)
                .map_err(NativePvmError::runtime)?;
            runtime
                .set_gpu_capabilities(renderer.capabilities())
                .map_err(NativePvmError::runtime)
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = (width, height);
            Err(NativePvmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    /// Drain pending GPU batches and return the newest rendered frame, if any.
    ///
    /// Execution events are queued back to the guest; requires a configured renderer.
    pub fn render_native_gpu(&self) -> Result<Option<NativePvmGpuFrame>, NativePvmError> {
        #[cfg(feature = "native-gpu")]
        {
            let mut runtime = self.lock()?;
            let mut renderer = self.renderer_lock()?;
            let renderer = renderer
                .as_mut()
                .ok_or_else(|| NativePvmError::runtime("native GPU renderer is not configured"))?;
            let mut frame = None;
            while let Some(batch) = runtime.take_gpu_batch() {
                let rendered = renderer.execute(&batch.bytes);
                for event in rendered.events {
                    runtime
                        .send_gpu_event(event)
                        .map_err(NativePvmError::runtime)?;
                }
                if let Some(rendered_frame) = rendered.frame {
                    frame = Some(rendered_frame.into());
                }
            }
            Ok(frame)
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            Err(NativePvmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    /// Take the newest CPU framebuffer.
    pub fn take_frame(&self) -> Result<Option<NativePvmFrame>, NativePvmError> {
        Ok(self.lock()?.take_frame().map(Into::into))
    }

    /// Take the pending Tri2D frame.
    pub fn take_tri2d(&self) -> Result<Option<NativePvmTri2dFrame>, NativePvmError> {
        Ok(self.lock()?.take_tri2d().map(Into::into))
    }

    /// Remove the oldest audio chunk.
    pub fn take_audio(&self) -> Result<Option<NativePvmAudioChunk>, NativePvmError> {
        Ok(self.lock()?.take_audio().map(Into::into))
    }

    /// Remove the oldest GPU batch for execution by an external renderer.
    pub fn take_gpu_batch(&self) -> Result<Option<NativePvmGpuBatch>, NativePvmError> {
        Ok(self.lock()?.take_gpu_batch().map(Into::into))
    }

    /// Take the newest accessibility snapshot.
    pub fn take_ui_semantics(&self) -> Result<Option<NativePvmUiSemanticsFrame>, NativePvmError> {
        Ok(self.lock()?.take_ui_semantics().map(Into::into))
    }

    /// Take the newest UI platform-output snapshot.
    pub fn take_ui_output(&self) -> Result<Option<NativePvmUiOutputFrame>, NativePvmError> {
        Ok(self.lock()?.take_ui_output().map(Into::into))
    }

    /// Remove the oldest queued guest log.
    pub fn take_log(&self) -> Result<Option<String>, NativePvmError> {
        Ok(self.lock()?.take_log())
    }

    /// Whether a CoreVM guest completed with exit status zero.
    pub fn is_exited(&self) -> Result<bool, NativePvmError> {
        Ok(self.lock()?.is_exited())
    }

    /// Take the latest guest save payload.
    pub fn take_save(&self) -> Result<Option<Vec<u8>>, NativePvmError> {
        Ok(self.lock()?.take_save())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_assets_are_rejected_before_program_parsing() {
        let result = NativePvmRuntime::new(
            Vec::new(),
            vec![
                NativePvmAsset {
                    path: "data.bin".into(),
                    bytes: vec![1],
                },
                NativePvmAsset {
                    path: "data.bin".into(),
                    bytes: vec![2],
                },
            ],
            NativePvmPresentationProfile::Framebuffer,
            false,
            1,
        );
        assert!(matches!(result, Err(NativePvmError::DuplicateAsset { .. })));
    }

    #[test]
    fn text_input_kinds_match_the_runtime_contract() {
        assert_eq!(
            TextInputKind::from(NativePvmTextInputKind::Text),
            TextInputKind::Text
        );
        assert_eq!(
            TextInputKind::from(NativePvmTextInputKind::ImePreedit),
            TextInputKind::ImePreedit
        );
        assert_eq!(
            TextInputKind::from(NativePvmTextInputKind::ImeCommit),
            TextInputKind::ImeCommit
        );
    }
}
