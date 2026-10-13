/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Bounded UI platform-output wire values shared by PolkaVM guests and Hosts.

#![no_std]

extern crate alloc;

use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::fmt;

/// Import name used by PolkaVM guests to submit one UI platform-output snapshot.
pub const UI_OUTPUT_SUBMIT_IMPORT: &str = "host_ui_output_submit";
/// Four-byte discriminator at the start of every UI output v1 stream.
pub const UI_OUTPUT_MAGIC: [u8; 4] = *b"PUI1";
/// Current UI output wire version.
pub const UI_OUTPUT_VERSION: u16 = 1;
/// Encoded byte length of the UI output v1 header.
pub const UI_OUTPUT_HEADER_BYTES: usize = 48;
/// Encoded byte length of every UI output command header.
pub const UI_OUTPUT_COMMAND_HEADER_BYTES: usize = 8;
/// Maximum complete UI output stream accepted by the Host.
pub const MAX_UI_OUTPUT_BYTES: usize = MAX_UI_COPY_IMAGE_BYTES + 256 * 1024;
/// Maximum commands accepted in one UI output stream.
pub const MAX_UI_OUTPUT_COMMANDS: usize = 64;
/// Maximum UTF-8 clipboard text accepted in one command.
pub const MAX_UI_COPY_TEXT_BYTES: usize = 64 * 1024;
/// Maximum RGBA pixels accepted in one clipboard image.
pub const MAX_UI_COPY_IMAGE_PIXELS: usize = 1024 * 1024;
/// Maximum RGBA bytes accepted in one clipboard image.
pub const MAX_UI_COPY_IMAGE_BYTES: usize = MAX_UI_COPY_IMAGE_PIXELS * 4;
/// Maximum width or height accepted for one clipboard image.
pub const MAX_UI_COPY_IMAGE_DIMENSION: usize = 2048;
/// Maximum UTF-8 URL accepted in one command.
pub const MAX_UI_OPEN_URL_BYTES: usize = 8 * 1024;

/// The UI output was accepted.
pub const UI_OUTPUT_SUBMIT_ACCEPTED: u32 = 0;
/// The UI output was malformed, out of bounds, or over a limit.
pub const UI_OUTPUT_SUBMIT_INVALID: u32 = 1;
/// A UI output was already submitted during the current guest call.
pub const UI_OUTPUT_SUBMIT_DUPLICATE: u32 = 2;

/// The pointer is currently over mutable text.
pub const UI_OUTPUT_FLAG_MUTABLE_TEXT: u8 = 1 << 0;
/// The header contains active IME geometry.
pub const UI_OUTPUT_FLAG_IME: u8 = 1 << 1;
const UI_OUTPUT_FLAGS_V1: u8 = UI_OUTPUT_FLAG_MUTABLE_TEXT | UI_OUTPUT_FLAG_IME;

/// Copy UTF-8 text to the platform clipboard.
pub const UI_OUTPUT_COMMAND_COPY_TEXT: u8 = 1;
/// Open a UTF-8 URL through the Host's navigation policy.
pub const UI_OUTPUT_COMMAND_OPEN_URL: u8 = 2;
/// Copy unpremultiplied sRGBA pixels to the platform clipboard.
pub const UI_OUTPUT_COMMAND_COPY_IMAGE: u8 = 3;
/// Open the URL in a new platform surface rather than replacing the current one.
pub const UI_OUTPUT_OPEN_URL_NEW_SURFACE: u8 = 1 << 0;
/// Replace the Host-retained text viewport: changed rows as styled runs.
/// Only accepted by Hosts that render the viewport; see [`UiOutputDecodeOptions`].
pub const UI_OUTPUT_COMMAND_TEXT_VIEWPORT: u8 = 4;
/// Encoded byte length of the text-viewport command header.
pub const TEXT_VIEWPORT_HEADER_BYTES: usize = 52;
/// Encoded byte length of every text-viewport row header.
pub const TEXT_VIEWPORT_ROW_HEADER_BYTES: usize = 8;
/// Encoded byte length of every text-viewport styled run.
pub const TEXT_VIEWPORT_RUN_BYTES: usize = 8;
/// Maximum rows in one viewport and maximum changed rows in one command.
pub const MAX_TEXT_VIEWPORT_ROWS: usize = 512;
/// Maximum UTF-8 bytes in one viewport row.
pub const MAX_TEXT_VIEWPORT_ROW_BYTES: usize = 16 * 1024;
/// The run is rendered with a bold weight.
pub const TEXT_VIEWPORT_RUN_BOLD: u8 = 1 << 0;
/// The run is rendered italic.
pub const TEXT_VIEWPORT_RUN_ITALIC: u8 = 1 << 1;
const TEXT_VIEWPORT_RUN_FLAGS: u8 = TEXT_VIEWPORT_RUN_BOLD | TEXT_VIEWPORT_RUN_ITALIC;
/// Cursor row value meaning "no cursor in this viewport".
pub const TEXT_VIEWPORT_NO_CURSOR: u32 = u32::MAX;

/// A surface-relative rectangle in logical UI points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiRect {
    /// Left edge.
    pub min_x: f32,
    /// Top edge.
    pub min_y: f32,
    /// Right edge.
    pub max_x: f32,
    /// Bottom edge.
    pub max_y: f32,
}

impl UiRect {
    fn is_valid(self) -> bool {
        [self.min_x, self.min_y, self.max_x, self.max_y]
            .iter()
            .all(|value| value.is_finite())
            && self.max_x >= self.min_x
            && self.max_y >= self.min_y
    }
}

/// Geometry for an active text editor and its primary cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiImeOutput {
    /// Bounds of the text editor.
    pub rect: UiRect,
    /// Bounds of the primary text cursor.
    pub cursor_rect: UiRect,
}

impl UiImeOutput {
    fn is_valid(self) -> bool {
        self.rect.is_valid() && self.cursor_rect.is_valid()
    }
}

