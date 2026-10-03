/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Browser-only Wasm ABI. Unless documented otherwise, status operations return
//! zero on success and one on failure; error accessors expose the last diagnostic
//! as UTF-8 bytes.
//!
//! Output pointers are read-only offsets into this module's linear memory, valid
//! until the corresponding output is replaced, cleared, or reset. Staging is the
//! only writable region exposed to callers and must be filled before consumption.
//! JavaScript must refresh memory views after calls that can grow memory.
//!
//! SAFETY: the `polkavm_browser_*` export names are unique in this Wasm module.
//! Individual allowances permit their required unmangled symbols; other code
//! remains subject to the workspace unsafe-code policy.

use crate::{
    keyboard_insets_records, safe_area_insets_records, ApplicationRuntime, AudioChunk, Frame,
    GpuBatch, InputEvent, InputEventType, MediatedInputCommand, MediatedInputStatus,
    PresentationProfile, Tri2dFrame, UiOutputFrame, UiSemanticsFrame, INPUT_EVENT_BYTES,
    INPUT_KEYBOARD_INSETS, INPUT_SAFE_AREA_INSETS, MAX_ASSET_BYTES, MAX_ASSET_FILES,
    MAX_ASSET_FILE_BYTES, MAX_PROGRAM_BYTES, UPDATE_AFTER_IDLE,
};
use anyhow::{anyhow, Result};
use polkavm::BackendKind;
use std::cell::RefCell;
use std::collections::HashMap;

const MAX_ASSET_NAME_BYTES: usize = 1_024;
const MAX_STAGING_BYTES: usize = MAX_ASSET_FILE_BYTES + MAX_ASSET_NAME_BYTES;
const TRANSLATION_LIMITS: polkavm_wasm_compiler::Limits = polkavm_wasm_compiler::Limits {
    max_program_bytes: MAX_PROGRAM_BYTES,
    max_rw_data_bytes: crate::MAX_GUEST_RW_DATA_BYTES,
    max_stack_bytes: crate::MAX_GUEST_STACK_BYTES,
    max_heap_bytes: crate::MAX_GUEST_HEAP_BYTES,
};

// SAFETY: the browser host provides these synchronous imports. random_fill writes
// only the supplied region and must not retain the pointer or re-enter the runtime.
#[allow(unsafe_code)]
#[link(wasm_import_module = "polkavm_browser")]
unsafe extern "C" {
    #[link_name = "clock_wall_ms"]
    fn browser_clock_wall_ms() -> f64;
    #[link_name = "random_fill"]
    fn browser_random_fill(pointer: *mut u8, length: usize) -> i32;
}

#[allow(unsafe_code)] // Narrow boundary to the synchronous browser clock import.
pub(crate) fn wall_clock_ns() -> u64 {
    // SAFETY: the clock import takes no pointers and does not re-enter the runtime.
    let milliseconds = unsafe { browser_clock_wall_ms() };
    if !milliseconds.is_finite() || milliseconds < 0.0 {
        return 0;
    }
    (milliseconds as u64).saturating_mul(1_000_000)
}

#[allow(unsafe_code)] // Narrow boundary to the synchronous browser random import.
pub(crate) fn fill_random(bytes: &mut [u8]) -> i32 {
    // SAFETY: bytes is exclusively borrowed and valid for the synchronous call.
    unsafe { browser_random_fill(bytes.as_mut_ptr(), bytes.len()) }
}

struct Launch {
    program: Vec<u8>,
    assets: HashMap<String, Vec<u8>>,
    asset_bytes: usize,
    max_gas_per_update: u64,
    audio_enabled: bool,
    presentation: PresentationProfile,
}

enum Phase {
    Empty,
    Building(Launch),
    Running(ApplicationRuntime),
}

