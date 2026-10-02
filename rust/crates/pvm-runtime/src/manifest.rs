/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::PresentationProfile;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// Validated host-facing description of a strict App manifest v2 executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppDescriptor {
    /// Semantic application version.
    pub app_version: Vec<u32>,
    /// PolkaVM entrypoint within the verified application archive.
    pub program_path: String,
    /// Selected presentation profile.
    pub presentation: PresentationProfile,
    /// Whether the application may submit audio.
    pub audio_enabled: bool,
    /// Required device-input features.
    pub input_features: Vec<String>,
    /// Required WebGPU limits, empty for other profiles.
    pub gpu_limits: BTreeMap<String, u64>,
    /// File types the application can receive as consented launch assets.
    pub file_input_handlers: Vec<FileInputHandler>,
}

/// A validated registration used by a Host to route a selected file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInputHandler {
    /// Stable handler identifier within the product.
    pub id: String,
    /// User-facing description shown by the Host consent prompt.
    pub label: String,
    /// Lowercase filename extensions including their leading dot.
    pub extensions: Vec<String>,
    /// Lowercase MIME types, when the format has a reliable media type.
    pub media_types: Vec<String>,
    /// Maximum accepted file size.
    pub max_bytes: u64,
    /// Launch-asset path at which the Host delivers the selected bytes.
    pub mount_path: String,
}