/// Host cursor requested by the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum UiCursorIcon {
    /// Platform default cursor.
    #[default]
    Default = 0,
    /// Hide the cursor.
    None = 1,
    /// A context menu is available.
    ContextMenu = 2,
    /// Help is available.
    Help = 3,
    /// Pointing hand for a link or action.
    PointingHand = 4,
    /// Work is in progress while interaction remains possible.
    Progress = 5,
    /// Work is in progress and interaction should wait.
    Wait = 6,
    /// Table-cell selection.
    Cell = 7,
    /// Precision crosshair.
    Crosshair = 8,
    /// Horizontal text selection.
    Text = 9,
    /// Vertical text selection.
    VerticalText = 10,
    /// Alias or shortcut operation.
    Alias = 11,
    /// Copy operation.
    Copy = 12,
    /// Move operation.
    Move = 13,
    /// The current item cannot be dropped here.
    NoDrop = 14,
    /// The requested operation is forbidden.
    NotAllowed = 15,
    /// The item can be grabbed.
    Grab = 16,
    /// The item is being grabbed.
    Grabbing = 17,
    /// Bidirectional scrolling.
    AllScroll = 18,
    /// Horizontal resize.
    ResizeHorizontal = 19,
    /// North-east to south-west resize.
    ResizeNeSw = 20,
    /// North-west to south-east resize.
    ResizeNwSe = 21,
    /// Vertical resize.
    ResizeVertical = 22,
    /// East-edge resize.
    ResizeEast = 23,
    /// South-east-edge resize.
    ResizeSouthEast = 24,
    /// South-edge resize.
    ResizeSouth = 25,
    /// South-west-edge resize.
    ResizeSouthWest = 26,
    /// West-edge resize.
    ResizeWest = 27,
    /// North-west-edge resize.
    ResizeNorthWest = 28,
    /// North-edge resize.
    ResizeNorth = 29,
    /// North-east-edge resize.
    ResizeNorthEast = 30,
    /// Column resize.
    ResizeColumn = 31,
    /// Row resize.
    ResizeRow = 32,
    /// Zoom in.
    ZoomIn = 33,
    /// Zoom out.
    ZoomOut = 34,
}

impl TryFrom<u8> for UiCursorIcon {
    type Error = UiOutputDecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Default,
            1 => Self::None,
            2 => Self::ContextMenu,
            3 => Self::Help,
            4 => Self::PointingHand,
            5 => Self::Progress,
            6 => Self::Wait,
            7 => Self::Cell,
            8 => Self::Crosshair,
            9 => Self::Text,
            10 => Self::VerticalText,
            11 => Self::Alias,
            12 => Self::Copy,
            13 => Self::Move,
            14 => Self::NoDrop,
            15 => Self::NotAllowed,
            16 => Self::Grab,
            17 => Self::Grabbing,
            18 => Self::AllScroll,
            19 => Self::ResizeHorizontal,
            20 => Self::ResizeNeSw,
            21 => Self::ResizeNwSe,
            22 => Self::ResizeVertical,
            23 => Self::ResizeEast,
            24 => Self::ResizeSouthEast,
            25 => Self::ResizeSouth,
            26 => Self::ResizeSouthWest,
            27 => Self::ResizeWest,
            28 => Self::ResizeNorthWest,
            29 => Self::ResizeNorth,
            30 => Self::ResizeNorthEast,
            31 => Self::ResizeColumn,
            32 => Self::ResizeRow,
            33 => Self::ZoomIn,
            34 => Self::ZoomOut,
            _ => return Err(UiOutputDecodeError::CursorIcon),
        })
    }
}

/// Persistent UI integration state included in one output snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiOutputDescriptor {
    /// Cursor requested for the application surface.
    pub cursor_icon: UiCursorIcon,
    /// Whether the pointer is over mutable text.
    pub mutable_text_under_cursor: bool,
    /// Active text-editor geometry, when editing text.
    pub ime: Option<UiImeOutput>,
}

/// One decoded ephemeral UI command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiOutputCommand<'a> {
    /// Replace the platform clipboard text.
    CopyText(&'a str),
    /// Navigate to a URL through Host policy.
    OpenUrl {
        /// Untrusted URL requested by the guest.
        url: &'a str,
        /// Whether a new platform surface was requested.
        new_surface: bool,
    },
    /// Replace the platform clipboard image with unpremultiplied sRGBA pixels.
    CopyImage {
        /// Image width in pixels.
        width: u32,
        /// Image height in pixels.
        height: u32,
        /// Row-major unpremultiplied sRGBA pixels.
        rgba: &'a [u8],
    },
    /// Replace the Host-retained text viewport with the rows that changed.
    TextViewport(UiTextViewport<'a>),
}

/// Geometry and identity of one Host-retained text viewport snapshot.
///
/// Lengths are in logical UI points relative to the application surface. Row
/// `r` of the document is drawn with its top edge at
/// `origin_y + (r - first_row) * line_height - scroll_y_fraction`, where the
/// fraction is already folded into `origin_y` by the sender.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTextViewportDescriptor {
    /// Monotonic per-guest snapshot number; the Host applies rows in order.
    pub sequence: u32,
    /// First visible document row.
    pub first_row: u32,
    /// Visible rows; rows outside `first_row..first_row + row_count` are dropped.
    pub row_count: u32,
    /// Cursor as `(row, column)` in document rows and UTF-8 byte columns.
    pub cursor: Option<(u32, u32)>,
    /// Text area origin and size.
    pub bounds: UiRect,
    /// Vertical pitch between rows.
    pub line_height: f32,
    /// Font size used by the sender's layout.
    pub font_size: f32,
    /// Horizontal scroll offset applied before drawing every row.
    pub scroll_x: f32,
}

impl UiTextViewportDescriptor {
    fn is_valid(&self) -> bool {
        self.row_count as usize <= MAX_TEXT_VIEWPORT_ROWS
            && self.first_row.checked_add(self.row_count).is_some()
            && self.cursor.is_none_or(|(row, _)| {
                row != TEXT_VIEWPORT_NO_CURSOR
                    && row >= self.first_row
                    && row < self.first_row + self.row_count
            })
            && self.bounds.is_valid()
            && self.line_height.is_finite()
            && self.line_height > 0.0
            && self.font_size.is_finite()
            && self.font_size > 0.0
            && self.scroll_x.is_finite()
            && self.scroll_x >= 0.0
    }
}

/// One styled run within a text-viewport row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiTextRun {
    /// UTF-8 byte length covered by this run; runs tile the row text exactly.
    pub len: u16,
    /// Text colour as `0xRRGGBBAA`.
    pub rgba: u32,
    /// [`TEXT_VIEWPORT_RUN_BOLD`] and [`TEXT_VIEWPORT_RUN_ITALIC`] bits.
    pub flags: u8,
}

