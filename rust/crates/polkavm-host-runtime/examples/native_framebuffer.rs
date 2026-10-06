/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Capture the first framebuffer from a caller-supplied, asset-free guest.
//!
//! From the repository root:
//! ```sh
//! cargo run -p polkavm-host-runtime --example native_framebuffer -- \
//!   rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm frame.ppm
//! ```
//! The repository fixture requires no assets and produces a 320 × 200 image.
//!
//! The host owns guest selection, presentation policy, execution scheduling and
//! output storage. This headless host allows framebuffer presentation only,
//! disables audio, provides no assets or external services, and stops after the
//! first frame (or 60 bounded updates). It is not a general application launcher:
//! a production host must verify its application manifest and mediate permissions.
//! The output is binary PPM (RGB); guest alpha is discarded. Dropping the stopped
//! runtime releases its VM and guest memory, including on an execution error.

use anyhow::{bail, Context, Result};
use polkavm_host_runtime::{ApplicationRuntime, BackendKind, Frame, PresentationProfile};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};

const USAGE: &str = "Usage: native_framebuffer <guest.polkavm> <output.ppm>";
const GAS_PER_UPDATE: u64 = 10_000_000;
const MAX_UPDATES: usize = 60;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let guest = args.next().context(USAGE)?;
    if guest == "--help" || guest == "-h" {
        if args.next().is_some() {
            bail!(USAGE);
        }
        println!("{USAGE}\nCapture an asset-free guest's first framebuffer as binary PPM.");
        return Ok(());
    }
    let output = args.next().context(USAGE)?;
    if args.next().is_some() {
        bail!(USAGE);
    }

    let program = fs::read(&guest).context("read guest program")?;
    let mut runtime = ApplicationRuntime::new_with_backend(
        &program,
        HashMap::new(),
        PresentationProfile::Framebuffer,
        false,
        GAS_PER_UPDATE,
        BackendKind::Interpreter,
    )
    .context("create bounded application runtime")?;
    let result = capture_frame(&mut runtime);
    // Stop on success and failure, before propagating any execution error.
    runtime.stop();
    drop(runtime);
    let frame = result?;

    let mut writer = BufWriter::new(File::create(&output).context("create output image")?);
    write!(writer, "P6\n{} {}\n255\n", frame.width, frame.height)?;
    for pixel in frame.argb.chunks_exact(4) {
        writer.write_all(&[pixel[2], pixel[1], pixel[0]])?;
    }
    writer.flush().context("flush output image")?;
    println!(
        "Wrote {} x {} RGB PPM to {:?}",
        frame.width, frame.height, output
    );
    Ok(())
}

fn capture_frame(runtime: &mut ApplicationRuntime) -> Result<Frame> {
    runtime.init().context("initialize guest")?;
    if let Some(frame) = runtime.take_frame() {
        return Ok(frame);
    }
    for _ in 0..MAX_UPDATES {
        runtime.update().context("update guest")?;
        if let Some(frame) = runtime.take_frame() {
            return Ok(frame);
        }
    }
    bail!("guest produced no framebuffer within {MAX_UPDATES} updates")
}
