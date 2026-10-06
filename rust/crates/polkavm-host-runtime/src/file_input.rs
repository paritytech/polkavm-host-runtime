/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

/// Maximum UTF-8 bytes in a guest file-registration descriptor.
pub const MAX_FILE_DESCRIPTOR_BYTES: usize = 4 * 1024;
/// Maximum UTF-8 bytes in a registration identifier.
pub const MAX_FILE_ID_BYTES: usize = 64;
/// Maximum UTF-8 bytes in a user-facing file-type label.
pub const MAX_FILE_LABEL_BYTES: usize = 80;
/// Maximum filename extensions per registration.
pub const MAX_FILE_EXTENSIONS: usize = 16;
/// Maximum ASCII bytes after an extension's leading dot.
pub const MAX_FILE_EXTENSION_BYTES: usize = 16;
/// Maximum MIME types per registration.
pub const MAX_FILE_MIME_TYPES: usize = 16;
/// Maximum ASCII bytes in a MIME type.
pub const MAX_FILE_MIME_TYPE_BYTES: usize = 127;
/// Maximum UTF-8 bytes in a relaunch asset path.
pub const MAX_FILE_MOUNT_PATH_BYTES: usize = 1_024;
/// Longest base name the runtime hands to the guest through `host_file_info`.
pub const MAX_FILE_NAME_BYTES: usize = 1_024;
/// Largest file delivered as a mediated-input byte result.
pub const MAX_INLINE_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Largest file delivered as a fresh execution's asset.
pub const MAX_RELAUNCH_FILE_BYTES: usize = 128 * 1024 * 1024;
/// Largest retained stream representable by ABI 1 offsets.
pub const MAX_STREAM_FILE_BYTES: u32 = u32::MAX;
/// Maximum bytes transferred by one stream or cache operation.
pub const MAX_FILE_READ_BYTES: u32 = 65_536;
/// Aggregate private working-cache reservation for one execution.
pub const MAX_FILE_CACHE_BYTES: u32 = 512 * 1024 * 1024;

/// The handle has no selected source or usable cache.
pub const FILE_READ_INVALID_HANDLE: i32 = -1;
/// The requested offset, size, or cache state is invalid.
pub const FILE_READ_INVALID_RANGE: i32 = -2;
/// The guest memory range is not accessible for the requested transfer.
pub const FILE_READ_INVALID_DESTINATION: i32 = -3;
/// The retained source or private cache failed its I/O operation.
pub const FILE_READ_IO_ERROR: i32 = -4;

/// The host supports file input, but not the requested delivery.
pub const FILE_REGISTER_DELIVERY_UNAVAILABLE: i32 = -4;
/// File metadata was requested for an invalid handle or destination.
pub const FILE_INFO_INVALID: i32 = -1;

/// How a selected file reaches the guest.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileDelivery {
    /// The bytes become a `host_input_read` result of the running execution.
    Inline,
    /// A fresh execution reads the file as the asset at the mount path.
    Relaunch,
    /// The Host retains the file and serves bounded range reads.
    Stream,
}

/// A validated `host_file_register` descriptor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDescriptor {
    /// Stable identifier unique within this execution.
    pub id: String,
    /// User-facing description for the host picker.
    pub label: String,
    /// Lowercase filename extensions including their leading dots.
    pub extensions: Vec<String>,
    /// Lowercase MIME types accepted by the picker.
    pub mime_types: Vec<String>,
    /// How the host delivers the selected file.
    pub delivery: FileDelivery,
    /// Maximum accepted selection size in bytes.
    pub max_bytes: u32,
    /// Asset path used only by relaunch delivery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mount_path: Option<String>,
}

/// File deliveries this Host can serve for one execution.
///
/// A Host that serves no deliveries leaves file input unavailable, so every
/// registration returns `-2`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileInputSupport {
    /// Permit bounded inline byte results.
    pub inline: bool,
    /// Permit fresh-execution asset delivery.
    pub relaunch: bool,
    /// Permit retained read-only range sources.
    pub stream: bool,
    /// The manifest's `runtime.entrypoint`, which no mount path may replace.
    pub entrypoint: String,
}

/// One file registration of the current execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileRegistration {
    /// Execution-local guest registration handle.
    pub handle: u32,
    /// Validated runtime descriptor.
    pub descriptor: FileDescriptor,
    /// Current guest-visible request or selection state.
    pub status: crate::MediatedInputStatus,
}

/// A guest-triggered request for the Host to open its file picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInputRequest {
    /// Registration whose picker the host should present.
    pub handle: u32,
    /// Validated picker and delivery constraints.
    pub descriptor: FileDescriptor,
}

/// A file the user selected in Host UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSelection {
    /// The name the Host received; the runtime strips any path and replaces
    /// control characters before the guest sees it.
    pub name: String,
    /// The lowercase `type/subtype` the Host resolved, or empty.
    pub mime_type: String,
    /// Selected file contents for inline or relaunch delivery.
    pub bytes: Vec<u8>,
}