/// One validated changed row borrowed from the stream.
#[derive(Clone, Copy, Debug)]
pub struct UiTextRow<'a> {
    /// Document row.
    pub row: u32,
    /// Row text without line terminators.
    pub text: &'a str,
    runs: &'a [u8],
}

impl<'a> UiTextRow<'a> {
    /// Styled runs covering `text` in order.
    pub fn runs(self) -> impl ExactSizeIterator<Item = UiTextRun> + 'a {
        self.runs
            .chunks_exact(TEXT_VIEWPORT_RUN_BYTES)
            .map(|run| UiTextRun {
                len: read_u16(run, 0),
                flags: run[2],
                rgba: read_u32(run, 4),
            })
    }
}

/// A validated text-viewport command borrowed from the stream.
#[derive(Clone, Copy, Debug)]
pub struct UiTextViewport<'a> {
    descriptor: UiTextViewportDescriptor,
    changed_count: u32,
    rows: &'a [u8],
}

impl<'a> UiTextViewport<'a> {
    /// Geometry and identity of this snapshot.
    pub fn descriptor(self) -> UiTextViewportDescriptor {
        self.descriptor
    }

    /// Number of changed rows carried by this snapshot.
    pub fn changed_count(self) -> usize {
        self.changed_count as usize
    }

    /// Changed rows in ascending document order.
    pub fn rows(self) -> UiTextRows<'a> {
        UiTextRows {
            bytes: self.rows,
            remaining: self.changed_count,
        }
    }
}

impl PartialEq for UiTextViewport<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && self.rows == other.rows
    }
}

impl Eq for UiTextViewport<'_> {}

/// Iterator over changed rows of a validated text-viewport command.
#[derive(Clone, Debug)]
pub struct UiTextRows<'a> {
    bytes: &'a [u8],
    remaining: u32,
}

impl<'a> Iterator for UiTextRows<'a> {
    type Item = UiTextRow<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let row = read_u32(self.bytes, 0);
        let text_len = usize::from(read_u16(self.bytes, 4));
        let run_count = usize::from(read_u16(self.bytes, 6));
        let text_start = TEXT_VIEWPORT_ROW_HEADER_BYTES;
        let runs_start = text_start + text_len;
        let end = runs_start + run_count * TEXT_VIEWPORT_RUN_BYTES;
        let item = UiTextRow {
            row,
            text: core::str::from_utf8(&self.bytes[text_start..runs_start])
                .expect("validated UI viewport row text"),
            runs: &self.bytes[runs_start..end],
        };
        self.bytes = &self.bytes[end..];
        self.remaining -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for UiTextRows<'_> {}

/// A validated borrowed UI output stream.
#[derive(Clone, Copy, Debug)]
pub struct UiOutput<'a> {
    bytes: &'a [u8],
    descriptor: UiOutputDescriptor,
    command_count: u16,
}

impl<'a> UiOutput<'a> {
    /// Persistent state requested by this snapshot.
    pub fn descriptor(self) -> UiOutputDescriptor {
        self.descriptor
    }

    /// Number of ephemeral commands in this snapshot.
    pub fn command_count(self) -> usize {
        usize::from(self.command_count)
    }

    /// Iterate over validated commands in submission order.
    pub fn commands(self) -> UiOutputCommands<'a> {
        UiOutputCommands {
            bytes: self.bytes,
            offset: UI_OUTPUT_HEADER_BYTES,
            remaining: self.command_count,
        }
    }
}

/// Iterator over commands in a validated UI output stream.
#[derive(Clone, Debug)]
pub struct UiOutputCommands<'a> {
    bytes: &'a [u8],
    offset: usize,
    remaining: u16,
}

