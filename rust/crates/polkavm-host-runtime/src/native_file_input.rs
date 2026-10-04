/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{FileCache, FileReadSource, MAX_FILE_CACHE_BYTES, MAX_FILE_READ_BYTES};
use anyhow::{bail, Context, Result};
use std::fs::{File, OpenOptions};
#[cfg(not(unix))]
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::Path;
#[cfg(not(unix))]
use std::path::PathBuf;

/// A user-selected local file retained without loading it into guest assets.
/// Pass it to `send_file_stream`; cancellation and teardown close it by Drop.
pub struct LocalFileSource {
    file: File,
    size: u64,
}

impl LocalFileSource {
    /// Retain an already-open regular file and capture its current length.
    pub fn new(file: File) -> Result<Self> {
        let metadata = file.metadata().context("inspect selected stream file")?;
        if !metadata.is_file() {
            bail!("selected stream source is not a regular file");
        }
        Ok(Self {
            file,
            size: metadata.len(),
        })
    }
}

impl FileReadSource for LocalFileSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_exact_at(&mut self, offset: u32, destination: &mut [u8]) -> Result<()> {
        if destination.len() > MAX_FILE_READ_BYTES as usize
            || u64::from(offset) + destination.len() as u64 > self.size
        {
            bail!("file read exceeds the selected source or bounded chunk size");
        }
        #[cfg(unix)]
        self.file
            .read_exact_at(destination, u64::from(offset))
            .context("read selected stream range")?;
        #[cfg(not(unix))]
        {
            self.file
                .seek(SeekFrom::Start(u64::from(offset)))
                .context("seek selected stream range")?;
            self.file
                .read_exact(destination)
                .context("read selected stream range")?;
        }
        Ok(())
    }
}

/// A private temporary disk cache in a directory selected by the Host.
///
/// No path is exposed to the guest. On Unix the newly opened file is unlinked
/// immediately, so even a process crash releases its backing storage. Elsewhere
/// Drop closes the file before removing its private name.
pub struct LocalFileCache {
    file: Option<File>,
    size: u64,
    #[cfg(not(unix))]
    path: PathBuf,
}

impl LocalFileCache {
    /// Create a private temporary cache in a host-selected directory.
    pub fn new(directory: &Path) -> Result<Self> {
        for _ in 0..16 {
            let mut random = [0u8; 16];
            getrandom::fill(&mut random)
                .map_err(|error| anyhow::anyhow!("generate private cache name: {error}"))?;
            let path = directory.join(format!(
                "polkavm-cache-{:032x}",
                u128::from_le_bytes(random)
            ));
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("create private file cache"),
            };
            #[cfg(unix)]
            std::fs::remove_file(&path).context("unlink private file cache")?;
            return Ok(Self {
                file: Some(file),
                size: 0,
                #[cfg(not(unix))]
                path,
            });
        }
        bail!("could not allocate a unique private cache name");
    }
}

impl FileReadSource for LocalFileCache {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_exact_at(&mut self, offset: u32, destination: &mut [u8]) -> Result<()> {
        if destination.len() > MAX_FILE_READ_BYTES as usize
            || u64::from(offset) + destination.len() as u64 > self.size
        {
            bail!("cache read exceeds the reserved size or bounded chunk size");
        }
        let file = self.file.as_mut().expect("cache remains open until drop");
        #[cfg(unix)]
        file.read_exact_at(destination, u64::from(offset))
            .context("read private cache range")?;
        #[cfg(not(unix))]
        {
            file.seek(SeekFrom::Start(u64::from(offset)))
                .context("seek private cache range")?;
            file.read_exact(destination)
                .context("read private cache range")?;
        }
        Ok(())
    }
}

impl FileCache for LocalFileCache {
    fn reset(&mut self, size: u32) -> Result<()> {
        if size == 0 || size > MAX_FILE_CACHE_BYTES {
            bail!("cache size exceeds the private cache limit");
        }
        let file = self.file.as_mut().expect("cache remains open until drop");
        file.set_len(0).context("truncate private cache")?;
        self.size = 0;
        file.set_len(u64::from(size))
            .context("resize private cache")?;
        self.size = u64::from(size);
        Ok(())
    }

    fn write_exact_at(&mut self, offset: u32, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty()
            || bytes.len() > MAX_FILE_READ_BYTES as usize
            || u64::from(offset) + bytes.len() as u64 > self.size
        {
            bail!("cache write exceeds the reserved size or bounded chunk size");
        }
        let file = self.file.as_mut().expect("cache remains open until drop");
        #[cfg(unix)]
        file.write_all_at(bytes, u64::from(offset))
            .context("write private cache range")?;
        #[cfg(not(unix))]
        {
            file.seek(SeekFrom::Start(u64::from(offset)))
                .context("seek private cache range")?;
            file.write_all(bytes).context("write private cache range")?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.file
            .as_ref()
            .expect("cache remains open until drop")
            .sync_data()
            .context("flush private cache")
    }
}

impl Drop for LocalFileCache {
    fn drop(&mut self) {
        drop(self.file.take());
        #[cfg(not(unix))]
        let _ = std::fs::remove_file(&self.path);
    }
}