/// Metadata of a selected stream. The Host retains the file, not its bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileStreamSelection {
    /// Host-received name, sanitized to a bounded base name by the runtime.
    pub name: String,
    /// Lowercase MIME type, or empty when unknown.
    pub mime_type: String,
    /// Exact retained source length, checked against its registration.
    pub size: u64,
}

/// A retained read-only selection. Dropping the source releases its resources.
pub trait FileReadSource: Send {
    /// Length of the retained source in bytes.
    fn size(&self) -> u64;
    /// Fill the complete bounded destination or return an I/O error.
    fn read_exact_at(&mut self, offset: u32, destination: &mut [u8]) -> Result<()>;
}

/// Host-private derived-file storage, independent of the selected source.
///
/// The runtime exposes only bounded sequential writes followed by sealed reads.
/// Dropping the cache must close and delete its temporary backing storage.
pub trait FileCache: FileReadSource {
    /// Discard old contents and reserve exactly this many bytes.
    fn reset(&mut self, size: u32) -> Result<()>;
    /// Write the complete bounded chunk or return an I/O error.
    fn write_exact_at(&mut self, offset: u32, bytes: &[u8]) -> Result<()>;
    /// Flush completed writes before the runtime seals the cache.
    fn flush(&mut self) -> Result<()>;
}

pub(crate) struct SelectedFileCache {
    pub(crate) backend: Box<dyn FileCache>,
    pub(crate) size: u32,
    pub(crate) written: u32,
    pub(crate) sealed: bool,
}

impl SelectedFileCache {
    pub(crate) fn new(backend: Box<dyn FileCache>) -> Self {
        Self {
            backend,
            size: 0,
            written: 0,
            sealed: false,
        }
    }
}

pub(crate) struct SelectedFileStream {
    pub(crate) source: Box<dyn FileReadSource>,
    pub(crate) cache: Option<SelectedFileCache>,
}

/// The outcome of delivering a selected file to a registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileInputDelivery {
    /// The inline bytes or stream selection are ready (status 3).
    Ready,
    /// The file is empty or above `maxBytes`; the registration reports
    /// status 6 and no bytes reach the guest.
    Rejected,
    /// The registration is neither idle nor the active request, or another
    /// request is active; nothing changed.
    Refused,
    /// The execution stopped; start a fresh one with this file mounted.
    Relaunch(FileRelaunch),
}

/// A relaunch-delivered file to mount in a fresh execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileRelaunch {
    /// The registration `id` whose re-registration reports status 3.
    pub id: String,
    /// The asset path the file replaces.
    pub mount_path: String,
    /// The sanitized base name reported by `host_file_info`.
    pub name: String,
    /// Lowercase MIME type, or empty when unknown.
    pub mime_type: String,
    /// Selected file contents to mount before initialization.
    pub bytes: Vec<u8>,
}

/// What `host_file_info` reports about a selected file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileInfo {
    pub(crate) name: String,
    pub(crate) mime_type: String,
    pub(crate) size: u64,
}

impl FileInfo {
    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("file info serializes")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawDescriptor {
    id: String,
    label: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    mime_types: Vec<String>,
    delivery: FileDelivery,
    max_bytes: u64,
    #[serde(default, deserialize_with = "present_string")]
    mount_path: Option<String>,
}

/// Rejects an explicit `null`: an optional field is either absent or a string.
pub(crate) fn present_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

/// Parses one descriptor, enforcing every rule that does not depend on the
/// execution's other registrations.
pub(crate) fn parse_descriptor(bytes: &[u8]) -> Option<FileDescriptor> {
    if bytes.len() > MAX_FILE_DESCRIPTOR_BYTES {
        return None;
    }
    // A struct also deserializes from a JSON array; only an object is a descriptor.
    if bytes
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        != Some(&b'{')
    {
        return None;
    }
    let raw: RawDescriptor = serde_json::from_slice(bytes).ok()?;
    let max_bytes = u32::try_from(raw.max_bytes).ok()?;
    let bound = match raw.delivery {
        FileDelivery::Inline => MAX_INLINE_FILE_BYTES,
        FileDelivery::Relaunch => MAX_RELAUNCH_FILE_BYTES,
        FileDelivery::Stream => MAX_STREAM_FILE_BYTES as usize,
    };
    let mount_path_valid = match (raw.delivery, &raw.mount_path) {
        (FileDelivery::Inline | FileDelivery::Stream, None) => true,
        (FileDelivery::Relaunch, Some(path)) => valid_mount_path(path),
        _ => false,
    };
    let valid = valid_id(&raw.id)
        && FileDescriptor::valid_type_filter(&raw.label, &raw.extensions, &raw.mime_types)
        && (1..=bound).contains(&(max_bytes as usize))
        && mount_path_valid;
    valid.then_some(FileDescriptor {
        id: raw.id,
        label: raw.label,
        extensions: raw.extensions,
        mime_types: raw.mime_types,
        delivery: raw.delivery,
        max_bytes,
        mount_path: raw.mount_path,
    })
}

impl FileDescriptor {
    /// The label and type rules shared with the manifest `fileTypes` hint.
    pub(crate) fn valid_type_filter(
        label: &str,
        extensions: &[String],
        mime_types: &[String],
    ) -> bool {
        valid_label(label)
            && unique_within(extensions, MAX_FILE_EXTENSIONS, valid_extension)
            && unique_within(mime_types, MAX_FILE_MIME_TYPES, valid_mime_type)
            && !(extensions.is_empty() && mime_types.is_empty())
    }
}

fn unique_within(values: &[String], limit: usize, valid: fn(&str) -> bool) -> bool {
    values.len() <= limit
        && values.iter().all(|value| valid(value))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

pub(crate) fn valid_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_FILE_ID_BYTES
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

pub(crate) fn valid_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_FILE_LABEL_BYTES
}

pub(crate) fn valid_extension(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix('.') else {
        return false;
    };
    (1..=MAX_FILE_EXTENSION_BYTES).contains(&suffix.len())
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

pub(crate) fn valid_mime_type(value: &str) -> bool {
    let part = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$&^_.+-".contains(&byte)
            })
    };
    value.len() <= MAX_FILE_MIME_TYPE_BYTES
        && value
            .split_once('/')
            .is_some_and(|(kind, subtype)| part(kind) && part(subtype))
}