struct BrowserHost {
    phase: Phase,
    staging: Vec<u8>,
    frame: Option<Frame>,
    tri2d: Option<Tri2dFrame>,
    ui_semantics: Option<UiSemanticsFrame>,
    ui_output: Option<UiOutputFrame>,
    gpu_batch: Option<GpuBatch>,
    audio: Option<AudioChunk>,
    host_frame_request: Option<Vec<u8>>,
    mediated_input_command: Option<MediatedInputCommand>,
    log: Option<String>,
    save: Option<Vec<u8>>,
    translation: Vec<u8>,
    error: String,
}

impl BrowserHost {
    fn new() -> Self {
        Self {
            phase: Phase::Empty,
            staging: Vec::new(),
            frame: None,
            tri2d: None,
            ui_semantics: None,
            ui_output: None,
            gpu_batch: None,
            audio: None,
            host_frame_request: None,
            mediated_input_command: None,
            log: None,
            save: None,
            translation: Vec::new(),
            error: String::new(),
        }
    }

    fn running(&mut self) -> Result<&mut ApplicationRuntime> {
        match &mut self.phase {
            Phase::Running(runtime) => Ok(runtime),
            _ => Err(anyhow!("PolkaVM browser runtime is not running")),
        }
    }

    fn clear_outputs(&mut self) {
        self.frame = None;
        self.tri2d = None;
        self.ui_semantics = None;
        self.ui_output = None;
        self.gpu_batch = None;
        self.host_frame_request = None;
        self.mediated_input_command = None;
        self.audio = None;
        self.log = None;
        self.save = None;
    }
}

thread_local! {
    static HOST: RefCell<BrowserHost> = RefCell::new(BrowserHost::new());
}

fn status(operation: impl FnOnce(&mut BrowserHost) -> Result<()>) -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.error.clear();
        match operation(&mut host) {
            Ok(()) => 0,
            Err(error) => {
                host.error = format!("{error:#}");
                1
            }
        }
    })
}

/// Return the browser ABI version.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_abi_version() -> u32 {
    2
}

/// Stop the runtime and invalidate all staging, translation, and output buffers.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_reset() {
    HOST.with(|host| *host.borrow_mut() = BrowserHost::new());
}

/// Allocate bounded writable staging bytes; return their offset, or zero on error.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_staging_reserve(length: u32) -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.error.clear();
        let length = length as usize;
        if length == 0 || length > MAX_STAGING_BYTES {
            host.error = format!("invalid PolkaVM browser staging length {length}");
            host.staging.clear();
            return 0;
        }
        host.staging = vec![0; length];
        host.staging.as_mut_ptr() as usize as u32
    })
}

/// Translate the staged program into a single Wasm module; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_translate_staged() -> u32 {
    status(|host| {
        host.translation = polkavm_wasm_compiler::translate(&host.staging, TRANSLATION_LIMITS)?;
        Ok(())
    })
}

/// Translate the staged program into a root with embedded code parts; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_translate_partitioned_staged() -> u32 {
    status(|host| {
        host.translation =
            polkavm_wasm_compiler::translate_partitioned(&host.staging, TRANSLATION_LIMITS)?;
        Ok(())
    })
}

/// Return the translated root module's read-only memory offset.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_translation_pointer() -> u32 {
    HOST.with(|host| host.borrow().translation.as_ptr() as usize as u32)
}

/// Return the translated root module's length in bytes, including embedded parts.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_translation_length() -> u32 {
    HOST.with(|host| host.borrow().translation.len() as u32)
}