impl<'a> Iterator for UiOutputCommands<'a> {
    type Item = UiOutputCommand<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let opcode = self.bytes[self.offset];
        let flags = self.bytes[self.offset + 1];
        let length = read_u32(self.bytes, self.offset + 4) as usize;
        let start = self.offset + UI_OUTPUT_COMMAND_HEADER_BYTES;
        let end = start + length;
        let payload = &self.bytes[start..end];
        self.offset = end;
        self.remaining -= 1;
        Some(match opcode {
            UI_OUTPUT_COMMAND_COPY_TEXT => UiOutputCommand::CopyText(
                core::str::from_utf8(payload).expect("validated UI command text"),
            ),
            UI_OUTPUT_COMMAND_OPEN_URL => UiOutputCommand::OpenUrl {
                url: core::str::from_utf8(payload).expect("validated UI command URL"),
                new_surface: flags & UI_OUTPUT_OPEN_URL_NEW_SURFACE != 0,
            },
            UI_OUTPUT_COMMAND_COPY_IMAGE => UiOutputCommand::CopyImage {
                width: read_u32(payload, 0),
                height: read_u32(payload, 4),
                rgba: &payload[8..],
            },
            UI_OUTPUT_COMMAND_TEXT_VIEWPORT => {
                UiOutputCommand::TextViewport(text_viewport_from_validated(payload))
            }
            _ => unreachable!("validated UI command opcode"),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::from(self.remaining);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for UiOutputCommands<'_> {}

/// Incremental encoder that reuses a caller-owned byte buffer.
pub struct UiOutputEncoder<'a> {
    destination: &'a mut Vec<u8>,
    command_count: u16,
}

impl<'a> UiOutputEncoder<'a> {
    /// Begin a stream, clearing and retaining the destination allocation.
    pub fn begin(
        destination: &'a mut Vec<u8>,
        descriptor: UiOutputDescriptor,
    ) -> Result<Self, UiOutputEncodeError> {
        if descriptor.ime.is_some_and(|ime| !ime.is_valid()) {
            return Err(UiOutputEncodeError::ImeBounds);
        }
        destination.clear();
        destination
            .try_reserve(UI_OUTPUT_HEADER_BYTES)
            .map_err(UiOutputEncodeError::Allocation)?;
        destination.extend_from_slice(&UI_OUTPUT_MAGIC);
        push_u16(destination, UI_OUTPUT_VERSION);
        push_u16(destination, UI_OUTPUT_HEADER_BYTES as u16);
        push_u32(destination, 0);
        push_u16(destination, 0);
        destination.push(descriptor.cursor_icon as u8);
        let mut flags = 0;
        if descriptor.mutable_text_under_cursor {
            flags |= UI_OUTPUT_FLAG_MUTABLE_TEXT;
        }
        if descriptor.ime.is_some() {
            flags |= UI_OUTPUT_FLAG_IME;
        }
        destination.push(flags);
        let ime = descriptor.ime.unwrap_or_default();
        for value in [
            ime.rect.min_x,
            ime.rect.min_y,
            ime.rect.max_x,
            ime.rect.max_y,
            ime.cursor_rect.min_x,
            ime.cursor_rect.min_y,
            ime.cursor_rect.max_x,
            ime.cursor_rect.max_y,
        ] {
            push_u32(destination, value.to_bits());
        }
        Ok(Self {
            destination,
            command_count: 0,
        })
    }

    /// Append one clipboard-text command.
    pub fn copy_text(&mut self, text: &str) -> Result<(), UiOutputEncodeError> {
        if text.len() > MAX_UI_COPY_TEXT_BYTES {
            return Err(UiOutputEncodeError::CopyTextLength);
        }
        self.push_command(UI_OUTPUT_COMMAND_COPY_TEXT, 0, text.as_bytes())
    }

    /// Append one clipboard-image command with row-major unpremultiplied sRGBA pixels.
    pub fn copy_image(
        &mut self,
        width: usize,
        height: usize,
        rgba: &[u8],
    ) -> Result<(), UiOutputEncodeError> {
        validate_copy_image(width, height, rgba.len())
            .map_err(|_| UiOutputEncodeError::CopyImage)?;
        let mut header = [0u8; 8];
        header[..4].copy_from_slice(&(width as u32).to_le_bytes());
        header[4..].copy_from_slice(&(height as u32).to_le_bytes());
        self.push_command_parts(UI_OUTPUT_COMMAND_COPY_IMAGE, 0, &header, rgba)
    }

    /// Append one URL navigation command.
    pub fn open_url(&mut self, url: &str, new_surface: bool) -> Result<(), UiOutputEncodeError> {
        if url.is_empty() || url.len() > MAX_UI_OPEN_URL_BYTES {
            return Err(UiOutputEncodeError::OpenUrlLength);
        }
        self.push_command(
            UI_OUTPUT_COMMAND_OPEN_URL,
            u8::from(new_surface) * UI_OUTPUT_OPEN_URL_NEW_SURFACE,
            url.as_bytes(),
        )
    }

    /// Begin one text-viewport command. Rows are appended through the returned
    /// encoder, which must be finished before any other command is appended.
    pub fn text_viewport(
        &mut self,
        descriptor: UiTextViewportDescriptor,
    ) -> Result<UiTextViewportEncoder<'_, 'a>, UiOutputEncodeError> {
        if !descriptor.is_valid() {
            return Err(UiOutputEncodeError::TextViewport);
        }
        let mut header = [0u8; TEXT_VIEWPORT_HEADER_BYTES];
        let (cursor_row, cursor_column) = descriptor.cursor.unwrap_or((TEXT_VIEWPORT_NO_CURSOR, 0));
        for (index, value) in [
            descriptor.sequence,
            descriptor.first_row,
            descriptor.row_count,
            0,
            cursor_row,
            cursor_column,
            descriptor.bounds.min_x.to_bits(),
            descriptor.bounds.min_y.to_bits(),
            descriptor.bounds.max_x.to_bits(),
            descriptor.bounds.max_y.to_bits(),
            descriptor.line_height.to_bits(),
            descriptor.font_size.to_bits(),
            descriptor.scroll_x.to_bits(),
        ]
        .into_iter()
        .enumerate()
        {
            header[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let command_start = self.destination.len();
        self.push_command(UI_OUTPUT_COMMAND_TEXT_VIEWPORT, 0, &header)?;
        Ok(UiTextViewportEncoder {
            encoder: self,
            command_start,
            descriptor,
            changed_count: 0,
            last_row: None,
        })
    }

    /// Finish the stream by writing its total length and command count.
    pub fn finish(self) {
        let length = self.destination.len() as u32;
        self.destination[8..12].copy_from_slice(&length.to_le_bytes());
        self.destination[12..14].copy_from_slice(&self.command_count.to_le_bytes());
    }

    fn push_command(
        &mut self,
        opcode: u8,
        flags: u8,
        payload: &[u8],
    ) -> Result<(), UiOutputEncodeError> {
        self.push_command_parts(opcode, flags, payload, &[])
    }

    fn push_command_parts(
        &mut self,
        opcode: u8,
        flags: u8,
        prefix: &[u8],
        payload: &[u8],
    ) -> Result<(), UiOutputEncodeError> {
        if usize::from(self.command_count) == MAX_UI_OUTPUT_COMMANDS {
            return Err(UiOutputEncodeError::CommandCount);
        }
        let payload_length = prefix
            .len()
            .checked_add(payload.len())
            .ok_or(UiOutputEncodeError::OutputLength)?;
        let added = UI_OUTPUT_COMMAND_HEADER_BYTES
            .checked_add(payload_length)
            .ok_or(UiOutputEncodeError::OutputLength)?;
        let next_length = self
            .destination
            .len()
            .checked_add(added)
            .ok_or(UiOutputEncodeError::OutputLength)?;
        if next_length > MAX_UI_OUTPUT_BYTES || next_length > u32::MAX as usize {
            return Err(UiOutputEncodeError::OutputLength);
        }
        self.destination
            .try_reserve(added)
            .map_err(UiOutputEncodeError::Allocation)?;
        self.destination.push(opcode);
        self.destination.push(flags);
        push_u16(self.destination, 0);
        push_u32(self.destination, payload_length as u32);
        self.destination.extend_from_slice(prefix);
        self.destination.extend_from_slice(payload);
        self.command_count += 1;
        Ok(())
    }
}

/// Appends changed rows to one text-viewport command.
pub struct UiTextViewportEncoder<'e, 'a> {
    encoder: &'e mut UiOutputEncoder<'a>,
    command_start: usize,
    descriptor: UiTextViewportDescriptor,
    changed_count: u32,
    last_row: Option<u32>,
}

impl UiTextViewportEncoder<'_, '_> {
    /// Append one changed row. Rows must be visible, ascending, free of line
    /// terminators, and exactly tiled by `runs`.
    pub fn row(
        &mut self,
        row: u32,
        text: &str,
        runs: &[UiTextRun],
    ) -> Result<(), UiOutputEncodeError> {
        let descriptor = self.descriptor;
        if self.changed_count as usize == MAX_TEXT_VIEWPORT_ROWS
            || row < descriptor.first_row
            || row >= descriptor.first_row + descriptor.row_count
            || self.last_row.is_some_and(|last| row <= last)
            || text.len() > MAX_TEXT_VIEWPORT_ROW_BYTES
            || text.bytes().any(|byte| byte == b'\n' || byte == b'\r')
            || runs.len() > text.len().max(1)
            || runs
                .iter()
                .any(|run| run.len == 0 || run.flags & !TEXT_VIEWPORT_RUN_FLAGS != 0)
            || runs.iter().map(|run| usize::from(run.len)).sum::<usize>() != text.len()
        {
            return Err(UiOutputEncodeError::TextViewport);
        }
        let added =
            TEXT_VIEWPORT_ROW_HEADER_BYTES + text.len() + runs.len() * TEXT_VIEWPORT_RUN_BYTES;
        let destination = &mut *self.encoder.destination;
        let next_length = destination
            .len()
            .checked_add(added)
            .ok_or(UiOutputEncodeError::OutputLength)?;
        if next_length > MAX_UI_OUTPUT_BYTES {
            return Err(UiOutputEncodeError::OutputLength);
        }
        destination
            .try_reserve(added)
            .map_err(UiOutputEncodeError::Allocation)?;
        push_u32(destination, row);
        push_u16(destination, text.len() as u16);
        push_u16(destination, runs.len() as u16);
        destination.extend_from_slice(text.as_bytes());
        for run in runs {
            push_u16(destination, run.len);
            destination.push(run.flags);
            destination.push(0);
            push_u32(destination, run.rgba);
        }
        self.changed_count += 1;
        self.last_row = Some(row);
        Ok(())
    }

    /// Finish the command by writing its payload length and changed-row count.
    pub fn finish(self) {
        let destination = &mut *self.encoder.destination;
        let payload_length =
            destination.len() - self.command_start - UI_OUTPUT_COMMAND_HEADER_BYTES;
        destination[self.command_start + 4..self.command_start + 8]
            .copy_from_slice(&(payload_length as u32).to_le_bytes());
        let changed_at = self.command_start + UI_OUTPUT_COMMAND_HEADER_BYTES + 12;
        destination[changed_at..changed_at + 4].copy_from_slice(&self.changed_count.to_le_bytes());
    }
}

/// Encode failure before a stream is submitted to a Host.
#[derive(Debug)]
pub enum UiOutputEncodeError {
    /// Active IME geometry is non-finite or unordered.
    ImeBounds,
    /// The command count exceeds [`MAX_UI_OUTPUT_COMMANDS`].
    CommandCount,
    /// Clipboard text exceeds [`MAX_UI_COPY_TEXT_BYTES`].
    CopyTextLength,
    /// Clipboard image dimensions or byte length are outside the wire contract.
    CopyImage,
    /// A URL is empty or exceeds [`MAX_UI_OPEN_URL_BYTES`].
    OpenUrlLength,
    /// The complete stream exceeds [`MAX_UI_OUTPUT_BYTES`].
    OutputLength,
    /// Text-viewport geometry or a row violates the wire contract.
    TextViewport,
    /// The destination could not reserve enough memory.
    Allocation(TryReserveError),
}

impl PartialEq for UiOutputEncodeError {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

impl Eq for UiOutputEncodeError {}

impl fmt::Display for UiOutputEncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ImeBounds => "invalid UI IME bounds",
            Self::CommandCount => "too many UI output commands",
            Self::CopyTextLength => "UI clipboard text exceeds the wire limit",
            Self::CopyImage => "UI clipboard image exceeds the wire limits",
            Self::OpenUrlLength => "UI URL is empty or exceeds the wire limit",
            Self::OutputLength => "UI output exceeds the wire limit",
            Self::TextViewport => "UI text viewport violates the wire contract",
            Self::Allocation(_) => "could not allocate the UI output stream",
        })
    }
}

impl core::error::Error for UiOutputEncodeError {}

/// UI output v1 decoding or validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiOutputDecodeError {
    /// The complete byte length is outside the wire bounds.
    Length,
    /// The stream discriminator is not [`UI_OUTPUT_MAGIC`].
    Magic,
    /// The stream version is unsupported.
    Version,
    /// The header size or encoded total size is invalid.
    Header,
    /// The stream contains unknown header flags.
    Flags,
    /// The cursor value is unsupported.
    CursorIcon,
    /// IME geometry is non-finite, unordered, or noncanonical.
    ImeBounds,
    /// The command count exceeds [`MAX_UI_OUTPUT_COMMANDS`].
    CommandCount,
    /// A command header or payload is truncated.
    CommandLength,
    /// A command opcode is unsupported.
    CommandOpcode,
    /// A command contains unsupported flags or reserved data.
    CommandFlags,
    /// Clipboard text exceeds [`MAX_UI_COPY_TEXT_BYTES`].
    CopyTextLength,
    /// A URL is empty or exceeds [`MAX_UI_OPEN_URL_BYTES`].
    OpenUrlLength,
    /// Clipboard image dimensions or byte length are outside the wire contract.
    CopyImage,
    /// A command payload is not valid UTF-8.
    Utf8,
    /// Bytes remain after the declared command sequence.
    TrailingBytes,
    /// A text-viewport command was sent to a Host that does not render one.
    TextViewportUnsupported,
    /// Text-viewport geometry or a row violates the wire contract.
    TextViewport,
}

