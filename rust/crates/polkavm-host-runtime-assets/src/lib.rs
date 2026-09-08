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

const ASSETS: [BrowserAsset; 8] = [
    BrowserAsset {
        path: "polkavm-browser-runtime.wasm",
        content_type: "application/wasm",
        bytes: include_bytes!("../assets/polkavm-browser-runtime.wasm"),
        sha256: "9a436e15682014df906182a12041792c85c0d5f692bd4c5632a9a9d160792a90",
    },
    BrowserAsset {
        path: "polkavm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-worker.js"),
        sha256: "cfd5237a988c07bf81bbfe81ec5ae8e79c5490e04ec010509a52d3a064c9bc38",
    },
    BrowserAsset {
        path: "polkavm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-gpu-worker.js"),
        sha256: "57f084ba804d48f2af5dea415aedade936119fe97c1d25f8525e0a4e382a9854",
    },
    BrowserAsset {
        path: "polkavm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-translated.js"),
        sha256: "68bce29b80e3276e15f725519a08ddb546bf6232215a913128580e257a828aed",
    },
    BrowserAsset {
        path: "polkavm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-runtime-core.js"),
        sha256: "93cfb4b2fa2e054ba356ccc59fba76249a718e1ba52737b6992941e1cd8fec68",
    },
    BrowserAsset {
        path: "polkavm-wasm-worker-entry.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-worker-entry.js"),
        sha256: "fa600faff369b09eae5a50dd4b08445b7762d89d6db269b70230ad5a8bf67951",
    },
    BrowserAsset {
        path: "polkavm-computer.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-computer.js"),
        sha256: "baa5353c8a3abc87d85340b32902f645c9c3797f637e352e032f78e52b9a5902",
    },
    BrowserAsset {
        path: "SHA256SUMS",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/SHA256SUMS"),
        sha256: "bdfb12253e1eff5d383c62ef0aed849d1c614f20e21b7219b16d51a45da6c5cf",
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