fn launch_begin(max_gas_per_update: u64, audio_enabled: u32, presentation: u32) -> u32 {
    status(|host| {
        if !matches!(host.phase, Phase::Empty) {
            return Err(anyhow!("PolkaVM browser launch is already active"));
        }
        let program = std::mem::take(&mut host.staging);
        if program.is_empty() || program.len() > MAX_PROGRAM_BYTES {
            return Err(anyhow!(
                "PolkaVM browser program must contain 1..={MAX_PROGRAM_BYTES} bytes"
            ));
        }
        if max_gas_per_update == 0 {
            return Err(anyhow!("PolkaVM browser gas budget must be nonzero"));
        }
        if audio_enabled > 1 {
            return Err(anyhow!("invalid PolkaVM browser audio capability"));
        }
        let presentation = match presentation {
            0 => PresentationProfile::Framebuffer,
            1 => PresentationProfile::Tri2d,
            2 => PresentationProfile::WebGpuRaster,
            3 => PresentationProfile::WebGpu,
            _ => return Err(anyhow!("invalid PolkaVM browser presentation profile")),
        };
        host.phase = Phase::Building(Launch {
            program,
            assets: HashMap::new(),
            asset_bytes: 0,
            max_gas_per_update,
            audio_enabled: audio_enabled == 1,
            presentation,
        });
        Ok(())
    })
}

/// Consume the staged program and begin a framebuffer launch; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_launch_begin(max_gas_per_update: u64, audio_enabled: u32) -> u32 {
    launch_begin(max_gas_per_update, audio_enabled, 0)
}

/// Begin launch with profile 0=framebuffer, 1=Tri2D, 2=raster GPU, or 3=full GPU.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_launch_begin_v2(
    max_gas_per_update: u64,
    audio_enabled: u32,
    presentation: u32,
) -> u32 {
    launch_begin(max_gas_per_update, audio_enabled, presentation)
}

/// Consume staged UTF-8 path bytes followed by asset content; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_launch_add_asset(path_length: u32) -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        let path_length = path_length as usize;
        if path_length == 0 || path_length > MAX_ASSET_NAME_BYTES || path_length >= bytes.len() {
            return Err(anyhow!("invalid PolkaVM browser asset path length"));
        }
        let path = std::str::from_utf8(&bytes[..path_length])
            .map_err(|_| anyhow!("PolkaVM browser asset path is not UTF-8"))?;
        if path.contains('\0') || path.contains('\\') {
            return Err(anyhow!("invalid PolkaVM browser asset path"));
        }
        let data = &bytes[path_length..];
        if data.len() > MAX_ASSET_FILE_BYTES {
            return Err(anyhow!("PolkaVM browser asset exceeds size limit"));
        }
        let Phase::Building(launch) = &mut host.phase else {
            return Err(anyhow!("PolkaVM browser launch is not accepting assets"));
        };
        if launch.assets.len() == MAX_ASSET_FILES {
            return Err(anyhow!("PolkaVM browser launch exceeds asset count limit"));
        }
        let asset_bytes = launch
            .asset_bytes
            .checked_add(data.len())
            .ok_or_else(|| anyhow!("PolkaVM browser asset size overflow"))?;
        if asset_bytes > MAX_ASSET_BYTES {
            return Err(anyhow!("PolkaVM browser launch exceeds asset byte limit"));
        }
        if launch.assets.contains_key(path) {
            return Err(anyhow!("duplicate PolkaVM browser asset {path}"));
        }
        launch.assets.insert(path.to_owned(), data.to_vec());
        launch.asset_bytes = asset_bytes;
        Ok(())
    })
}

/// Instantiate the pending launch using the interpreter backend; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_launch_start() -> u32 {
    status(|host| {
        let phase = std::mem::replace(&mut host.phase, Phase::Empty);
        let Phase::Building(launch) = phase else {
            host.phase = phase;
            return Err(anyhow!("PolkaVM browser launch is not ready"));
        };
        let runtime = ApplicationRuntime::new_with_backend(
            &launch.program,
            launch.assets,
            launch.presentation,
            launch.audio_enabled,
            launch.max_gas_per_update,
            BackendKind::Interpreter,
        )?;
        host.phase = Phase::Running(runtime);
        Ok(())
    })
}

/// Consume staged random bytes for the running guest; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_set_random_bytes() -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        host.running()?.set_random_bytes(bytes)
    })
}

/// Return one when the running guest imports motion support, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_uses_motion() -> u32 {
    HOST.with(|host| match &host.borrow().phase {
        Phase::Running(runtime) => u32::from(runtime.uses_motion()),
        _ => 0,
    })
}