impl fmt::Display for UiOutputDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Length => "invalid UI output length",
            Self::Magic => "invalid UI output magic",
            Self::Version => "unsupported UI output version",
            Self::Header => "invalid UI output header",
            Self::Flags => "unsupported UI output flags",
            Self::CursorIcon => "unsupported UI cursor icon",
            Self::ImeBounds => "invalid UI IME bounds",
            Self::CommandCount => "too many UI output commands",
            Self::CommandLength => "invalid UI output command length",
            Self::CommandOpcode => "unsupported UI output command",
            Self::CommandFlags => "invalid UI output command flags",
            Self::CopyTextLength => "UI clipboard text exceeds the wire limit",
            Self::OpenUrlLength => "UI URL is empty or exceeds the wire limit",
            Self::CopyImage => "UI clipboard image exceeds the wire limits",
            Self::Utf8 => "UI output command is not valid UTF-8",
            Self::TrailingBytes => "UI output has trailing bytes",
            Self::TextViewportUnsupported => "this Host does not render a UI text viewport",
            Self::TextViewport => "UI text viewport violates the wire contract",
        })
    }
}

impl core::error::Error for UiOutputDecodeError {}

/// Host capabilities that widen the accepted command set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UiOutputDecodeOptions {
    /// The Host renders [`UiOutputCommand::TextViewport`] snapshots.
    pub text_viewport: bool,
}

