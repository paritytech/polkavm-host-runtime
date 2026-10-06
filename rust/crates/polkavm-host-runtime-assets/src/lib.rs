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

const ASSETS: [BrowserAsset; 15] = [
    BrowserAsset {
        path: "LICENSE-MPL-2.0",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/LICENSE-MPL-2.0"),
        sha256: "4b89d4518bd135ab4ee154a7bce722246b57a98c3d7efc1a09409898160c2bd1",
    },
    BrowserAsset {
        path: "SHA256SUMS",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/SHA256SUMS"),
        sha256: "55c09c035418ae8e7c3ad3d2957ff3bacbf44f95f6433f9d05058c159b8d5396",
    },
    BrowserAsset {
        path: "THIRD_PARTY_LICENSES.txt",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/THIRD_PARTY_LICENSES.txt"),
        sha256: "4faed1b37725064787327657c8254f05322335c31c601fdbcc3b473ae189ba24",
    },
    BrowserAsset {
        path: "THIRD_PARTY_NOTICES.md",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/THIRD_PARTY_NOTICES.md"),
        sha256: "9c0e912f1ba2853c28497f21c6bed1aa17dd11939d01cd3d59656bec5fa32ce1",
    },
    BrowserAsset {
        path: "file-input-router.d.ts",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/file-input-router.d.ts"),
        sha256: "f72633219f482e458a1420f60ecc84d638d12d5389173fb6c7db85ed4447667f",
    },
    BrowserAsset {
        path: "file-input-router.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/file-input-router.js"),
        sha256: "788ed73bc024bbbe1651000d7df913e9c6f778909f45e8b077b8e3cafe7e5af3",
    },
    BrowserAsset {
        path: "polkavm-browser-runtime.wasm",
        content_type: "application/wasm",
        bytes: include_bytes!("../assets/polkavm-browser-runtime.wasm"),
        sha256: "dc224c5d1f4cc109af7747ddae03abd73217031c982123d75f186aa4cb7a15fc",
    },
    BrowserAsset {
        path: "polkavm-computer.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-computer.js"),
        sha256: "edb78a7e5973b92c36231e1383fb384f400ba277e9158e5228f39b7234ad4e29",
    },
    BrowserAsset {
        path: "polkavm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-gpu-worker.js"),
        sha256: "4a69ffb026d58ea9353929a67b2aba403e0e82c3e23c3b61b2d425fa91241fad",
    },
    BrowserAsset {
        path: "polkavm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-runtime-core.js"),
        sha256: "1a7b738cd04d84be9b654bc1329f63e573957d7cea88cd58b30c33294cc8f4d2",
    },
    BrowserAsset {
        path: "polkavm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-translated.js"),
        sha256: "c5b1783067d5db58a8d0a34dd0649a7d04e7cd23cd86a46368c8959820b55234",
    },
    BrowserAsset {
        path: "polkavm-wasm-worker-entry.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-worker-entry.js"),
        sha256: "fa600faff369b09eae5a50dd4b08445b7762d89d6db269b70230ad5a8bf67951",
    },
    BrowserAsset {
        path: "polkavm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-worker.js"),
        sha256: "92f25922c91fccadb7abeef64679a0c036c32e12f97fa80bcf328387566c8dab",
    },
    BrowserAsset {
        path: "session.d.ts",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/session.d.ts"),
        sha256: "380087d0b567d52c7421562b4d2ef20441f7a957f2340bddea73a05a5e689dda",
    },
    BrowserAsset {
        path: "session.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/session.js"),
        sha256: "caa19c63fae1fd4efd59b9b49f9fbd8425cebc8d744d6512f1d92fae24a96943",
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