/// Return one when the running guest imports pointer capture, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_uses_pointer_capture() -> u32 {
    HOST.with(|host| match &host.borrow().phase {
        Phase::Running(runtime) => u32::from(runtime.uses_pointer_capture()),
        _ => 0,
    })
}

/// Set whether the host supports pointer capture; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_set_pointer_capture_supported(supported: u32) -> u32 {
    status(|host| {
        host.running()?
            .set_pointer_capture_supported(supported != 0);
        Ok(())
    })
}

/// Report whether pointer capture is active; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_set_pointer_capture_active(active: u32) -> u32 {
    status(|host| host.running()?.set_pointer_capture_active(active != 0))
}

/// Returns 0 when the guest asked for nothing, 1 to arm capture, 2 to release.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_pointer_capture_request() -> u32 {
    HOST.with(|host| match &mut host.borrow_mut().phase {
        Phase::Running(runtime) => match runtime.take_pointer_capture_request() {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        },
        _ => 0,
    })
}

/// Set the motion-wire availability value; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_set_motion_availability(availability: u32) -> u32 {
    status(|host| {
        let availability = crate::motion_wire::MotionAvailability::try_from(availability)
            .map_err(|_| anyhow!("invalid motion availability"))?;
        host.running()?.set_motion_availability(availability);
        Ok(())
    })
}

/// Consume a staged motion-wire sample for the guest; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_motion_sample() -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        host.running()?.send_motion_sample(&bytes)
    })
}

/// Consume the staged GPU capability record; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_set_gpu_capabilities() -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        host.running()?.set_gpu_capabilities(bytes)
    })
}

/// Consume a staged GPU event for the guest; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_gpu_event() -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        host.running()?.send_gpu_event(bytes)
    })
}

/// Returns 0 on admission, 1 on a terminal/invalid response, and 2 on retryable backpressure.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_host_frame_response() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.error.clear();
        let bytes = std::mem::take(&mut host.staging);
        let result = match &mut host.phase {
            Phase::Running(runtime) => runtime.send_host_frame_response(bytes),
            _ => Err(crate::HostFrameResponseError::RuntimeStopped),
        };
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error.to_string();
                if error == crate::HostFrameResponseError::QueueFull {
                    2
                } else {
                    1
                }
            }
        }
    })
}

/// Number of accepted responses still waiting for the guest to poll them.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_pending_host_frame_responses() -> u32 {
    HOST.with(|host| match &host.borrow().phase {
        Phase::Running(runtime) => runtime.pending_host_frame_responses() as u32,
        _ => 0,
    })
}

/// Consume staged NUL-separated UTF-8 mediated-input kinds; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_set_mediated_input_kinds() -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| anyhow!("mediated-input kinds are not UTF-8"))?;
        let kinds = text.split('\0').map(str::to_owned).collect::<Vec<_>>();
        host.running()?.set_mediated_input_kinds(&kinds)
    })
}
/// Complete a mediated-input handle, forwarding staged data only for Ready results.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_mediated_input_result(handle: u32, result: u32) -> u32 {
    status(|host| {
        let mut bytes = std::mem::take(&mut host.staging);
        let result = MediatedInputStatus::try_from(result)?;
        if result != MediatedInputStatus::Ready {
            bytes.clear();
        }
        host.running()?
            .send_mediated_input_result(handle, result, bytes)
    })
}

/// Invoke guest initialization; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_init() -> u32 {
    status(|host| host.running()?.init())
}

/// Set elapsed milliseconds and invoke the guest update; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_update(time_ms: f64) -> u32 {
    status(|host| {
        if !time_ms.is_finite() || time_ms < 0.0 {
            return Err(anyhow!("invalid PolkaVM browser timestamp"));
        }
        let runtime = host.running()?;
        runtime.set_time_ms(time_ms.min(u64::MAX as f64) as u64);
        runtime.update()
    })
}