/// Decode and validate one complete UI output v1 stream for a Host without
/// optional capabilities.
pub fn decode_ui_output(bytes: &[u8]) -> Result<UiOutput<'_>, UiOutputDecodeError> {
    decode_ui_output_with(bytes, UiOutputDecodeOptions::default())
}

/// Decode and validate one complete UI output v1 stream.
pub fn decode_ui_output_with(
    bytes: &[u8],
    options: UiOutputDecodeOptions,
) -> Result<UiOutput<'_>, UiOutputDecodeError> {
    if bytes.len() < UI_OUTPUT_HEADER_BYTES || bytes.len() > MAX_UI_OUTPUT_BYTES {
        return Err(UiOutputDecodeError::Length);
    }
    if bytes[..4] != UI_OUTPUT_MAGIC {
        return Err(UiOutputDecodeError::Magic);
    }
    if read_u16(bytes, 4) != UI_OUTPUT_VERSION {
        return Err(UiOutputDecodeError::Version);
    }
    if read_u16(bytes, 6) as usize != UI_OUTPUT_HEADER_BYTES
        || read_u32(bytes, 8) as usize != bytes.len()
    {
        return Err(UiOutputDecodeError::Header);
    }
    let command_count = read_u16(bytes, 12);
    if usize::from(command_count) > MAX_UI_OUTPUT_COMMANDS {
        return Err(UiOutputDecodeError::CommandCount);
    }
    let cursor_icon = UiCursorIcon::try_from(bytes[14])?;
    let flags = bytes[15];
    if flags & !UI_OUTPUT_FLAGS_V1 != 0 {
        return Err(UiOutputDecodeError::Flags);
    }
    let rect = UiRect {
        min_x: read_f32(bytes, 16),
        min_y: read_f32(bytes, 20),
        max_x: read_f32(bytes, 24),
        max_y: read_f32(bytes, 28),
    };
    let cursor_rect = UiRect {
        min_x: read_f32(bytes, 32),
        min_y: read_f32(bytes, 36),
        max_x: read_f32(bytes, 40),
        max_y: read_f32(bytes, 44),
    };
    let ime = if flags & UI_OUTPUT_FLAG_IME != 0 {
        let ime = UiImeOutput { rect, cursor_rect };
        if !ime.is_valid() {
            return Err(UiOutputDecodeError::ImeBounds);
        }
        Some(ime)
    } else {
        if bytes[16..UI_OUTPUT_HEADER_BYTES]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(UiOutputDecodeError::ImeBounds);
        }
        None
    };

    let mut offset = UI_OUTPUT_HEADER_BYTES;
    for _ in 0..command_count {
        let header_end = offset
            .checked_add(UI_OUTPUT_COMMAND_HEADER_BYTES)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        let header = bytes
            .get(offset..header_end)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        let opcode = header[0];
        let command_flags = header[1];
        if header[2] != 0 || header[3] != 0 {
            return Err(UiOutputDecodeError::CommandFlags);
        }
        let payload_length = read_u32(header, 4) as usize;
        let end = header_end
            .checked_add(payload_length)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        let payload = bytes
            .get(header_end..end)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        match opcode {
            UI_OUTPUT_COMMAND_COPY_TEXT => {
                if command_flags != 0 {
                    return Err(UiOutputDecodeError::CommandFlags);
                }
                if payload.len() > MAX_UI_COPY_TEXT_BYTES {
                    return Err(UiOutputDecodeError::CopyTextLength);
                }
                core::str::from_utf8(payload).map_err(|_| UiOutputDecodeError::Utf8)?;
            }
            UI_OUTPUT_COMMAND_OPEN_URL => {
                if command_flags & !UI_OUTPUT_OPEN_URL_NEW_SURFACE != 0 {
                    return Err(UiOutputDecodeError::CommandFlags);
                }
                if payload.is_empty() || payload.len() > MAX_UI_OPEN_URL_BYTES {
                    return Err(UiOutputDecodeError::OpenUrlLength);
                }
                core::str::from_utf8(payload).map_err(|_| UiOutputDecodeError::Utf8)?;
            }
            UI_OUTPUT_COMMAND_COPY_IMAGE => {
                if command_flags != 0 {
                    return Err(UiOutputDecodeError::CommandFlags);
                }
                let rgba = payload.get(8..).ok_or(UiOutputDecodeError::CopyImage)?;
                let width = read_u32(payload, 0) as usize;
                let height = read_u32(payload, 4) as usize;
                validate_copy_image(width, height, rgba.len())
                    .map_err(|_| UiOutputDecodeError::CopyImage)?;
            }
            UI_OUTPUT_COMMAND_TEXT_VIEWPORT => {
                if command_flags != 0 {
                    return Err(UiOutputDecodeError::CommandFlags);
                }
                if !options.text_viewport {
                    return Err(UiOutputDecodeError::TextViewportUnsupported);
                }
                validate_text_viewport(payload)?;
            }
            _ => return Err(UiOutputDecodeError::CommandOpcode),
        }
        offset = end;
    }
    if offset != bytes.len() {
        return Err(UiOutputDecodeError::TrailingBytes);
    }

    Ok(UiOutput {
        bytes,
        descriptor: UiOutputDescriptor {
            cursor_icon,
            mutable_text_under_cursor: flags & UI_OUTPUT_FLAG_MUTABLE_TEXT != 0,
            ime,
        },
        command_count,
    })
}

fn read_text_viewport_descriptor(payload: &[u8]) -> (UiTextViewportDescriptor, u32) {
    let cursor_row = read_u32(payload, 16);
    let descriptor = UiTextViewportDescriptor {
        sequence: read_u32(payload, 0),
        first_row: read_u32(payload, 4),
        row_count: read_u32(payload, 8),
        cursor: (cursor_row != TEXT_VIEWPORT_NO_CURSOR)
            .then(|| (cursor_row, read_u32(payload, 20))),
        bounds: UiRect {
            min_x: read_f32(payload, 24),
            min_y: read_f32(payload, 28),
            max_x: read_f32(payload, 32),
            max_y: read_f32(payload, 36),
        },
        line_height: read_f32(payload, 40),
        font_size: read_f32(payload, 44),
        scroll_x: read_f32(payload, 48),
    };
    (descriptor, read_u32(payload, 12))
}

