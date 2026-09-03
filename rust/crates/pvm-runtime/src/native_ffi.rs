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

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmPresentationProfile {
    Framebuffer,
    Tri2d,
    WebGpuRaster,
}

impl From<NativePvmPresentationProfile> for PresentationProfile {
    fn from(value: NativePvmPresentationProfile) -> Self {
        match value {
            NativePvmPresentationProfile::Framebuffer => Self::Framebuffer,
            NativePvmPresentationProfile::Tri2d => Self::Tri2d,
            NativePvmPresentationProfile::WebGpuRaster => Self::WebGpuRaster,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmInputEventType {
    KeyDown,
    KeyUp,
    ButtonDown,
    ButtonUp,
    PointerMove,
    PointerDelta,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmTextInputKind {
    Text,
    ImePreedit,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePvmMotionAvailability {
    Unavailable,
    Available,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmAsset {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmFrame {
    pub width: u32,
    pub height: u32,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmUiSemanticsFrame {
    pub bytes: Vec<u8>,
}

impl From<UiSemanticsFrame> for NativePvmUiSemanticsFrame {
    fn from(frame: UiSemanticsFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmUiOutputFrame {
    pub bytes: Vec<u8>,
}

impl From<UiOutputFrame> for NativePvmUiOutputFrame {
    fn from(frame: UiOutputFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmTri2dFrame {
    pub width: u32,
    pub height: u32,
    pub draw_count: u32,
    pub vertex_count: u32,
    pub index_count: u32,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmAudioChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmGpuBatch {
    pub bytes: Vec<u8>,
}

impl From<GpuBatch> for NativePvmGpuBatch {
    fn from(batch: GpuBatch) -> Self {
        Self { bytes: batch.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePvmGpuFrame {
    pub width: u32,
    pub height: u32,
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

#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum NativePvmError {
    #[error("{detail}")]
    Runtime { detail: String },
    #[error("asset path appears more than once: {path}")]
    DuplicateAsset { path: String },
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

    pub fn init(&self) -> Result<(), NativePvmError> {
        self.lock()?.init().map_err(NativePvmError::runtime)
    }

    pub fn update(&self) -> Result<(), NativePvmError> {
        self.lock()?.update().map_err(NativePvmError::runtime)
    }

    pub fn backend(&self) -> Result<String, NativePvmError> {
        Ok(format!("{:?}", self.lock()?.backend()).to_ascii_lowercase())
    }

    pub fn uses_motion(&self) -> Result<bool, NativePvmError> {
        Ok(self.lock()?.uses_motion())
    }

    pub fn last_gas_used(&self) -> Result<u64, NativePvmError> {
        Ok(self.lock()?.last_gas_used())
    }

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

    pub fn send_text_input(
        &self,
        kind: NativePvmTextInputKind,
        text: String,
    ) -> Result<(), NativePvmError> {
        self.lock()?
            .send_text_input(kind.into(), &text)
            .map_err(NativePvmError::runtime)
    }

    pub fn set_motion_availability(
        &self,
        availability: NativePvmMotionAvailability,
    ) -> Result<(), NativePvmError> {
        self.lock()?.set_motion_availability(availability.into());
        Ok(())
    }

    pub fn send_motion_sample(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        self.lock()?
            .send_motion_sample(&bytes)
            .map_err(NativePvmError::runtime)
    }

    pub fn gpu_ready(&self) -> Result<bool, NativePvmError> {
        Ok(self.lock()?.gpu_ready())
    }

    pub fn set_gpu_capabilities(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        self.lock()?
            .set_gpu_capabilities(bytes)
            .map_err(NativePvmError::runtime)
    }

    pub fn send_gpu_event(&self, bytes: Vec<u8>) -> Result<(), NativePvmError> {
        self.lock()?
            .send_gpu_event(bytes)
            .map_err(NativePvmError::runtime)
    }

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

    pub fn take_frame(&self) -> Result<Option<NativePvmFrame>, NativePvmError> {
        Ok(self.lock()?.take_frame().map(Into::into))
    }

    pub fn take_tri2d(&self) -> Result<Option<NativePvmTri2dFrame>, NativePvmError> {
        Ok(self.lock()?.take_tri2d().map(Into::into))
    }

    pub fn take_audio(&self) -> Result<Option<NativePvmAudioChunk>, NativePvmError> {
        Ok(self.lock()?.take_audio().map(Into::into))
    }

    pub fn take_gpu_batch(&self) -> Result<Option<NativePvmGpuBatch>, NativePvmError> {
        Ok(self.lock()?.take_gpu_batch().map(Into::into))
    }

    pub fn take_ui_semantics(&self) -> Result<Option<NativePvmUiSemanticsFrame>, NativePvmError> {
        Ok(self.lock()?.take_ui_semantics().map(Into::into))
    }

    pub fn take_ui_output(&self) -> Result<Option<NativePvmUiOutputFrame>, NativePvmError> {
        Ok(self.lock()?.take_ui_output().map(Into::into))
    }

    pub fn take_log(&self) -> Result<Option<String>, NativePvmError> {
        Ok(self.lock()?.take_log())
    }

    pub fn is_exited(&self) -> Result<bool, NativePvmError> {
        Ok(self.lock()?.is_exited())
    }

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
