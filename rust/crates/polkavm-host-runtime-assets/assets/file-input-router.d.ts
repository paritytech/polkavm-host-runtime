/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

interface FileInputDescriptorFields {
  id: string;
  label: string;
  extensions: readonly string[];
  mimeTypes: readonly string[];
  maxBytes: number;
}
export type FileInputDescriptor = FileInputDescriptorFields & (
  | { delivery: "inline" | "stream"; mountPath?: never }
  | { delivery: "relaunch"; mountPath: string }
);
/** One live execution registration from the runtime's file-registrations output. */
export interface FileInputRegistration {
  handle: number;
  descriptor: FileInputDescriptor;
}
export interface FileInputProduct {
  id: string;
  entrypoint: string;
  /** Replace with current runtime output; discovery fileTypes are not registrations. */
  registrations: readonly FileInputRegistration[];
}
export interface FileInputCandidate<P extends FileInputProduct = FileInputProduct> {
  readonly product: P;
  readonly handle: number;
  readonly handler: Readonly<FileInputDescriptor>;
}
export interface FileInputMetadata {
  name: string;
  size: number;
  type?: string;
}
export interface FileInputFile extends FileInputMetadata {
  arrayBuffer(): Promise<ArrayBuffer>;
}
/** The runtime, not this router, decides whether an approved file causes relaunch. */
export type FileInputRuntimeMessage = {
  type: "file-input";
  handle: number;
  name: string;
  mimeType: string;
} & (
  | { bytes: Uint8Array; file?: never }
  | { file: Blob; bytes?: never }
);
export type FileInputResult<P extends FileInputProduct = FileInputProduct> =
  | { status: "unhandled" | "cancelled" }
  | { status: "ambiguous"; candidates: FileInputCandidate<P>[] }
  | { status: "declined" | "delivered"; candidate: FileInputCandidate<P> }
  | { status: "rejected"; candidate?: FileInputCandidate<P>; error: unknown };
export interface FileInputCallbacks<P extends FileInputProduct = FileInputProduct> {
  products: readonly P[];
  /** Return one of the supplied candidates, or a falsy cancellation value. */
  chooseCandidate?: (selection: {
    file: FileInputFile;
    candidates: FileInputCandidate<P>[];
  }) => FileInputCandidate<P> | null | undefined | false |
    PromiseLike<FileInputCandidate<P> | null | undefined | false>;
  confirmDelivery: (selection: FileInputCandidate<P> & {
    file: Required<FileInputMetadata>;
  }) => boolean | PromiseLike<boolean>;
  /** Bind this callback to the execution that supplied the registrations. */
  sendToRuntime: (delivery: FileInputCandidate<P> & {
    message: FileInputRuntimeMessage;
  }) => unknown;
}
/** Validates registrations and matches metadata without reading bytes. Throws on malformed registrations. */
export function routeFileInput<P extends FileInputProduct>(
  products: readonly P[], file: FileInputMetadata,
): FileInputCandidate<P>[];
/** Throws on malformed registrations; does not inspect file contents. */
export function filePickerAccept<P extends FileInputProduct>(products: readonly P[]): string;
/**
 * Consents before reading inline/relaunch bytes or passing the original stream Blob.
 * Stream sources must be Blob instances with a name (for example, browser File).
 * Stale selection/registration/metadata returns cancelled; errors return rejected.
 * Delivered means the host callback completed, not that the guest accepted the file.
 */
export function deliverFileInput<P extends FileInputProduct>(
  options: FileInputCallbacks<P> & { file: FileInputFile },
): Promise<FileInputResult<P>>;
export interface FileInputControls<P extends FileInputProduct = FileInputProduct> {
  readonly picker: HTMLInputElement;
  deliver(file: FileInputFile): Promise<FileInputResult<P>>;
  /** Cancels pending delivery before handoff and suppresses later result callbacks. */
  dispose(): void;
}
/** Throws on invalid setup. Per-delivery failures are passed to onResult. */
export function attachFileInputControls<P extends FileInputProduct>(
  options: FileInputCallbacks<P> & {
    dropTarget: HTMLElement;
    openButton: HTMLElement;
    onResult?: (result: FileInputResult<P>) => unknown;
  },
): FileInputControls<P>;
