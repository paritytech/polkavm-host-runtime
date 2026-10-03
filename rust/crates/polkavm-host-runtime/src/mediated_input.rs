/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use anyhow::{anyhow, bail, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Maximum UTF-8 bytes in a mediated-input kind token.
pub const MAX_MEDIATED_INPUT_KIND_BYTES: usize = 32;
/// Maximum UTF-8 bytes in a mediated-input media type token.
pub const MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES: usize = 64;
/// Maximum bytes returned by a mediated-input request.
pub const MAX_MEDIATED_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum simultaneous registrations and supported kinds.
pub const MAX_MEDIATED_INPUT_REGISTRATIONS: usize = 8;

/// Registration failed because its arguments were invalid.
pub const MEDIATED_INPUT_REGISTER_INVALID: i32 = -1;
/// Registration failed because the host does not support the kind.
pub const MEDIATED_INPUT_REGISTER_UNAVAILABLE: i32 = -2;
/// Registration failed because the registration or handle quota was exhausted.
pub const MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED: i32 = -3;

/// The request was queued for the host.
pub const MEDIATED_INPUT_TRIGGER_ACCEPTED: u32 = 0;
/// The trigger referred to an unknown registration.
pub const MEDIATED_INPUT_TRIGGER_INVALID_HANDLE: u32 = 1;
/// Another mediated-input request is already active.
pub const MEDIATED_INPUT_TRIGGER_BUSY: u32 = 2;

/// Cancellation was queued for the host.
pub const MEDIATED_INPUT_CANCEL_ACCEPTED: u32 = 0;
/// Cancellation referred to an unknown registration.
pub const MEDIATED_INPUT_CANCEL_INVALID_HANDLE: u32 = 1;
/// Cancellation referred to a registration without an active request.
pub const MEDIATED_INPUT_CANCEL_NOT_ACTIVE: u32 = 2;

/// Guest-visible lifecycle state of a mediated-input registration.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediatedInputStatus {
    /// The handle does not identify a registration.
    Invalid = 0,
    /// Registered and idle, with no result available.
    Registered = 1,
    /// Waiting for the host to complete the request.
    Active = 2,
    /// Result bytes are available for the guest to consume.
    Ready = 3,
    /// The request was cancelled.
    Cancelled = 4,
    /// Host policy or the user denied the request.
    PermissionDenied = 5,
    /// The host could not complete the request.
    Failed = 6,
}

impl TryFrom<u32> for MediatedInputStatus {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            1 => Ok(Self::Registered),
            2 => Ok(Self::Active),
            3 => Ok(Self::Ready),
            4 => Ok(Self::Cancelled),
            5 => Ok(Self::PermissionDenied),
            6 => Ok(Self::Failed),
            _ => Err(anyhow!("invalid mediated-input status {value}")),
        }
    }
}

/// Bounded request for host-mediated input, subject to host consent policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediatedInputRequest {
    /// Registration handle used to complete or cancel this request.
    pub handle: u32,
    /// Registered host-supported input kind.
    pub kind: String,
    /// Requested result media type.
    pub media_type: String,
    /// Maximum number of result bytes accepted by this registration.
    pub max_bytes: u32,
}

/// Pending action that the host must process outside guest execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediatedInputCommand {
    /// Present or otherwise fulfill a consented input request.
    Request(MediatedInputRequest),
    /// Cancel the outstanding host interaction for a registration.
    Cancel {
        /// Registration whose active interaction should be cancelled.
        handle: u32,
    },
}

