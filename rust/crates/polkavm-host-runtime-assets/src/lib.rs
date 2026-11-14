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
        sha256: "7298895f40335bc0dff8a5db7dd139b9340a2b08c716e4e70f6947ca92c7f4db",
    },
    BrowserAsset {
        path: "THIRD_PARTY_LICENSES.txt",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/THIRD_PARTY_LICENSES.txt"),
        sha256: "bdb4aa5a991e03e4538dd1cd70d3f3614f0befd8a0d7d3737ad04bb72143df79",
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
        sha256: "8c71877ba14370c45e192ec35699a853f4fb57327d7e358f9453b70be8a0d673",
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
        sha256: "0a50b511b0321ecdd2fd9fb3f106ec3515b0acd76d3c452c87cfe1c4cf9cd251",
    },
    BrowserAsset {
        path: "polkavm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-runtime-core.js"),
        sha256: "b04e9d5bd64dc3840b3f579ccaae4622e1d5fd2dc6a6c42af314c18052d4f83e",
    },
    BrowserAsset {
        path: "polkavm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-translated.js"),
        sha256: "eff7feebdea88223e69cd946fef61a229fdf260db40e044fb5b3fa38faff91a0",
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
        sha256: "831b837598630a5561aa50e499295092f9161ba4f855a79274368c8daa914f01",
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
