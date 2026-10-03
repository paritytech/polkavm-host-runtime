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

const ASSETS: [BrowserAsset; 14] = [
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
        sha256: "7c2979a7bcb3ed83347250ef314205f36548962624a634a8dbc0d6efb127d7ca",
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
        path: "file-input-router.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/file-input-router.js"),
        sha256: "324bff516e76e7471782f70e283460be9b02aa90d2910146684f5c93416d4ac0",
    },
    BrowserAsset {
        path: "polkavm-browser-runtime.wasm",
        content_type: "application/wasm",
        bytes: include_bytes!("../assets/polkavm-browser-runtime.wasm"),
        sha256: "de0311b7d3212eb0a926d8cc85765dc39c64811e9f2a5c1b7a11d5e7531c742b",
    },
    BrowserAsset {
        path: "polkavm-computer.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-computer.js"),
        sha256: "d16fb4c2898b2d04671103e0c193d6e21cbb12378c1cd9191ead47b022ca5878",
    },
    BrowserAsset {
        path: "polkavm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-gpu-worker.js"),
        sha256: "f84aa16e54fa97e27180340f9d90cac05cbf4cacf469a4c2922cec2534e481db",
    },
    BrowserAsset {
        path: "polkavm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-runtime-core.js"),
        sha256: "8d6f83e3b88e037ec250e12e3947e36d0a87af71575303f7f91993ea5f469271",
    },
    BrowserAsset {
        path: "polkavm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/polkavm-wasm-translated.js"),
        sha256: "0b32aa5df4febd36d9718fd2dc8c8c3995997ceba2dcfad7714b3d10be13ca91",
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
        sha256: "2e71ce6f277030e89cf686a6bcc7ac2f215195d97cba7834e9d5a3fc1a2c5860",
    },
    BrowserAsset {
        path: "session.d.ts",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/session.d.ts"),
        sha256: "5370cf468a560cb96de1f890e4d1955a9d484509d2effbc0ca1bb7d38cb2d8d8",
    },
    BrowserAsset {
        path: "session.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/session.js"),
        sha256: "f34ddf23513a55555295a2bb74051a979faccb7cdeed13cf99d1df3020aa711a",
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
