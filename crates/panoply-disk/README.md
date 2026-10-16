# panoply-disk

The disk map engine: a parallel scanner that counts what files really cost on disk, and a compact tree to
draw, drill into and clean up from.

- **Allocated size**, never apparent size. Sparse files and compressed files cost what they occupy.
- **Hard links** are counted once (by device and file ID). **APFS clones** are counted once through the
  clone ID and private size the OS reports.
- **Other volumes and network mounts** are skipped unless asked for.
- **Folders that cannot be read** are listed with a reason, never a crash. On macOS a refusal that only
  Full Disk Access can fix is told apart from an ordinary permission error.
- One backend per platform behind one trait, and a portable fallback: `getattrlistbulk` on macOS,
  `getdents64` plus `statx` on Linux, `GetFileInformationByHandleEx` (the `NtQueryDirectoryFile` call,
  one syscall per folder) on Windows.

The tree holds 32 bytes per entry plus the names in one shared byte buffer: no `String` per node.

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
