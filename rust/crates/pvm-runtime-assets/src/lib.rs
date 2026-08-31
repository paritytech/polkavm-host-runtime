//! Browser artifacts for the host-neutral PolkaVM runtime.

/// One immutable browser runtime file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserAsset {
    /// Relative export path.
    pub path: &'static str,
    /// HTTP content type.
    pub content_type: &'static str,
    /// File contents.
    pub bytes: &'static [u8],
    /// Lowercase SHA-256 digest.
    pub sha256: &'static str,
}

/// Runtime package version shared by every asset.
pub const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Return the complete browser runtime asset set.
pub fn browser_assets() -> &'static [BrowserAsset] {
    &ASSETS
}

const ASSETS: [BrowserAsset; 7] = [
    BrowserAsset {
        path: "pvm-browser-runtime.wasm",
        content_type: "application/wasm",
        bytes: include_bytes!("../assets/pvm-browser-runtime.wasm"),
        sha256: "3b448399a39ee571765f41266032c621ce3668176d7f0d6372c85d6068e7d4a5",
    },
    BrowserAsset {
        path: "pvm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-worker.js"),
        sha256: "18606f99395a1b1d27ba1341b757b604f0025bfd2daac96b0b50174e562b2b4b",
    },
    BrowserAsset {
        path: "pvm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-gpu-worker.js"),
        sha256: "33418e2c81c117539569eb4cf91af4d058cdfae7dd4556b13f3687f6d6b3bae4",
    },
    BrowserAsset {
        path: "pvm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-wasm-translated.js"),
        sha256: "a4c2a9ed4274c79b0c11ddbac163f1de200d41005ca1ce54909a5b8116d82c46",
    },
    BrowserAsset {
        path: "pvm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-runtime-core.js"),
        sha256: "37a3aae65994fb5c7ad660fbfad8fca774ecd165e8cfc2a982279c9571d21fb1",
    },
    BrowserAsset {
        path: "pvm-wasm-worker-entry.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-wasm-worker-entry.js"),
        sha256: "9c929f5d5c64a1b75e7e48485d7c3944ed6838112177ea778827a2c407c2d820",
    },
    BrowserAsset {
        path: "SHA256SUMS",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/SHA256SUMS"),
        sha256: "69926edad64d5f0826cffc5aa85dde9a0133fd4648b3a87e32129744f69b28c6",
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn embedded_assets_match_their_recorded_digests() {
        let mut paths = HashSet::new();
        for asset in browser_assets() {
            assert!(paths.insert(asset.path), "duplicate asset {}", asset.path);
            let digest = Sha256::digest(asset.bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(digest, asset.sha256, "digest mismatch for {}", asset.path);
        }
    }
}
