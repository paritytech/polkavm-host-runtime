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
        sha256: "c8cd7d02cb79231bb644bb60b775225307abda5263c505181f5dcdadb8925680",
    },
    BrowserAsset {
        path: "polkavm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-worker.js"),
        sha256: "49c6b327bd07b598fa9a7b0353820dd91fe8ff70cb365b3bad909a9bcc41c82f",
    },
    BrowserAsset {
        path: "polkavm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-gpu-worker.js"),
        sha256: "447e1a9f8ffe4db4db83c37b70bfbeb2409b5ca9e437cb68d0d1822c053c07b4",
    },
    BrowserAsset {
        path: "polkavm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-translated.js"),
        sha256: "3f919b7f94416553c60fce27dac587750432a1c225c59e442670724d4ccdcb43",
    },
    BrowserAsset {
        path: "polkavm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-runtime-core.js"),
        sha256: "cf865bfb7ed516d0b05a31f0fc9cc6bdd849ba08cc5a3754f19e4a42b474c481",
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
        sha256: "d16fb4c2898b2d04671103e0c193d6e21cbb12378c1cd9191ead47b022ca5878",
    },
    BrowserAsset {
        path: "SHA256SUMS",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/SHA256SUMS"),
        sha256: "7f7948ead1968276f583063c9d0c520c51d15cdffdc455954a13e2b7944ba4b9",
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
