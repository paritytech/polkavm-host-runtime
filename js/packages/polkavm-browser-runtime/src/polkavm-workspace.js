/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

"use strict";

(() => {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const PREFIX = encoder.encode("PVMFS001");
  const MAX_FRAME_BYTES = 1024 * 1024;
  const MAX_REQUESTS = 32;
  const MAX_QUEUED_BYTES = 4 * 1024 * 1024;
  const CHUNK_BYTES = 262144;
  const LIMITS = Object.freeze({
    maxFiles: 64,
    maxDirectories: 256, // In addition to the fixed /home root.
    maxFileBytes: 1048576,
    maxPathBytes: 200,
  });
  const EMPTY = new Uint8Array();
  const C = globalThis.PolkaVmComputer;

  function isWorkspaceFrame(bytes) {
    return (
      bytes instanceof Uint8Array &&
      bytes.length >= PREFIX.length &&
      PREFIX.every((byte, index) => bytes[index] === byte)
    );
  }

  function decodeFrame(bytes) {
    if (
      !isWorkspaceFrame(bytes) ||
      bytes.length < 12 ||
      bytes.length > MAX_FRAME_BYTES
    ) {
      throw new Error("invalid workspace frame envelope");
    }
    const length = new DataView(
      bytes.buffer,
      bytes.byteOffset,
      bytes.byteLength,
    ).getUint32(8, true);
    if (length === 0 || length > bytes.length - 12) {
      throw new Error("truncated workspace frame header");
    }
    const header = JSON.parse(decoder.decode(bytes.subarray(12, 12 + length)));
    if (
      header === null ||
      typeof header !== "object" ||
      Array.isArray(header)
    ) {
      throw new Error("invalid workspace frame header");
    }
    return { header, payload: bytes.subarray(12 + length) };
  }

  function encodeFrame(header, payload = EMPTY) {
    const json = encoder.encode(JSON.stringify(header));
    if (
      !(payload instanceof Uint8Array) ||
      12 + json.length + payload.length > MAX_FRAME_BYTES
    ) {
      throw new Error("workspace frame exceeds the host limit");
    }
    const bytes = new Uint8Array(12 + json.length + payload.length);
    bytes.set(PREFIX);
    new DataView(bytes.buffer).setUint32(8, json.length, true);
    bytes.set(json, 12);
    bytes.set(payload, 12 + json.length);
    return bytes;
  }

  function fail(status, message) {
    const error = new Error(message);
    error.status = status;
    throw error;
  }

  function check(status, operation) {
    if (status < 0) fail(status, `${operation} failed (${status})`);
    return status;
  }

  function keys(value, allowed) {
    return (
      value !== null &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      Object.keys(value).every((key) => allowed.includes(key))
    );
  }

  function integer(value, max) {
    return Number.isInteger(value) && value >= 0 && value <= max;
  }

  function decimal(value) {
    return (
      typeof value === "string" &&
      /^(0|[1-9][0-9]{0,19})$/.test(value) &&
      BigInt(value) <= 0xffffffffffffffffn
    );
  }

  const OPERATIONS = {
    workspace: [],
    metadata: ["path"],
    list: ["path"],
    read: ["path", "offset", "capacity", "inode", "mtimeNs"],
    begin_write: ["path", "length", "overwrite", "ignoreIfExists"],
    write_chunk: ["offset"],
    commit_write: [],
    mkdir: ["path"],
    rename: ["path", "to", "overwrite", "ignoreIfExists", "createParents"],
    remove: ["path", "kind", "recursive", "ignoreIfNotExists"],
    copy: ["path", "to", "overwrite", "ignoreIfExists"],
    readable_copy: ["path"],
  };

  /** A private Computer namespace. respond resolves only after the runtime has
   * accepted the frame; commit resolves only after the Host's durable transaction.
   * Neither hook, nor a project identifier, is accessible through guest frames. */
  class WorkspaceService {
    #devices = null;
    #info;
    #commit;
    #format;
    #respond;
    #fatal;
    #queue = [];
    #ids = new Set();
    #queuedBytes = 0;
    #busy = false;
    #closed = false;
    #upload = null;
    #rollback = null;
    #cancelCommit = null;

    constructor(snapshot, { commit, format, respond, fatal }) {
      if (
        !C ||
        typeof respond !== "function" ||
        typeof fatal !== "function" ||
        typeof commit !== "function"
      ) {
        throw new Error(
          "workspace service requires ComputerDevices and Host callbacks",
        );
      }
      this.#commit = commit;
      this.#format = format;
      this.#respond = respond;
      this.#fatal = fatal;
      if (snapshot == null) return;
      if (
        !keys(snapshot, ["name", "files", "metadata"]) ||
        typeof snapshot.name !== "string" ||
        decoder.decode(encoder.encode(snapshot.name)) !== snapshot.name ||
        !Array.isArray(snapshot.files) ||
        snapshot.files.length > LIMITS.maxFiles
      ) {
        throw new Error("invalid workspace snapshot");
      }
      const metadata = snapshot.metadata;
      if (
        !metadata ||
        !decimal(metadata.nextInode) ||
        !decimal(metadata.clockNs) ||
        !Array.isArray(metadata.entries) ||
        metadata.entries.length > LIMITS.maxFiles + LIMITS.maxDirectories + 1 ||
        metadata.entries.some(
          (entry) => !entry || !decimal(entry.inode) || !decimal(entry.mtimeNs),
        )
      ) {
        throw new Error("invalid workspace filesystem metadata");
      }
      this.#info = encoder.encode(
        JSON.stringify({ root: "/home", name: snapshot.name, limits: LIMITS }),
      );
      // Check the complete reply bound before any guest initialization.
      encodeFrame({ id: 0xffffffff, status: 0 }, this.#info);
      const devices = new C.ComputerDevices();
      try {
        const paths = new Set();
        for (const file of snapshot.files) {
          if (
            !keys(file, ["path", "bytes"]) ||
            typeof file.path !== "string" ||
            !(file.bytes instanceof Uint8Array) ||
            paths.has(file.path)
          ) {
            throw new Error("invalid or duplicate workspace file");
          }
          paths.add(file.path);
          devices.mountFile(file.path, file.bytes);
        }
        // Import enforces exact byte/metadata correspondence, unique inodes,
        // canonical paths, explicit parent directories and version-1 counters.
        devices.importFilesystemMetadata(snapshot.metadata);
        this.#devices = devices;
      } catch (error) {
        devices.dispose();
        throw error;
      }
    }

    /** Retains ownership of bytes until the bounded serialized request finishes.
     * Uncorrelatable frames/duplicate active IDs are fatal rather than hanging. */
    receive(bytes) {
      if (this.#closed) throw new Error("workspace service is stopped");
      const request = decodeFrame(bytes);
      const { id } = request.header;
      if (!integer(id, 0xffffffff) || id === 0 || this.#ids.has(id)) {
        throw new Error("invalid or duplicate workspace request id");
      }
      if (
        this.#ids.size >= MAX_REQUESTS ||
        this.#queuedBytes + bytes.length > MAX_QUEUED_BYTES
      ) {
        throw new Error("workspace request queue limit exceeded");
      }
      this.#ids.add(id);
      this.#queuedBytes += bytes.length;
      this.#queue.push({ ...request, length: bytes.length });
      void this.#drain();
    }

    stop() {
      if (this.#closed) return;
      this.#closed = true;
      this.#upload = null;
      this.#queue.length = 0;
      this.#ids.clear();
      this.#queuedBytes = 0;
      this.#cancelCommit?.(
        new Error("workspace service stopped before commit acknowledgement"),
      );
      this.#restore();
      this.#devices?.dispose();
    }

    async #drain() {
      if (this.#busy) return;
      this.#busy = true;
      try {
        while (!this.#closed && this.#queue.length) {
          const { header, payload, length } = this.#queue.shift();
          let result;
          try {
            this.#validate(header, payload);
            if (!this.#devices)
              fail(C.STATUS_DENIED, "workspace capability is not enabled");
            result = await this.#execute(header, payload);
          } catch (error) {
            if (
              ["begin_write", "write_chunk", "commit_write"].includes(header.op)
            ) {
              this.#upload = null;
            }
            result = {
              status: Number.isInteger(error.status) ? error.status : -1,
              error: String(error.message ?? error).slice(0, 2048),
            };
          }
          if (this.#closed) break;
          const { payload: responsePayload, ...response } = result;
          await this.#respond(
            encodeFrame({ id: header.id, ...response }, responsePayload),
          );
          if (this.#closed) break;
          this.#ids.delete(header.id);
          this.#queuedBytes -= length;
        }
      } catch (error) {
        this.stop();
        this.#fatal(error);
      } finally {
        this.#busy = false;
      }
    }

    #validate(header, payload) {
      const fields =
        typeof header.op === "string" && Object.hasOwn(OPERATIONS, header.op)
          ? OPERATIONS[header.op]
          : null;
      if (!fields || !keys(header, ["id", "op", ...fields])) {
        fail(C.STATUS_INVALID, "invalid workspace operation or arguments");
      }
      if (header.op !== "write_chunk" && payload.length !== 0) {
        fail(C.STATUS_INVALID, "unexpected workspace request payload");
      }
      for (const name of fields) {
        const value = header[name];
        if (name === "inode" || name === "mtimeNs") {
          if (value !== undefined && !decimal(value))
            fail(C.STATUS_INVALID, `invalid ${name}`);
        } else if (name === "path" || name === "to") {
          if (typeof value !== "string")
            fail(C.STATUS_INVALID, `invalid ${name}`);
          // Reuse Computer's path rules even for recursive operations below.
          if (this.#devices?.fsMetadata(value).status === C.STATUS_INVALID) {
            fail(C.STATUS_INVALID, `invalid workspace ${name}`);
          }
        } else if (
          [
            "overwrite",
            "ignoreIfExists",
            "createParents",
            "recursive",
            "ignoreIfNotExists",
          ].includes(name)
        ) {
          if (typeof value !== "boolean")
            fail(C.STATUS_INVALID, `invalid ${name}`);
        } else if (name === "kind") {
          if (value !== 1 && value !== 2)
            fail(C.STATUS_INVALID, "invalid remove kind");
        } else if (
          !integer(
            value,
            name === "capacity" ? CHUNK_BYTES : LIMITS.maxFileBytes,
          )
        ) {
          fail(C.STATUS_INVALID, `invalid ${name}`);
        }
      }
      if (payload.length > CHUNK_BYTES)
        fail(C.STATUS_LIMIT, "workspace chunk exceeds limit");
    }

    #entry(path) {
      const result = this.#devices.fsMetadata(path);
      check(result.status, `metadata ${path}`);
      const view = new DataView(result.record.buffer);
      return {
        kind: view.getUint32(0, true),
        inode: view.getBigUint64(16, true),
        mtimeNs: view.getBigUint64(8, true),
      };
    }

    #destination(path, overwrite, ignoreIfExists) {
      const result = this.#devices.fsMetadata(path);
      if (result.status === C.STATUS_NOT_FOUND) return false;
      check(result.status, `metadata ${path}`);
      if (overwrite) return false;
      if (ignoreIfExists) return true;
      fail(C.STATUS_EXISTS, `workspace destination exists: ${path}`);
    }

    #prepareWrite(path, overwrite, ignoreIfExists) {
      if (this.#destination(path, overwrite, ignoreIfExists)) return true;
      const fs = this.#devices.filesystem;
      if (fs.entries.get(path)?.kind === 2)
        fail(C.STATUS_IS_DIRECTORY, `${path} is a directory`);
      check(fs.parentStatus(path), `parent of ${path}`);
      if (!fs.entries.has(path) && fs.files.size >= LIMITS.maxFiles) {
        fail(C.STATUS_LIMIT, "workspace file limit exceeded");
      }
      if (!fs.canMutate(fs.entries.has(path) ? 0 : 1)) {
        fail(C.STATUS_LIMIT, "workspace metadata limit exceeded");
      }
      return false;
    }

    #readablePath(path) {
      const slash = path.lastIndexOf("/");
      const dot = path.lastIndexOf(".");
      const extension = dot > slash + 1 ? path.slice(dot) : "";
      const stem = extension ? path.slice(0, dot) : path;
      for (let suffix = 1; ; suffix++) {
        const target = `${stem}.readable${suffix === 1 ? "" : `-${suffix}`}${extension}`;
        if (encoder.encode(target).length > LIMITS.maxPathBytes)
          fail(
            C.STATUS_LIMIT,
            "readable copy path exceeds workspace path limit",
          );
        const result = this.#devices.fsMetadata(target);
        if (result.status === C.STATUS_NOT_FOUND) return target;
        check(result.status, `metadata ${target}`);
      }
    }

    #replaceFile(path, bytes) {
      const handle = check(
        this.#devices.fsOpen(
          path,
          C.FS_OPEN_WRITE | C.FS_OPEN_CREATE | C.FS_OPEN_TRUNCATE,
        ),
        `open ${path}`,
      );
      try {
        // TRUNCATE replaces the old buffer before fsWrite. Rollback references
        // therefore cannot be modified even for shorter or same-length writes.
        const written = check(
          this.#devices.fsWrite(handle, bytes),
          `write ${path}`,
        );
        if (written !== bytes.length)
          fail(-1, `incomplete workspace write: ${path}`);
      } finally {
        this.#devices.fsClose(handle);
      }
    }

    #mkdir(path) {
      let current = "/home";
      let created = false;
      for (const part of path.slice(6).split("/")) {
        if (!part) continue;
        current += `/${part}`;
        const result = this.#devices.fsMetadata(current);
        if (result.status === C.STATUS_NOT_FOUND) {
          check(this.#devices.fsMkdir(current), `mkdir ${current}`);
          created = true;
        } else {
          check(result.status, `metadata ${current}`);
          if (new DataView(result.record.buffer).getUint32(0, true) !== 2) {
            fail(C.STATUS_NOT_DIRECTORY, `${current} is not a directory`);
          }
        }
      }
      return created ? { status: 0 } : { status: 0, ignored: true };
    }

    #restore() {
      if (!this.#rollback) return;
      const { files, metadata } = this.#rollback;
      for (const handle of this.#devices.openFiles.keys())
        this.#devices.fsClose(handle);
      this.#devices.files.clear();
      for (const [path, bytes] of files) this.#devices.files.set(path, bytes);
      this.#devices.modified.clear();
      this.#devices.removed.clear();
      this.#devices.importFilesystemMetadata(metadata);
      this.#rollback = null;
    }

    async #mutate(action) {
      this.#rollback = {
        metadata: this.#devices.exportFilesystemMetadata(),
        files: new Map(this.#devices.files),
      };
      try {
        const result = action();
        const metadata = this.#devices.takeFilesystemMetadata();
        if (metadata !== null) {
          // takeModifiedFiles returns copies; transferring them cannot detach
          // live filesystem data or the old buffers retained for rollback.
          const modified = this.#devices
            .takeModifiedFiles()
            .map(([path, bytes]) => ({ path, bytes }));
          const removed = this.#devices.takeRemovedFiles();
          const cancelled = new Promise((_, reject) => {
            this.#cancelCommit = reject;
          });
          await Promise.race([
            cancelled,
            this.#commit({ modified, removed, metadata }),
          ]);
          if (this.#closed) throw new Error("workspace service stopped");
        }
        this.#rollback = null;
        return result ?? { status: 0 };
      } catch (error) {
        this.#restore();
        throw error;
      } finally {
        this.#cancelCommit = null;
      }
    }

    async #execute(request, payload) {
      const { op, path } = request;
      switch (op) {
        case "workspace":
          return { status: 0, payload: this.#info };
        case "metadata": {
          const result = this.#devices.fsMetadata(path);
          check(result.status, `metadata ${path}`);
          return { status: result.status, payload: result.record };
        }
        case "list": {
          const result = this.#devices.fsListDirectory(path);
          check(result.status, `list ${path}`);
          return { status: result.status, payload: result.record };
        }
        case "read": {
          const entry = this.#entry(path);
          if (
            (request.inode !== undefined &&
              entry.inode !== BigInt(request.inode)) ||
            (request.mtimeNs !== undefined &&
              entry.mtimeNs !== BigInt(request.mtimeNs))
          ) {
            fail(
              C.STATUS_DENIED,
              "workspace file identity or version changed during read",
            );
          }
          const handle = check(
            this.#devices.fsOpen(path, C.FS_OPEN_READ),
            `open ${path}`,
          );
          try {
            check(
              this.#devices.fsSeek(handle, request.offset, 0),
              `seek ${path}`,
            );
            const result = this.#devices.fsRead(handle, request.capacity);
            check(result.status, `read ${path}`);
            return { status: result.status, payload: result.bytes };
          } finally {
            this.#devices.fsClose(handle);
          }
        }
        case "readable_copy": {
          if (typeof this.#format !== "function")
            fail(
              C.STATUS_DENIED,
              "readable copy formatting is not supported by this Host",
            );
          if (this.#entry(path).kind !== 1)
            fail(C.STATUS_IS_DIRECTORY, `${path} is a directory`);
          const target = this.#readablePath(path);
          this.#prepareWrite(target, false, false);
          // The formatter owns this copy and may transfer its buffer. It never
          // receives the saved filesystem buffer or any guest editor contents.
          const bytes = await this.#format(
            path,
            this.#devices.files.get(path).slice(),
          );
          if (this.#closed) throw new Error("workspace service stopped");
          if (!(bytes instanceof Uint8Array))
            fail(C.STATUS_INVALID, "readable formatter returned invalid bytes");
          if (bytes.length > LIMITS.maxFileBytes)
            fail(
              C.STATUS_LIMIT,
              "readable copy exceeds workspace file size limit",
            );
          return this.#mutate(() => {
            this.#replaceFile(target, bytes);
            return { status: 0, payload: encoder.encode(target) };
          });
        }
        case "begin_write":
          this.#upload = null;
          if (
            this.#prepareWrite(path, request.overwrite, request.ignoreIfExists)
          ) {
            return { status: 0, ignored: true };
          }
          this.#upload = {
            path,
            bytes: new Uint8Array(request.length),
            offset: 0,
            overwrite: request.overwrite,
            ignoreIfExists: request.ignoreIfExists,
          };
          return { status: 0 };
        case "write_chunk": {
          const upload = this.#upload;
          if (
            !upload ||
            request.offset !== upload.offset ||
            payload.length === 0 ||
            upload.offset + payload.length > upload.bytes.length
          ) {
            fail(
              C.STATUS_INVALID,
              "workspace write chunk is missing, noncontiguous or exceeds staged length",
            );
          }
          upload.bytes.set(payload, upload.offset);
          upload.offset += payload.length;
          return { status: 0 };
        }
        case "commit_write": {
          const upload = this.#upload;
          this.#upload = null;
          if (!upload || upload.offset !== upload.bytes.length) {
            fail(C.STATUS_INVALID, "workspace upload is incomplete");
          }
          return this.#mutate(() => {
            if (
              this.#prepareWrite(
                upload.path,
                upload.overwrite,
                upload.ignoreIfExists,
              )
            ) {
              return { status: 0, ignored: true };
            }
            this.#replaceFile(upload.path, upload.bytes);
          });
        }
        case "mkdir":
          return this.#mutate(() => this.#mkdir(path));
        case "rename":
          return this.#mutate(() => {
            this.#entry(path);
            if (path === request.to) return { status: 0, ignored: true };
            if (
              this.#destination(
                request.to,
                request.overwrite,
                request.ignoreIfExists,
              )
            ) {
              return { status: 0, ignored: true };
            }
            if (request.createParents)
              this.#mkdir(request.to.slice(0, request.to.lastIndexOf("/")));
            check(this.#devices.fsRename(path, request.to), `rename ${path}`);
          });
        case "remove":
          return this.#mutate(() => {
            const metadata = this.#devices.fsMetadata(path);
            if (
              metadata.status === C.STATUS_NOT_FOUND &&
              request.ignoreIfNotExists
            ) {
              return { status: 0, ignored: true };
            }
            check(metadata.status, `metadata ${path}`);
            const kind = new DataView(metadata.record.buffer).getUint32(
              0,
              true,
            );
            if (kind !== request.kind) {
              fail(
                kind === 2 ? C.STATUS_IS_DIRECTORY : C.STATUS_NOT_DIRECTORY,
                "workspace remove kind mismatch",
              );
            }
            if (path === "/home")
              fail(C.STATUS_DENIED, "cannot remove workspace root");
            if (kind === 2 && request.recursive) {
              const children = [...this.#devices.filesystem.entries.keys()]
                .filter((child) => child.startsWith(`${path}/`))
                .sort((a, b) => b.length - a.length);
              for (const child of children) {
                const childKind =
                  this.#devices.filesystem.entries.get(child).kind;
                check(
                  childKind === 1
                    ? this.#devices.fsRemove(child)
                    : this.#devices.fsRmdir(child),
                  `remove ${child}`,
                );
              }
            }
            check(
              kind === 1
                ? this.#devices.fsRemove(path)
                : this.#devices.fsRmdir(path),
              `remove ${path}`,
            );
          });
        case "copy":
          return this.#mutate(() => {
            if (this.#entry(path).kind !== 1)
              fail(C.STATUS_IS_DIRECTORY, `${path} is a directory`);
            if (
              this.#prepareWrite(
                request.to,
                request.overwrite,
                request.ignoreIfExists,
              )
            ) {
              return { status: 0, ignored: true };
            }
            this.#replaceFile(request.to, this.#devices.files.get(path));
          });
      }
    }
  }

  globalThis.PolkaVmWorkspace = {
    WorkspaceService,
    isWorkspaceFrame,
    decodeFrame,
    encodeFrame,
  };
})();