#[derive(Debug)]
struct Registration {
    kind: String,
    media_type: String,
    max_bytes: usize,
    status: MediatedInputStatus,
    result: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
pub(crate) struct MediatedInputState {
    supported_kinds: BTreeSet<String>,
    registrations: BTreeMap<u32, Registration>,
    commands: VecDeque<MediatedInputCommand>,
    next_handle: u32,
}

impl MediatedInputState {
    pub(crate) fn set_supported_kinds(&mut self, kinds: &[String]) -> Result<()> {
        if kinds.len() > MAX_MEDIATED_INPUT_REGISTRATIONS {
            bail!("too many mediated-input kinds");
        }
        if self
            .registrations
            .values()
            .any(|registration| registration.status == MediatedInputStatus::Active)
        {
            bail!("cannot change mediated-input kinds while a request is active");
        }
        let mut supported = BTreeSet::new();
        for kind in kinds {
            if !valid_token(kind, MAX_MEDIATED_INPUT_KIND_BYTES) {
                bail!("invalid mediated-input kind {kind}");
            }
            if !supported.insert(kind.clone()) {
                bail!("duplicate mediated-input kind {kind}");
            }
        }
        self.supported_kinds = supported;
        self.registrations
            .retain(|_, registration| self.supported_kinds.contains(&registration.kind));
        Ok(())
    }

    pub(crate) fn register(&mut self, kind: String, media_type: String, max_bytes: usize) -> i32 {
        if !valid_token(&kind, MAX_MEDIATED_INPUT_KIND_BYTES)
            || !valid_token(&media_type, MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES)
            || !(1..=MAX_MEDIATED_INPUT_BYTES).contains(&max_bytes)
        {
            return MEDIATED_INPUT_REGISTER_INVALID;
        }
        if !self.supported_kinds.contains(&kind) {
            return MEDIATED_INPUT_REGISTER_UNAVAILABLE;
        }
        if let Some((&handle, _)) = self.registrations.iter().find(|(_, registration)| {
            registration.kind == kind
                && registration.media_type == media_type
                && registration.max_bytes == max_bytes
        }) {
            return handle as i32;
        }
        if self.registrations.len() == MAX_MEDIATED_INPUT_REGISTRATIONS {
            return MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED;
        }
        let Some(handle) = self.allocate_handle() else {
            return MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED;
        };
        self.registrations.insert(
            handle,
            Registration {
                kind,
                media_type,
                max_bytes,
                status: MediatedInputStatus::Registered,
                result: None,
            },
        );
        handle as i32
    }

    pub(crate) fn trigger(&mut self, handle: u32) -> u32 {
        if self
            .registrations
            .values()
            .any(|registration| registration.status == MediatedInputStatus::Active)
        {
            return MEDIATED_INPUT_TRIGGER_BUSY;
        }
        let Some(registration) = self.registrations.get_mut(&handle) else {
            return MEDIATED_INPUT_TRIGGER_INVALID_HANDLE;
        };
        registration.status = MediatedInputStatus::Active;
        registration.result = None;
        self.commands
            .push_back(MediatedInputCommand::Request(MediatedInputRequest {
                handle,
                kind: registration.kind.clone(),
                media_type: registration.media_type.clone(),
                max_bytes: registration.max_bytes as u32,
            }));
        MEDIATED_INPUT_TRIGGER_ACCEPTED
    }

    pub(crate) fn status(&self, handle: u32) -> MediatedInputStatus {
        self.registrations
            .get(&handle)
            .map_or(MediatedInputStatus::Invalid, |registration| {
                registration.status
            })
    }

    pub(crate) fn result(&self, handle: u32) -> Option<&[u8]> {
        self.registrations.get(&handle)?.result.as_deref()
    }

    pub(crate) fn consume_result(&mut self, handle: u32) {
        if let Some(registration) = self.registrations.get_mut(&handle) {
            registration.result = None;
            registration.status = MediatedInputStatus::Registered;
        }
    }

    pub(crate) fn cancel(&mut self, handle: u32) -> u32 {
        let Some(registration) = self.registrations.get_mut(&handle) else {
            return MEDIATED_INPUT_CANCEL_INVALID_HANDLE;
        };
        if registration.status != MediatedInputStatus::Active {
            return MEDIATED_INPUT_CANCEL_NOT_ACTIVE;
        }
        registration.status = MediatedInputStatus::Cancelled;
        registration.result = None;
        self.commands
            .push_back(MediatedInputCommand::Cancel { handle });
        MEDIATED_INPUT_CANCEL_ACCEPTED
    }

