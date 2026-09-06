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
        sha256: "b5b8b39affa759317c77d223ec530e61734869e2a3925f78c71e885a879b2911",
    },
    BrowserAsset {
        path: "polkavm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-worker.js"),
        sha256: "a075e02bf90e9c0fbd31fbb3c6084d34a69a1148e392f67c273ad7bd292047ca",
    },
    BrowserAsset {
        path: "polkavm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-gpu-worker.js"),
        sha256: "86cb899953b303dca45b0a5f2f2409713809e7223b9a9a5b853c9660d152edec",
    },
    BrowserAsset {
        path: "polkavm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-translated.js"),
        sha256: "cf7f28ae591fc60fea3bf0e8ddf1ed5667912152e12833eebc3379712dd046f1",
    },
    BrowserAsset {
        path: "polkavm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-runtime-core.js"),
        sha256: "fa157e3d52dfe23f3fe96050e62cfabed96abed93fcd7b0778c8db5ecf1e2632",
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
        sha256: "b2d9f099e486d7f6b25a0419bef32e79a910c3981665b109313ba19d3c4c9b0a",
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