impl FileInputHandler {
    /// Returns whether cheap file metadata makes this handler a routing candidate.
    ///
    /// The application remains responsible for validating the delivered bytes.
    pub fn accepts(&self, file_name: &str, media_type: Option<&str>, byte_len: u64) -> bool {
        if byte_len > self.max_bytes {
            return false;
        }
        let extension = file_name
            .rsplit(['/', '\\'])
            .next()
            .and_then(|name| name.rfind('.').map(|offset| &name[offset..]));
        let extension_matches = extension.is_some_and(|value| {
            self.extensions
                .iter()
                .any(|accepted| accepted.eq_ignore_ascii_case(value))
        });
        let media_type_matches =
            media_type
                .filter(|value| !value.is_empty())
                .is_some_and(|value| {
                    self.media_types
                        .iter()
                        .any(|accepted| accepted.eq_ignore_ascii_case(value))
                });
        extension_matches || media_type_matches
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "$v")]
    version: u32,
    kind: String,
    #[serde(rename = "appVersion")]
    app_version: Vec<u32>,
    runtime: Runtime,
    capabilities: Capabilities,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    kind: String,
    #[serde(rename = "abiVersion")]
    abi_version: u32,
    entrypoint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capabilities {
    graphics: Graphics,
    #[serde(rename = "deviceInput")]
    device_input: Option<DeviceInput>,
    audio: Option<Audio>,
    #[serde(rename = "fileInput")]
    file_input: Option<FileInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Graphics {
    #[serde(rename = "abiVersion")]
    abi_version: u32,
    profile: String,
    #[serde(rename = "requiredFeatures", default)]
    required_features: Vec<String>,
    #[serde(rename = "requiredLimits", default)]
    required_limits: BTreeMap<String, u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceInput {
    #[serde(rename = "abiVersion")]
    abi_version: u32,
    #[serde(rename = "requiredFeatures", default)]
    required_features: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audio {
    #[serde(rename = "abiVersion")]
    abi_version: u32,
    #[serde(rename = "requiredFeatures", default)]
    required_features: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileInput {
    #[serde(rename = "abiVersion")]
    abi_version: u32,
    handlers: Vec<FileInputRegistration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileInputRegistration {
    id: String,
    label: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(rename = "mediaTypes", default)]
    media_types: Vec<String>,
    #[serde(rename = "maxBytes")]
    max_bytes: u64,
    #[serde(rename = "mountPath")]
    mount_path: String,
}

const GPU_LIMITS: &[&str] = &[
    "maxTextureDimension2D",
    "maxBufferSize",
    "maxBindingsPerBindGroup",
    "maxBindGroups",
    "maxVertexBuffers",
    "maxVertexAttributes",
    "maxColorAttachments",
    "maxStorageBufferBindingSize",
    "maxStorageBuffersPerShaderStage",
    "maxComputeWorkgroupStorageSize",
    "maxComputeInvocationsPerWorkgroup",
    "maxComputeWorkgroupSizeX",
    "maxComputeWorkgroupSizeY",
    "maxComputeWorkgroupSizeZ",
    "maxComputeWorkgroupsPerDimension",
];
fn validate_file_inputs(
    input: Option<FileInput>,
    program_path: &str,
) -> Result<Vec<FileInputHandler>> {
    let Some(input) = input else {
        return Ok(Vec::new());
    };
    if input.abi_version != 1 {
        bail!("file input capability must use ABI version 1");
    }
    if input.handlers.is_empty() || input.handlers.len() > 16 {
        bail!("file input capability must declare 1..=16 handlers");
    }
    let mut ids = BTreeSet::new();
    let mut mount_paths = BTreeSet::new();
    let mut handlers = Vec::with_capacity(input.handlers.len());
    for handler in input.handlers {
        if handler.id.is_empty()
            || handler.id.len() > 64
            || !handler
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !handler.id.as_bytes()[0].is_ascii_alphanumeric()
            || !handler.id.as_bytes()[handler.id.len() - 1].is_ascii_alphanumeric()
            || !ids.insert(handler.id.clone())
        {
            bail!("file input handler has an invalid or duplicate id");
        }
        if handler.label.trim().is_empty() || handler.label.len() > 80 {
            bail!("file input handler {} has an invalid label", handler.id);
        }
        if handler.extensions.is_empty() && handler.media_types.is_empty() {
            bail!(
                "file input handler {} declares no accepted type",
                handler.id
            );
        }
        let mut extensions = BTreeSet::new();
        for extension in &handler.extensions {
            if extension.len() < 2
                || extension.len() > 17
                || !extension.starts_with('.')
                || !extension[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                || !extensions.insert(extension)
            {
                bail!(
                    "file input handler {} has an invalid or duplicate extension",
                    handler.id
                );
            }
        }
        let mut media_types = BTreeSet::new();
        for media_type in &handler.media_types {
            let valid_token = |token: &str| {
                !token.is_empty()
                    && token.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(
                                byte,
                                b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                            )
                    })
            };
            if media_type.len() > 127
                || !media_type
                    .split_once('/')
                    .is_some_and(|(kind, subtype)| valid_token(kind) && valid_token(subtype))
                || !media_types.insert(media_type)
            {
                bail!(
                    "file input handler {} has an invalid or duplicate media type",
                    handler.id
                );
            }
        }
        if handler.max_bytes == 0 || handler.max_bytes > crate::MAX_ASSET_FILE_BYTES as u64 {
            bail!(
                "file input handler {} has an invalid maximum size",
                handler.id
            );
        }
        validate_path(&handler.mount_path)?;
        if handler.mount_path == program_path || !mount_paths.insert(handler.mount_path.clone()) {
            bail!(
                "file input handler {} has an invalid or duplicate mount path",
                handler.id
            );
        }
        handlers.push(FileInputHandler {
            id: handler.id,
            label: handler.label,
            extensions: handler.extensions,
            media_types: handler.media_types,
            max_bytes: handler.max_bytes,
            mount_path: handler.mount_path,
        });
    }
    Ok(handlers)
}

impl AppDescriptor {
    /// Parses and validates an embedded App v2 manifest after proving it is
    /// byte-identical to the executable record resolved from DotNS.
    pub fn parse_exact(embedded: &[u8], executable_record: &[u8]) -> Result<Self> {
        if embedded != executable_record {
            bail!("embedded App manifest differs from executable record");
        }
        let manifest: Manifest =
            serde_json::from_slice(embedded).context("parse strict App manifest v2")?;
        if manifest.version != 2 || manifest.kind != "app" {
            bail!("manifest must be $v 2 kind app");
        }
        if !(manifest.app_version.len() == 3 || manifest.app_version.len() == 4) {
            bail!("App version must contain three or four components");
        }
        if manifest.runtime.kind != "polkavm" || manifest.runtime.abi_version != 1 {
            bail!("App runtime must be PolkaVM ABI version 1");
        }
        validate_path(&manifest.runtime.entrypoint)?;
        if !manifest.runtime.entrypoint.ends_with(".polkavm") {
            bail!("PolkaVM entrypoint must end in .polkavm");
        }
        if manifest.capabilities.graphics.abi_version != 1 {
            bail!("graphics capability must use ABI version 1");
        }
        if !manifest.capabilities.graphics.required_features.is_empty() {
            bail!("graphics profile requests unsupported optional features");
        }
        let presentation = PresentationProfile::parse(&manifest.capabilities.graphics.profile)?;
        let gpu_limits = manifest.capabilities.graphics.required_limits;
        if presentation.supports_gpu() {
            for (name, value) in &gpu_limits {
                if !GPU_LIMITS.contains(&name.as_str()) || *value == 0 {
                    return Err(anyhow!("unsupported WebGPU required limit {name}"));
                }
            }
        } else if !gpu_limits.is_empty() {
            bail!("non-WebGPU graphics profile declares required limits");
        }
        let input_features = if let Some(input) = manifest.capabilities.device_input {
            if input.abi_version != 1 {
                bail!("device input capability must use ABI version 1");
            }
            for feature in &input.required_features {
                if feature != "pointer" && feature != "keyboard" && feature != "motion" {
                    bail!("unsupported device input feature {feature}");
                }
            }
            input.required_features
        } else {
            Vec::new()
        };
        let audio_enabled = if let Some(audio) = manifest.capabilities.audio {
            if audio.abi_version != 1 || !audio.required_features.is_empty() {
                bail!("audio capability requires unsupported features or ABI");
            }
            true
        } else {
            false
        };
        let file_input_handlers = validate_file_inputs(
            manifest.capabilities.file_input,
            &manifest.runtime.entrypoint,
        )?;
        Ok(Self {
            app_version: manifest.app_version,
            program_path: manifest.runtime.entrypoint,
            presentation,
            audio_enabled,
            input_features,
            gpu_limits,
            file_input_handlers,
        })
    }
}

pub(crate) fn validate_path(path: &str) -> Result<()> {
    if path.len() > crate::MAX_ASSET_NAME_BYTES
        || path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        bail!("invalid application asset path {path}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AppDescriptor;
    use crate::PresentationProfile;

    const FRAMEBUFFER: &[u8] = br#"{"$v":2,"kind":"app","appVersion":[1,2,3],"runtime":{"kind":"polkavm","abiVersion":1,"entrypoint":"app.polkavm"},"capabilities":{"graphics":{"abiVersion":1,"profile":"framebuffer","requiredFeatures":[]},"deviceInput":{"abiVersion":1,"requiredFeatures":["pointer","keyboard"]},"audio":{"abiVersion":1,"requiredFeatures":[]}}}"#;
    const MINIMAL: &[u8] = br#"{"$v":2,"kind":"app","appVersion":[1,2,3],"runtime":{"kind":"polkavm","abiVersion":1,"entrypoint":"app.polkavm"},"capabilities":{"graphics":{"abiVersion":1,"profile":"tri2d"},"deviceInput":{"abiVersion":1},"audio":{"abiVersion":1}}}"#;
    const MOTION: &[u8] = br#"{"$v":2,"kind":"app","appVersion":[1,2,3],"runtime":{"kind":"polkavm","abiVersion":1,"entrypoint":"app.polkavm"},"capabilities":{"graphics":{"abiVersion":1,"profile":"framebuffer","requiredFeatures":[]},"deviceInput":{"abiVersion":1,"requiredFeatures":["pointer","motion"]}}}"#;
    const FILE_INPUT: &[u8] = br#"{"$v":2,"kind":"app","appVersion":[1,2,3],"runtime":{"kind":"polkavm","abiVersion":1,"entrypoint":"app.polkavm"},"capabilities":{"graphics":{"abiVersion":1,"profile":"framebuffer"},"fileInput":{"abiVersion":1,"handlers":[{"id":"snes-rom","label":"SNES cartridge image","extensions":[".sfc",".smc"],"mediaTypes":["application/x-snes-rom"],"maxBytes":16777216,"mountPath":"game/cartridge.sfc"}]}}}"#;

    #[test]
    fn omitted_required_features_default_to_empty() {
        let descriptor = AppDescriptor::parse_exact(MINIMAL, MINIMAL).unwrap();
        assert_eq!(descriptor.presentation, PresentationProfile::Tri2d);
        assert!(descriptor.input_features.is_empty());
        assert!(descriptor.audio_enabled);
    }

    #[test]
    fn accepts_required_motion_input() {
        let descriptor = AppDescriptor::parse_exact(MOTION, MOTION).unwrap();
        assert_eq!(descriptor.input_features, ["pointer", "motion"]);
    }

    #[test]
    fn parses_exact_strict_manifest() {
        let descriptor = AppDescriptor::parse_exact(FRAMEBUFFER, FRAMEBUFFER).unwrap();
        assert_eq!(descriptor.presentation, PresentationProfile::Framebuffer);
        assert_eq!(descriptor.program_path, "app.polkavm");
        assert!(descriptor.audio_enabled);
    }

    #[test]
    fn rejects_external_byte_mismatch() {
        let mut changed = FRAMEBUFFER.to_vec();
        changed.push(b'\n');
        assert!(AppDescriptor::parse_exact(FRAMEBUFFER, &changed).is_err());
    }

    #[test]
    fn parses_and_matches_registered_file_inputs() {
        let descriptor = AppDescriptor::parse_exact(FILE_INPUT, FILE_INPUT).unwrap();
        let handler = &descriptor.file_input_handlers[0];
        assert_eq!(handler.id, "snes-rom");
        assert_eq!(handler.mount_path, "game/cartridge.sfc");
        assert!(handler.accepts("GAME.SFC", None, 524_288));
        assert!(handler.accepts("download", Some("application/x-snes-rom"), 524_288));
        assert!(!handler.accepts("game.nes", None, 524_288));
        assert!(!handler.accepts("game.sfc", None, 16_777_217));
    }

    #[test]
    fn rejects_unsafe_file_input_delivery_paths() {
        let unsafe_path = String::from_utf8(FILE_INPUT.to_vec())
            .unwrap()
            .replace("game/cartridge.sfc", "../cartridge.sfc");
        assert!(
            AppDescriptor::parse_exact(unsafe_path.as_bytes(), unsafe_path.as_bytes()).is_err()
        );
    }

    #[test]
    fn rejects_legacy_and_unknown_fields() {
        let legacy = FRAMEBUFFER
            .windows(6)
            .position(|window| window == b"\"$v\":2")
            .map(|offset| {
                let mut bytes = FRAMEBUFFER.to_vec();
                bytes[offset + 5] = b'1';
                bytes
            })
            .unwrap();
        assert!(AppDescriptor::parse_exact(&legacy, &legacy).is_err());
        let unknown = String::from_utf8(FRAMEBUFFER.to_vec())
            .unwrap()
            .replace("\"kind\":\"app\"", "\"kind\":\"app\",\"modalities\":{}");
        assert!(AppDescriptor::parse_exact(unknown.as_bytes(), unknown.as_bytes()).is_err());
    }
}
