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
        sha256: "182c471bd62e3279abe996f33d8e9ccfded4de7e6b4a18470cad1e67f1e074f3",
    },
    BrowserAsset {
        path: "pvm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-worker.js"),
        sha256: "c25dbaef94f91dcdfe33185fc58485fdc128c5f9edde11daa23f94fdb6464d18",
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
        sha256: "865bfe48c8e01c18a93d40f868afd8b21e2be2ec84e0c993faa54470480abd3c",
    },
    BrowserAsset {
        path: "pvm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-runtime-core.js"),
        sha256: "51891fa4bd0a513dd7ad14b82c4a9f6cbb880755f93612fa4d4ebecd1bcc8dcb",
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
        sha256: "2d73c779a18c2d07eb3b276021d495f0f37b7ed559738fd6660643969989f0a2",
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