/// Return one when the running guest imports update scheduling, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_uses_update_scheduling() -> u32 {
    HOST.with(|host| match &host.borrow().phase {
        Phase::Running(runtime) => u32::from(runtime.uses_update_scheduling()),
        _ => 0,
    })
}

/// Return the requested update delay, or UPDATE_AFTER_IDLE when idle or unsupported.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_update_after_ms() -> u32 {
    HOST.with(|host| match &host.borrow().phase {
        Phase::Running(runtime) if runtime.uses_update_scheduling() => {
            runtime.update_after_ms().unwrap_or(UPDATE_AFTER_IDLE)
        }
        _ => UPDATE_AFTER_IDLE,
    })
}

/// Release held input and discard retained audio; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_pause_input() -> u32 {
    status(|host| {
        host.running()?.pause_input();
        host.audio = None;
        Ok(())
    })
}

/// Queue a checked compact input event; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_input(event_type: u32, code: u32, x: u32, y: u32) -> u32 {
    status(|host| {
        let event_type = match event_type {
            1 => InputEventType::KeyDown,
            2 => InputEventType::KeyUp,
            3 => InputEventType::ButtonDown,
            4 => InputEventType::ButtonUp,
            5 => InputEventType::PointerMove,
            6 => InputEventType::PointerDelta,
            7 => InputEventType::SurfaceMetrics,
            18 => InputEventType::TouchStart,
            19 => InputEventType::TouchMove,
            20 => InputEventType::TouchEnd,
            21 => InputEventType::TouchCancel,
            _ => return Err(anyhow!("invalid PolkaVM browser input event type")),
        };
        let code = u8::try_from(code).map_err(|_| anyhow!("input code exceeds u8"))?;
        let x = u16::try_from(x).map_err(|_| anyhow!("input x exceeds u16"))?;
        let y = u16::try_from(y).map_err(|_| anyhow!("input y exceeds u16"))?;
        host.running()?.send_input(InputEvent {
            event_type,
            code,
            x,
            y,
        });
        Ok(())
    })
}

/// Consume one staged extended input record; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_input_record() -> u32 {
    status(|host| {
        let bytes = std::mem::take(&mut host.staging);
        let record: [u8; INPUT_EVENT_BYTES] = bytes
            .try_into()
            .map_err(|_| anyhow!("extended input record must contain {INPUT_EVENT_BYTES} bytes"))?;
        host.running()?.send_input_record(record)
    })
}

/// Queue checked safe-area or keyboard inset records; return status.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_send_view_insets(
    event_type: u32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> u32 {
    status(|host| {
        let left = u16::try_from(left).map_err(|_| anyhow!("left inset exceeds u16"))?;
        let top = u16::try_from(top).map_err(|_| anyhow!("top inset exceeds u16"))?;
        let right = u16::try_from(right).map_err(|_| anyhow!("right inset exceeds u16"))?;
        let bottom = u16::try_from(bottom).map_err(|_| anyhow!("bottom inset exceeds u16"))?;
        let records = match u8::try_from(event_type) {
            Ok(INPUT_SAFE_AREA_INSETS) => safe_area_insets_records(left, top, right, bottom),
            Ok(INPUT_KEYBOARD_INSETS) => keyboard_insets_records(left, top, right, bottom),
            _ => return Err(anyhow!("invalid PolkaVM browser inset event type")),
        };
        host.running()?.send_input_records(&records)
    })
}

/// Retain the next framebuffer output; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_frame() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.frame = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_frame(),
            _ => None,
        };
        u32::from(host.frame.is_some())
    })
}

/// Return the retained framebuffer width in pixels, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_frame_width() -> u32 {
    HOST.with(|host| host.borrow().frame.as_ref().map_or(0, |frame| frame.width))
}

/// Return the retained framebuffer height in pixels, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_frame_height() -> u32 {
    HOST.with(|host| host.borrow().frame.as_ref().map_or(0, |frame| frame.height))
}

