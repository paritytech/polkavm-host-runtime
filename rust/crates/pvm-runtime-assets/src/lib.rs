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
        sha256: "690ca3aef4b4cc397af89df0b642a9b9f579e86a150468c13c4f80cbb005ad15",
    },
    BrowserAsset {
        path: "pvm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-worker.js"),
        sha256: "5710ca2ec904640de4e01c29a4538707630313a9a45de9ddf1fbc815dbe81f8d",
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
        sha256: "808898bb7af3a82e92a8ee98135ecd335825bd998c36347d609b1cbac333a524",
    },
    BrowserAsset {
        path: "pvm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-runtime-core.js"),
        sha256: "e87ec89e1ba4158cfb205a8b8721d2848eb0b617ee8b7bd59e52add31095ed60",
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
        sha256: "7e028729457e021e0092eef229135c39fc24301b4fb082d1a8da1a221a4c8050",
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
