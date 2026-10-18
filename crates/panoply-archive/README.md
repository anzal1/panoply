# panoply-archive

The archive engine behind `archive.create` and `archive.extract`. Spec: `docs/specs/docs-find-files-share-capture.md`,
section Archives.

- Pack: zip (deflate, already compressed types stored), tar.gz, tar.zst. Each file is read once in 256 KiB chunks.
- Unpack: zip, tar, tar.gz, tar.zst, gz and zst, into a new folder. Memory is one buffer.
- Safe on archives from strangers. Entries that start at the root of the disk or use `..` are skipped and listed.
  Links are only made when their target stays inside the folder (relative, `..` only at the start and never more
  than the link's own depth), and nothing is ever written through a link. Existing files are never overwritten: a
  taken name gets ` (2)`. A zip bomb stops the job: a ratio above 1000 to 1 (judged after 64 MiB), more than 50 GiB,
  or more than a million entries. A ZIP is judged before anything is written, from its directory, and again while
  unpacking, in case the directory lies.
- A damaged archive keeps what was readable and says so.
- Cancel is polled between chunks.

Dependencies: `zip` (MIT, deflate only), `tar` (MIT or Apache-2.0, no xattr), `flate2` (MIT or Apache-2.0) and `zstd`
(BSD-3-Clause, with zstd-sys). 7z is not read yet: there is no code for it, and the detector says so.
