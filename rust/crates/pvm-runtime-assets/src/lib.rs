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
        path: "pvm-browser-runtime.wasm",
        content_type: "application/wasm",
        bytes: include_bytes!("../assets/pvm-browser-runtime.wasm"),
        sha256: "38d50d916eb480412c1e84ce6e31f8bca4c3f3d2af5ad193eee8e8ccd3bbcfd5",
    },
    BrowserAsset {
        path: "pvm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-worker.js"),
        sha256: "7fa96b5014462c1fa88c594e571a4d04fa43657ba5b444e67db2f81c4ceddda2",
    },
    BrowserAsset {
        path: "pvm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-gpu-worker.js"),
        sha256: "86cb899953b303dca45b0a5f2f2409713809e7223b9a9a5b853c9660d152edec",
    },
    BrowserAsset {
        path: "pvm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-wasm-translated.js"),
        sha256: "4944ee0432a5643325b86c43e2bd69dc6be375c37e0893cc9137a968525ce637",
    },
    BrowserAsset {
        path: "pvm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-runtime-core.js"),
        sha256: "326447dff6302553b1a58042d9ff56226e2f27cf96d836d3dd2bc02aa007c709",
    },
    BrowserAsset {
        path: "pvm-wasm-worker-entry.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-wasm-worker-entry.js"),
        sha256: "9c929f5d5c64a1b75e7e48485d7c3944ed6838112177ea778827a2c407c2d820",
    },
    BrowserAsset {
        path: "pvm-computer.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-computer.js"),
        sha256: "55a77833038d07dbaebba9369eead718c3a487c3cff297af4689a2afd551b976",
    },
    BrowserAsset {
        path: "SHA256SUMS",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/SHA256SUMS"),
        sha256: "444c45c138c550cd5a23cc5002a5b64a1e22f83d5240713e95f817f957ff76a1",
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