/// Return the retained framebuffer's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_frame_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .frame
            .as_ref()
            .map_or(0, |frame| frame.argb.as_ptr() as usize as u32)
    })
}

/// Return the retained framebuffer length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_frame_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .frame
            .as_ref()
            .map_or(0, |frame| frame.argb.len() as u32)
    })
}

/// Retain the next Tri2D frame; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_tri2d() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.tri2d = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_tri2d(),
            _ => None,
        };
        u32::from(host.tri2d.is_some())
    })
}

/// Return the retained Tri2D frame's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_tri2d_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .tri2d
            .as_ref()
            .map_or(0, |frame| frame.bytes.as_ptr() as usize as u32)
    })
}

/// Return the retained Tri2D frame length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_tri2d_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .tri2d
            .as_ref()
            .map_or(0, |frame| frame.bytes.len() as u32)
    })
}

/// Retain the next UI semantics frame; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_ui_semantics() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.ui_semantics = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_ui_semantics(),
            _ => None,
        };
        u32::from(host.ui_semantics.is_some())
    })
}

/// Return the retained UI semantics frame's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_ui_semantics_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .ui_semantics
            .as_ref()
            .map_or(0, |frame| frame.bytes.as_ptr() as usize as u32)
    })
}

/// Return the retained UI semantics frame length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_ui_semantics_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .ui_semantics
            .as_ref()
            .map_or(0, |frame| frame.bytes.len() as u32)
    })
}

/// Retain the next UI output frame; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_ui_output() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.ui_output = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_ui_output(),
            _ => None,
        };
        u32::from(host.ui_output.is_some())
    })
}

/// Return the retained UI output frame's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_ui_output_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .ui_output
            .as_ref()
            .map_or(0, |frame| frame.bytes.as_ptr() as usize as u32)
    })
}

/// Return the retained UI output frame length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_ui_output_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .ui_output
            .as_ref()
            .map_or(0, |frame| frame.bytes.len() as u32)
    })
}

/// Retain the next GPU batch; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_gpu_batch() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.gpu_batch = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_gpu_batch(),
            _ => None,
        };
        u32::from(host.gpu_batch.is_some())
    })
}

/// Return the retained GPU batch's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_gpu_batch_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .gpu_batch
            .as_ref()
            .map_or(0, |batch| batch.bytes.as_ptr() as usize as u32)
    })
}

/// Return the retained GPU batch length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_gpu_batch_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .gpu_batch
            .as_ref()
            .map_or(0, |batch| batch.bytes.len() as u32)
    })
}

/// Retain the next host-frame request; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_host_frame_request() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.host_frame_request = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_host_frame_request(),
            _ => None,
        };
        u32::from(host.host_frame_request.is_some())
    })
}

/// Return the retained host-frame request's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_host_frame_request_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .host_frame_request
            .as_ref()
            .map_or(0, |frame| frame.as_ptr() as usize as u32)
    })
}

/// Return the retained host-frame request length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_host_frame_request_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .host_frame_request
            .as_ref()
            .map_or(0, Vec::len) as u32
    })
}

/// Retain the next mediated-input command: zero=none, one=request, two=cancel.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_mediated_input_command() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.mediated_input_command = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_mediated_input_command(),
            _ => None,
        };
        match host.mediated_input_command {
            Some(MediatedInputCommand::Request(_)) => 1,
            Some(MediatedInputCommand::Cancel { .. }) => 2,
            None => 0,
        }
    })
}

/// Return the retained mediated-input command's handle, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_mediated_input_handle() -> u32 {
    HOST.with(|host| match host.borrow().mediated_input_command.as_ref() {
        Some(MediatedInputCommand::Request(request)) => request.handle,
        Some(MediatedInputCommand::Cancel { handle }) => *handle,
        None => 0,
    })
}

/// Return the retained request's maximum result size in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_mediated_input_max_bytes() -> u32 {
    HOST.with(|host| match host.borrow().mediated_input_command.as_ref() {
        Some(MediatedInputCommand::Request(request)) => request.max_bytes,
        _ => 0,
    })
}