fn validate_text_viewport(payload: &[u8]) -> Result<(), UiOutputDecodeError> {
    let invalid = UiOutputDecodeError::TextViewport;
    if payload.len() < TEXT_VIEWPORT_HEADER_BYTES {
        return Err(invalid);
    }
    let (descriptor, changed_count) = read_text_viewport_descriptor(payload);
    if !descriptor.is_valid()
        || changed_count > descriptor.row_count
        || (descriptor.cursor.is_none() && read_u32(payload, 20) != 0)
    {
        return Err(invalid);
    }
    let mut rows = &payload[TEXT_VIEWPORT_HEADER_BYTES..];
    let mut last_row = None;
    for _ in 0..changed_count {
        if rows.len() < TEXT_VIEWPORT_ROW_HEADER_BYTES {
            return Err(invalid);
        }
        let row = read_u32(rows, 0);
        let text_len = usize::from(read_u16(rows, 4));
        let run_count = usize::from(read_u16(rows, 6));
        if row < descriptor.first_row
            || row >= descriptor.first_row + descriptor.row_count
            || last_row.is_some_and(|last| row <= last)
            || run_count > text_len.max(1)
        {
            return Err(invalid);
        }
        let runs_start = TEXT_VIEWPORT_ROW_HEADER_BYTES + text_len;
        let end = runs_start + run_count * TEXT_VIEWPORT_RUN_BYTES;
        let text = rows
            .get(TEXT_VIEWPORT_ROW_HEADER_BYTES..runs_start)
            .ok_or(invalid)?;
        let runs = rows.get(runs_start..end).ok_or(invalid)?;
        if text.iter().any(|byte| *byte == b'\n' || *byte == b'\r') {
            return Err(invalid);
        }
        core::str::from_utf8(text).map_err(|_| UiOutputDecodeError::Utf8)?;
        let mut covered = 0usize;
        for run in runs.chunks_exact(TEXT_VIEWPORT_RUN_BYTES) {
            let len = usize::from(read_u16(run, 0));
            if len == 0 || run[2] & !TEXT_VIEWPORT_RUN_FLAGS != 0 || run[3] != 0 {
                return Err(invalid);
            }
            covered += len;
        }
        if covered != text_len {
            return Err(invalid);
        }
        last_row = Some(row);
        rows = &rows[end..];
    }
    if !rows.is_empty() {
        return Err(invalid);
    }
    Ok(())
}

fn text_viewport_from_validated(payload: &[u8]) -> UiTextViewport<'_> {
    let (descriptor, changed_count) = read_text_viewport_descriptor(payload);
    UiTextViewport {
        descriptor,
        changed_count,
        rows: &payload[TEXT_VIEWPORT_HEADER_BYTES..],
    }
}

fn validate_copy_image(width: usize, height: usize, rgba_bytes: usize) -> Result<(), ()> {
    if width == 0
        || height == 0
        || width > MAX_UI_COPY_IMAGE_DIMENSION
        || height > MAX_UI_COPY_IMAGE_DIMENSION
        || width
            .checked_mul(height)
            .filter(|pixels| *pixels <= MAX_UI_COPY_IMAGE_PIXELS)
            .and_then(|pixels| pixels.checked_mul(4))
            != Some(rgba_bytes)
    {
        return Err(());
    }
    Ok(())
}