pub(crate) fn valid_mount_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FILE_MOUNT_PATH_BYTES
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Reduces a Host-received name to the base name the guest may see.
pub(crate) fn sanitize_name(name: &str) -> Result<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let sanitized: String = base
        .chars()
        .map(|character| {
            if character.is_control() {
                char::REPLACEMENT_CHARACTER
            } else {
                character
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized.len() > MAX_FILE_NAME_BYTES {
        bail!("file name must reduce to 1..={MAX_FILE_NAME_BYTES} bytes");
    }
    Ok(sanitized)
}

pub(crate) fn validate_selected_mime_type(mime_type: &str) -> Result<()> {
    if !mime_type.is_empty() && !valid_mime_type(mime_type) {
        return Err(anyhow!("invalid file MIME type {mime_type}"));
    }
    Ok(())
}

impl FileRelaunch {
    pub(crate) fn validate(&self) -> Result<()> {
        if !valid_id(&self.id) || !valid_mount_path(&self.mount_path) {
            bail!("invalid relaunch file registration");
        }
        if sanitize_name(&self.name)? != self.name {
            bail!("relaunch file name is not a sanitized base name");
        }
        validate_selected_mime_type(&self.mime_type)?;
        if self.bytes.is_empty() || self.bytes.len() > MAX_RELAUNCH_FILE_BYTES {
            bail!("relaunch file must contain 1..={MAX_RELAUNCH_FILE_BYTES} bytes");
        }
        Ok(())
    }

    pub(crate) fn info(&self) -> FileInfo {
        FileInfo {
            name: self.name.clone(),
            mime_type: self.mime_type.clone(),
            size: self.bytes.len() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Vector {
        descriptor: String,
        valid: bool,
    }

    #[test]
    fn shared_descriptor_vectors_match_the_contract() {
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("../tests/fixtures/file-descriptors.json")).unwrap();
        for vector in vectors {
            assert_eq!(
                parse_descriptor(vector.descriptor.as_bytes()).is_some(),
                vector.valid,
                "{}",
                vector.descriptor
            );
        }
    }

    #[test]
    fn descriptors_are_bounded_before_parsing() {
        let padding = " ".repeat(MAX_FILE_DESCRIPTOR_BYTES);
        let descriptor = format!(
            r#"{{"id":"a","label":"A","extensions":[".a"],"delivery":"inline","maxBytes":1}}{padding}"#
        );
        assert!(parse_descriptor(&descriptor.as_bytes()[..MAX_FILE_DESCRIPTOR_BYTES]).is_some());
        assert!(parse_descriptor(descriptor.as_bytes()).is_none());
        assert!(parse_descriptor(&[b'{', 0xff, b'}']).is_none());
    }

    #[test]
    fn names_lose_paths_and_control_characters() {
        assert_eq!(
            sanitize_name("C:\\roms/Game\u{7}.sfc").unwrap(),
            "Game\u{fffd}.sfc"
        );
        assert!(sanitize_name("roms/").is_err());
        assert!(sanitize_name(&"a".repeat(MAX_FILE_NAME_BYTES + 1)).is_err());
    }

    #[test]
    fn file_info_uses_the_documented_shape() {
        let info = FileInfo {
            name: "Example \"Game\".sfc".into(),
            mime_type: String::new(),
            size: 1_048_576,
        };
        assert_eq!(
            info.encode(),
            br#"{"name":"Example \"Game\".sfc","mimeType":"","size":1048576}"#
        );
    }
}
