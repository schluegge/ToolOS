# ADR-0002: Metadata-first ZIP inspection

- Status: Accepted
- Date: 2026-07-11
- Capability: `archive.inspect`
- Blast radius: `READ_ONLY`

## Context

ToolOS must treat archives as untrusted input before extraction. A ZIP can contain traversal paths, absolute paths, names that collide on Windows, symbolic links, overlapping compressed ranges, extremely large declared output, or executable content. Direct extraction is outside the current read-only safety boundary.

## Decision

ToolOS implements a metadata-only ZIP inspector in the isolated `toolos-archive` crate and exposes it through the existing out-of-process system adapter.

The inspector:

- opens one explicitly selected ZIP file;
- reads the archive central directory and entry metadata;
- validates paths with the ZIP library's `enclosed_name` API;
- applies additional Windows path checks;
- detects case-insensitive and trailing-dot/space path collisions;
- detects symbolic links and encrypted entries;
- checks overlapping compressed ranges;
- calculates declared compressed and uncompressed totals;
- flags high expansion ratios and declared size limits;
- flags executable, installer, shortcut, registry, and script extensions;
- records structured evidence through the daemon;
- never extracts, executes, or writes archive entries.

## Source-copy evidence ledger

Implementation uses only APIs verified in the official `zip-rs/zip2` source at commit `c2999bcad2284d7a5ffafc1797f0fbfb5e127964`, whose package manifest identifies version 8.3.1:

| API | Verified behavior used by ToolOS |
|---|---|
| `ZipArchive::new(reader)` | Parses a ZIP archive from a `Read + Seek` reader. |
| `ZipArchive::len()` | Returns the number of archive entries. |
| `ZipArchive::by_index(index)` | Reads entry metadata by index. |
| `ZipArchive::has_overlapping_files()` | Reports compressed data ranges shared by multiple entries. |
| `ZipFile::name()` | Returns the decoded entry name and warns that it may be unsafe for extraction. |
| `ZipFile::enclosed_name()` | Returns a path only when it is enclosed and not absolute or escaping. |
| `ZipFile::is_dir()` / `is_symlink()` | Classifies directories and symbolic-link entries. |
| `ZipFile::encrypted()` | Reports encryption metadata without requesting credentials. |
| `ZipFile::compressed_size()` / `size()` | Supplies declared size metadata. |
| `ZipFile::compression()` | Supplies the declared compression method. |
| `ZipFile::unix_mode()` / `crc32()` | Supplies additional entry metadata. |
| `ZipWriter` / `SimpleFileOptions` | Creates bounded in-test ZIP fixtures without checking fixture binaries into the repository. |

Workspace configuration pins `zip = "=8.3.1"`, disables default features, and enables only `deflate`. The workspace Rust minimum is 1.88 because the verified dependency declares Rust 1.88.

## Decisions and limits

The report decision has three states:

- `ACCEPT_STRUCTURE`: no current structural finding was detected.
- `REVIEW`: extraction requires a human or later guarded workflow decision.
- `BLOCK`: structural metadata indicates extraction should not proceed.

`ACCEPT_STRUCTURE` is deliberately not named `SAFE`. It is not proof that entry contents are benign, correctly licensed, secret-free, non-malicious, or semantically valid.

Current numeric limits are policy defaults for the structural scanner, not universal archive truths:

- review above 10,000 entries;
- review above 4 GiB total declared uncompressed size;
- block at or above 20 GiB total declared uncompressed size;
- block an individual entry at or above 8 GiB;
- review an expansion ratio at or above 200:1 when output is at least 10 MiB;
- block an expansion ratio at or above 1,000:1 when output is at least 100 MiB;
- include at most 500 entry records in the response while evaluating every entry.

Changing these values requires tests and a policy decision record. They must not silently become extraction guarantees.

## Consequences

Positive:

- malicious path fixtures can be rejected before any write action;
- archive inspection is available in UI, CLI, daemon, adapter, evidence, and schema surfaces;
- future extraction can consume the same report without reimplementing ZIP parsing;
- archive parsing remains isolated from general machine inventory.

Negative:

- no malware, secret, license, CRC-content, or semantic scan is performed;
- encrypted content cannot be evaluated;
- a future extractor still requires destination identity, disk checks, locks, approvals, symlink containment, cancellation, and rollback.
