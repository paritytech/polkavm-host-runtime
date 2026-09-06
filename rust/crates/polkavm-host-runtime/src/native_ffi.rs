/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{
    ApplicationRuntime, AudioChunk, Frame, GpuBatch, HostFrameResponseError, InputEvent,
    InputEventType, PresentationProfile, TextInputKind, Tri2dFrame, UiOutputFrame,
    UiSemanticsFrame, INPUT_EVENT_BYTES,
};
#[cfg(feature = "native-gpu")]
use crate::{NativeGpuFrame, NativeGpuRenderer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmPresentationProfile {
    Framebuffer,
    Tri2d,
    WebGpuRaster,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmInputEventType {
    KeyDown,
    KeyUp,
    ButtonDown,
    ButtonUp,
    PointerMove,
    PointerDelta,
    SurfaceMetrics,
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
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmTextInputKind {
    Text,
    ImePreedit,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmMotionAvailability {
    Unavailable,
    Available,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmAsset {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmFrame {
    pub width: u32,
    pub height: u32,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmUiSemanticsFrame {
    pub bytes: Vec<u8>,
}

impl From<UiSemanticsFrame> for NativePolkaVmUiSemanticsFrame {
    fn from(frame: UiSemanticsFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmUiOutputFrame {
    pub bytes: Vec<u8>,
}

impl From<UiOutputFrame> for NativePolkaVmUiOutputFrame {
    fn from(frame: UiOutputFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmTri2dFrame {
    pub width: u32,
    pub height: u32,
    pub draw_count: u32,
    pub vertex_count: u32,
    pub index_count: u32,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmAudioChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmGpuBatch {
    pub bytes: Vec<u8>,
}

impl From<GpuBatch> for NativePolkaVmGpuBatch {
    fn from(batch: GpuBatch) -> Self {
        Self { bytes: batch.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmGpuFrame {
    pub width: u32,
    pub height: u32,
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

#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum NativePolkaVmError {
    #[error("{detail}")]
    Runtime { detail: String },
    #[error("PolkaVM runtime is stopped")]
    Stopped,
    #[error("host-frame response queue is full")]
    HostFrameResponseQueueFull,
    #[error("asset path appears more than once: {path}")]
    DuplicateAsset { path: String },
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

    pub fn init(&self) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .init()
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn update(&self) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .update()
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn stop(&self) -> Result<(), NativePolkaVmError> {
        self.lock()?.stop();
        Ok(())
    }

    pub fn backend(&self) -> Result<String, NativePolkaVmError> {
        Ok(format!("{:?}", self.lock()?.backend()).to_ascii_lowercase())
    }

    pub fn uses_motion(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_motion())
    }

    pub fn last_gas_used(&self) -> Result<u64, NativePolkaVmError> {
        Ok(self.lock()?.last_gas_used())
    }

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

    pub fn send_text_input(
        &self,
        kind: NativePolkaVmTextInputKind,
        text: String,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_text_input(kind.into(), &text)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn set_motion_availability(
        &self,
        availability: NativePolkaVmMotionAvailability,
    ) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_motion_availability(availability.into());
        Ok(())
    }

    pub fn send_motion_sample(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_motion_sample(&bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn uses_pointer_capture(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_pointer_capture())
    }

    pub fn set_pointer_capture_supported(&self, supported: bool) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_pointer_capture_supported(supported);
        Ok(())
    }

    pub fn set_pointer_capture_active(&self, active: bool) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_pointer_capture_active(active)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn take_pointer_capture_request(&self) -> Result<Option<bool>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_pointer_capture_request())
    }

    pub fn gpu_ready(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock_running()?.gpu_ready())
    }

    pub fn set_gpu_capabilities(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .set_gpu_capabilities(bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn send_gpu_event(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock_running()?
            .send_gpu_event(bytes)
            .map_err(NativePolkaVmError::runtime)
    }

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

    pub fn take_frame(&self) -> Result<Option<NativePolkaVmFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_frame().map(Into::into))
    }

    pub fn take_tri2d(&self) -> Result<Option<NativePolkaVmTri2dFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_tri2d().map(Into::into))
    }

    pub fn take_audio(&self) -> Result<Option<NativePolkaVmAudioChunk>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_audio().map(Into::into))
    }

    pub fn take_gpu_batch(&self) -> Result<Option<NativePolkaVmGpuBatch>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_gpu_batch().map(Into::into))
    }

    pub fn take_host_frame_request(&self) -> Result<Option<Vec<u8>>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_host_frame_request())
    }

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

    pub fn take_ui_semantics(
        &self,
    ) -> Result<Option<NativePolkaVmUiSemanticsFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_ui_semantics().map(Into::into))
    }

    pub fn take_ui_output(&self) -> Result<Option<NativePolkaVmUiOutputFrame>, NativePolkaVmError> {
        Ok(self.lock_running()?.take_ui_output().map(Into::into))
    }

    pub fn take_log(&self) -> Result<Option<String>, NativePolkaVmError> {
        Ok(self.lock()?.take_log())
    }

    pub fn is_exited(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.is_exited())
    }

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