fn push_u16(destination: &mut Vec<u8>, value: u16) {
    destination.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(destination: &mut Vec<u8>, value: u32) {
    destination.extend_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("fixed field"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed field"))
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(read_u32(bytes, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn descriptor() -> UiOutputDescriptor {
        UiOutputDescriptor {
            cursor_icon: UiCursorIcon::Text,
            mutable_text_under_cursor: true,
            ime: Some(UiImeOutput {
                rect: UiRect {
                    min_x: 10.0,
                    min_y: 20.0,
                    max_x: 210.0,
                    max_y: 60.0,
                },
                cursor_rect: UiRect {
                    min_x: 24.0,
                    min_y: 22.0,
                    max_x: 25.0,
                    max_y: 58.0,
                },
            }),
        }
    }

    #[test]
    fn output_round_trips_without_allocating_during_decode() {
        let mut bytes = Vec::new();
        let mut encoder = UiOutputEncoder::begin(&mut bytes, descriptor()).unwrap();
        encoder.copy_text("hello 🦀").unwrap();
        encoder.open_url("https://example.test/path", true).unwrap();
        encoder
            .copy_image(2, 1, &[255, 0, 0, 255, 0, 255, 0, 128])
            .unwrap();
        encoder.finish();

        let output = decode_ui_output(&bytes).unwrap();
        assert_eq!(output.descriptor(), descriptor());
        assert_eq!(output.command_count(), 3);
        assert_eq!(
            output.commands().collect::<Vec<_>>(),
            vec![
                UiOutputCommand::CopyText("hello 🦀"),
                UiOutputCommand::OpenUrl {
                    url: "https://example.test/path",
                    new_surface: true,
                },
                UiOutputCommand::CopyImage {
                    width: 2,
                    height: 1,
                    rgba: &[255, 0, 0, 255, 0, 255, 0, 128],
                },
            ]
        );
    }

    #[test]
    fn every_cursor_value_round_trips() {
        for value in 0..=UiCursorIcon::ZoomOut as u8 {
            let mut bytes = Vec::new();
            UiOutputEncoder::begin(
                &mut bytes,
                UiOutputDescriptor {
                    cursor_icon: UiCursorIcon::try_from(value).unwrap(),
                    ..UiOutputDescriptor::default()
                },
            )
            .unwrap()
            .finish();
            assert_eq!(
                decode_ui_output(&bytes).unwrap().descriptor().cursor_icon as u8,
                value
            );
        }
    }

    #[test]
    fn malformed_headers_and_commands_are_rejected() {
        let mut bytes = Vec::new();
        UiOutputEncoder::begin(&mut bytes, UiOutputDescriptor::default())
            .unwrap()
            .finish();

        let mut invalid = bytes.clone();
        invalid[14] = 255;
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::CursorIcon
        );

        let mut invalid = bytes.clone();
        invalid[16] = 1;
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::ImeBounds
        );

        let mut invalid = bytes.clone();
        invalid[12..14].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::CommandLength
        );

        let mut invalid = bytes;
        invalid.extend_from_slice(&[UI_OUTPUT_COMMAND_COPY_TEXT, 0, 0, 0, 1, 0, 0, 0, 0xff]);
        let invalid_length = invalid.len() as u32;
        invalid[8..12].copy_from_slice(&invalid_length.to_le_bytes());
        invalid[12..14].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::Utf8
        );
    }

    #[test]
    fn encoder_enforces_command_limits() {
        let mut bytes = Vec::new();
        let mut encoder =
            UiOutputEncoder::begin(&mut bytes, UiOutputDescriptor::default()).unwrap();
        assert_eq!(
            encoder.open_url("", false).unwrap_err(),
            UiOutputEncodeError::OpenUrlLength
        );
        for _ in 0..MAX_UI_OUTPUT_COMMANDS {
            encoder.copy_text("").unwrap();
        }
        assert_eq!(
            encoder.copy_text("").unwrap_err(),
            UiOutputEncodeError::CommandCount
        );
    }

    fn viewport() -> UiTextViewportDescriptor {
        UiTextViewportDescriptor {
            sequence: 7,
            first_row: 40,
            row_count: 3,
            cursor: Some((41, 4)),
            bounds: UiRect {
                min_x: 60.0,
                min_y: 30.0,
                max_x: 800.0,
                max_y: 600.0,
            },
            line_height: 20.0,
            font_size: 14.0,
            scroll_x: 0.0,
        }
    }

    fn encoded_viewport() -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder =
            UiOutputEncoder::begin(&mut bytes, UiOutputDescriptor::default()).unwrap();
        let mut rows = encoder.text_viewport(viewport()).unwrap();
        rows.row(
            40,
            "fn main() {",
            &[
                UiTextRun {
                    len: 2,
                    rgba: 0xff0000ff,
                    flags: TEXT_VIEWPORT_RUN_BOLD,
                },
                UiTextRun {
                    len: 9,
                    rgba: 0x00ff00ff,
                    flags: 0,
                },
            ],
        )
        .unwrap();
        rows.row(42, "", &[]).unwrap();
        rows.finish();
        encoder.copy_text("after").unwrap();
        encoder.finish();
        bytes
    }

    #[test]
    fn text_viewport_round_trips_and_requires_host_support() {
        let bytes = encoded_viewport();
        assert_eq!(
            decode_ui_output(&bytes).unwrap_err(),
            UiOutputDecodeError::TextViewportUnsupported
        );
        let options = UiOutputDecodeOptions {
            text_viewport: true,
        };
        let output = decode_ui_output_with(&bytes, options).unwrap();
        let mut commands = output.commands();
        let Some(UiOutputCommand::TextViewport(snapshot)) = commands.next() else {
            panic!("expected a text viewport");
        };
        assert_eq!(snapshot.descriptor(), viewport());
        assert_eq!(snapshot.changed_count(), 2);
        let rows: Vec<_> = snapshot.rows().collect();
        assert_eq!(rows[0].row, 40);
        assert_eq!(rows[0].text, "fn main() {");
        assert_eq!(
            rows[0].runs().collect::<Vec<_>>(),
            vec![
                UiTextRun {
                    len: 2,
                    rgba: 0xff0000ff,
                    flags: TEXT_VIEWPORT_RUN_BOLD,
                },
                UiTextRun {
                    len: 9,
                    rgba: 0x00ff00ff,
                    flags: 0,
                },
            ]
        );
        assert_eq!(rows[1].row, 42);
        assert_eq!(rows[1].text, "");
        assert_eq!(rows[1].runs().len(), 0);
        assert_eq!(commands.next(), Some(UiOutputCommand::CopyText("after")));
    }

    #[test]
    fn text_viewport_rows_must_tile_text_and_stay_visible() {
        let mut bytes = Vec::new();
        let mut encoder =
            UiOutputEncoder::begin(&mut bytes, UiOutputDescriptor::default()).unwrap();
        let mut rows = encoder.text_viewport(viewport()).unwrap();
        let run = |len| UiTextRun {
            len,
            rgba: 0,
            flags: 0,
        };
        assert_eq!(
            rows.row(43, "x", &[run(1)]).unwrap_err(),
            UiOutputEncodeError::TextViewport
        );
        assert_eq!(
            rows.row(40, "ab", &[run(1)]).unwrap_err(),
            UiOutputEncodeError::TextViewport
        );
        assert_eq!(
            rows.row(40, "a\nb", &[run(3)]).unwrap_err(),
            UiOutputEncodeError::TextViewport
        );
        rows.row(41, "ab", &[run(2)]).unwrap();
        assert_eq!(
            rows.row(40, "ab", &[run(2)]).unwrap_err(),
            UiOutputEncodeError::TextViewport
        );
        let mut invalid = viewport();
        invalid.cursor = Some((50, 0));
        drop(rows);
        assert!(matches!(
            encoder.text_viewport(invalid).map(drop),
            Err(UiOutputEncodeError::TextViewport)
        ));
    }

    #[test]
    fn corrupted_text_viewport_rows_are_rejected() {
        let options = UiOutputDecodeOptions {
            text_viewport: true,
        };
        let bytes = encoded_viewport();
        let row_header =
            UI_OUTPUT_HEADER_BYTES + UI_OUTPUT_COMMAND_HEADER_BYTES + TEXT_VIEWPORT_HEADER_BYTES;
        // Row moved outside the viewport.
        let mut moved = bytes.clone();
        moved[row_header..row_header + 4].copy_from_slice(&39u32.to_le_bytes());
        assert_eq!(
            decode_ui_output_with(&moved, options).unwrap_err(),
            UiOutputDecodeError::TextViewport
        );
        // Run no longer tiles the text.
        let mut untiled = bytes.clone();
        let first_run = row_header + TEXT_VIEWPORT_ROW_HEADER_BYTES + "fn main() {".len();
        untiled[first_run..first_run + 2].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(
            decode_ui_output_with(&untiled, options).unwrap_err(),
            UiOutputDecodeError::TextViewport
        );
        // Reserved run byte set.
        let mut reserved = bytes.clone();
        reserved[first_run + 3] = 1;
        assert_eq!(
            decode_ui_output_with(&reserved, options).unwrap_err(),
            UiOutputDecodeError::TextViewport
        );
        // Line terminator inside row text.
        let mut terminated = bytes;
        terminated[row_header + TEXT_VIEWPORT_ROW_HEADER_BYTES] = b'\n';
        assert_eq!(
            decode_ui_output_with(&terminated, options).unwrap_err(),
            UiOutputDecodeError::TextViewport
        );
    }
}