    pub(crate) fn take_command(&mut self) -> Option<MediatedInputCommand> {
        self.commands.pop_front()
    }

    pub(crate) fn complete(
        &mut self,
        handle: u32,
        status: MediatedInputStatus,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let registration = self
            .registrations
            .get_mut(&handle)
            .ok_or_else(|| anyhow!("unknown mediated-input handle {handle}"))?;
        if registration.status != MediatedInputStatus::Active {
            bail!("mediated-input handle {handle} is not active");
        }
        match status {
            MediatedInputStatus::Ready => {
                if bytes.is_empty() || bytes.len() > registration.max_bytes {
                    bail!("mediated-input result exceeds the registered bound");
                }
                registration.result = Some(bytes);
            }
            MediatedInputStatus::Cancelled
            | MediatedInputStatus::PermissionDenied
            | MediatedInputStatus::Failed => {
                if !bytes.is_empty() {
                    bail!("mediated-input failure carries unexpected bytes");
                }
                registration.result = None;
            }
            MediatedInputStatus::Invalid
            | MediatedInputStatus::Registered
            | MediatedInputStatus::Active => {
                bail!("invalid terminal mediated-input status")
            }
        }
        registration.status = status;
        Ok(())
    }

    fn allocate_handle(&mut self) -> Option<u32> {
        for _ in 0..=MAX_MEDIATED_INPUT_REGISTRATIONS {
            self.next_handle = self.next_handle.wrapping_add(1).max(1);
            if !self.registrations.contains_key(&self.next_handle)
                && self.next_handle <= i32::MAX as u32
            {
                return Some(self.next_handle);
            }
        }
        None
    }
}

fn valid_token(value: &str, max_bytes: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max_bytes
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'+')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_trigger_completion_and_read_are_bounded() {
        let mut state = MediatedInputState::default();
        state
            .set_supported_kinds(&["camera-ur".to_owned()])
            .unwrap();
        let handle = state.register(
            "camera-ur".to_owned(),
            "x-zklock-authorization".to_owned(),
            4,
        );
        assert!(handle > 0);
        assert_eq!(
            state.trigger(handle as u32),
            MEDIATED_INPUT_TRIGGER_ACCEPTED
        );
        assert_eq!(
            state.take_command(),
            Some(MediatedInputCommand::Request(MediatedInputRequest {
                handle: handle as u32,
                kind: "camera-ur".to_owned(),
                media_type: "x-zklock-authorization".to_owned(),
                max_bytes: 4,
            }))
        );
        assert!(state
            .complete(
                handle as u32,
                MediatedInputStatus::Ready,
                vec![1, 2, 3, 4, 5]
            )
            .is_err());
        state
            .complete(handle as u32, MediatedInputStatus::Ready, vec![1, 2, 3])
            .unwrap();
        assert_eq!(state.result(handle as u32), Some([1, 2, 3].as_slice()));
        state.consume_result(handle as u32);
        assert_eq!(state.result(handle as u32), None);
        assert_eq!(state.status(handle as u32), MediatedInputStatus::Registered);
    }

    #[test]
    fn cancellation_is_forwarded_and_late_results_are_rejected() {
        let mut state = MediatedInputState::default();
        state
            .set_supported_kinds(&["camera-ur".to_owned()])
            .unwrap();
        let handle = state.register("camera-ur".into(), "bytes".into(), 16) as u32;
        assert_eq!(state.trigger(handle), MEDIATED_INPUT_TRIGGER_ACCEPTED);
        state.take_command();
        assert_eq!(state.cancel(handle), MEDIATED_INPUT_CANCEL_ACCEPTED);
        assert_eq!(
            state.take_command(),
            Some(MediatedInputCommand::Cancel { handle })
        );
        assert!(state
            .complete(handle, MediatedInputStatus::Ready, vec![1])
            .is_err());
    }
}