/// Return the retained request kind's read-only UTF-8 memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_mediated_input_kind_pointer() -> u32 {
    HOST.with(|host| match host.borrow().mediated_input_command.as_ref() {
        Some(MediatedInputCommand::Request(request)) => request.kind.as_ptr() as usize as u32,
        _ => 0,
    })
}

/// Return the retained request kind's UTF-8 length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_mediated_input_kind_length() -> u32 {
    HOST.with(|host| match host.borrow().mediated_input_command.as_ref() {
        Some(MediatedInputCommand::Request(request)) => request.kind.len() as u32,
        _ => 0,
    })
}

/// Return the retained request media type's read-only UTF-8 offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_mediated_input_media_type_pointer() -> u32 {
    HOST.with(|host| match host.borrow().mediated_input_command.as_ref() {
        Some(MediatedInputCommand::Request(request)) => request.media_type.as_ptr() as usize as u32,
        _ => 0,
    })
}

/// Return the retained request media type's UTF-8 length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_mediated_input_media_type_length() -> u32 {
    HOST.with(|host| match host.borrow().mediated_input_command.as_ref() {
        Some(MediatedInputCommand::Request(request)) => request.media_type.len() as u32,
        _ => 0,
    })
}
/// Retain the next audio chunk; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_audio() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.audio = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_audio(),
            _ => None,
        };
        u32::from(host.audio.is_some())
    })
}

/// Return the retained audio chunk's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_audio_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .audio
            .as_ref()
            .map_or(0, |audio| audio.samples.as_ptr() as usize as u32)
    })
}

/// Return the retained audio chunk's interleaved i16 sample count, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_audio_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .audio
            .as_ref()
            .map_or(0, |audio| audio.samples.len() as u32)
    })
}

/// Return the retained audio chunk's sample rate in Hz, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_audio_sample_rate() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .audio
            .as_ref()
            .map_or(0, |audio| audio.sample_rate)
    })
}

/// Return the retained audio chunk's channel count, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_audio_channels() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .audio
            .as_ref()
            .map_or(0, |audio| audio.channels)
    })
}

/// Retain the next guest log message; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_log() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.log = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_log(),
            _ => None,
        };
        u32::from(host.log.is_some())
    })
}

/// Return the retained log message's read-only UTF-8 memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_log_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .log
            .as_ref()
            .map_or(0, |log| log.as_ptr() as usize as u32)
    })
}

/// Return the retained log message's UTF-8 length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_log_length() -> u32 {
    HOST.with(|host| host.borrow().log.as_ref().map_or(0, |log| log.len() as u32))
}

/// Retain the next save payload; return one if present, otherwise zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_take_save() -> u32 {
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        host.save = match &mut host.phase {
            Phase::Running(runtime) => runtime.take_save(),
            _ => None,
        };
        u32::from(host.save.is_some())
    })
}

/// Return the retained save payload's read-only memory offset, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_save_pointer() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .save
            .as_ref()
            .map_or(0, |save| save.as_ptr() as usize as u32)
    })
}

/// Return the retained save payload length in bytes, or zero.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_save_length() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .save
            .as_ref()
            .map_or(0, |save| save.len() as u32)
    })
}

/// Return the last error's read-only UTF-8 memory offset.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_error_pointer() -> u32 {
    HOST.with(|host| host.borrow().error.as_ptr() as usize as u32)
}

/// Return the last error's UTF-8 length in bytes.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_error_length() -> u32 {
    HOST.with(|host| host.borrow().error.len() as u32)
}

/// Discard retained output buffers without resetting the running guest.
#[allow(unsafe_code)] // Unique Wasm ABI export; see module safety contract.
#[no_mangle]
pub extern "C" fn polkavm_browser_clear_outputs() {
    HOST.with(|host| host.borrow_mut().clear_outputs());
}
