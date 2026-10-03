/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

export interface FileInputHandler {
  id: string;
  label: string;
  extensions?: readonly string[];
  mediaTypes?: readonly string[];
  maxBytes: number;
  mountPath: string;
}
export interface FileInputProduct {
  id: string;
  manifest: {
    /** Required when fileInput handlers are declared. */
    runtime?: { entrypoint: string };
    capabilities?: {
      fileInput?: { abiVersion: 1; handlers: readonly FileInputHandler[] } | null;
    };
  };
}
export interface NormalizedFileInputHandler extends FileInputHandler {
  extensions: string[];
  mediaTypes: string[];
}
export interface FileInputCandidate<P extends FileInputProduct = FileInputProduct> {
  product: P;
  handler: NormalizedFileInputHandler;
}
export interface FileInputMetadata {
  name: string;
  size: number;
  type?: string;
}
export interface FileInputFile extends FileInputMetadata {
  arrayBuffer(): Promise<ArrayBuffer>;
}
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
  launchProduct: (delivery: FileInputCandidate<P> & {
    asset: { path: string; bytes: Uint8Array };
  }) => unknown;
}
/** Validates registrations and matches metadata without reading bytes. Throws on malformed registrations. */
export function routeFileInput<P extends FileInputProduct>(
  products: readonly P[], file: FileInputMetadata,
): FileInputCandidate<P>[];
/** Throws on malformed registrations; does not inspect file contents. */
export function filePickerAccept<P extends FileInputProduct>(products: readonly P[]): string;
/** Reads bytes only after consent. Validation and callback failures return a rejected result. */
export function deliverFileInput<P extends FileInputProduct>(
  options: FileInputCallbacks<P> & { file: FileInputFile },
): Promise<FileInputResult<P>>;
export interface FileInputControls<P extends FileInputProduct = FileInputProduct> {
  readonly picker: HTMLInputElement;
  deliver(file: FileInputFile): Promise<FileInputResult<P>>;
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
