# ADR-0003: WinGet exact package resolution before execution

- Status: Accepted
- Date: 2026-07-11
- Milestone: C — Managed machine
- Implemented capabilities: `package.resolve.winget`, `package.preview.install`, `package.preview.uninstall`
- Execution status: preview only

## Context

ToolOS Phase 2 requires exact package identity before installation. Package display names and substring search are insufficient because multiple packages or sources can match. At the same time, enabling package installation before simulation, approval, residual-state tracking, and rollback boundaries exist would violate the current read-only safety boundary.

## Decision

ToolOS adopts WinGet as the first native Windows package provider through a separate out-of-process adapter.

The first provider slice:

1. validates a package ID, source, optional version, optional scope, and optional architecture;
2. passes arguments directly to `winget` without a shell;
3. runs only an exact, non-interactive `winget show` identity probe;
4. captures executable, argument array, duration, exit code, bounded stdout, bounded stderr, and timeout state;
5. generates exact install and uninstall command previews;
6. keeps `execution_enabled` set to `false` for every preview;
7. does not add agreement, hash-bypass, force, purge, or dependency-skip flags;
8. records the report as structured ToolOS evidence.

## Official API and CLI evidence ledger

The implementation uses exact commands and option names copied from the official Microsoft WinGet documentation:

| Surface | Official syntax used |
|---|---|
| Version health probe | `winget --version` |
| Exact identity probe | `winget show --id <ID> --exact --source <SOURCE> --disable-interactivity` |
| Optional show filters | `--version`, `--scope`, `--architecture` |
| Install preview | `winget install --id <ID> --exact --source <SOURCE> --disable-interactivity` |
| Optional install filters | `--version`, `--scope`, `--architecture` |
| Uninstall preview | `winget uninstall --id <ID> --exact --source <SOURCE> --disable-interactivity` |
| Optional uninstall filters | `--version`, `--scope` |

Official documentation additionally states that `--id` combined with `--exact` is the preferred way to limit selection to one package, and that specifying `--source` further disambiguates duplicate entries across configured sources.

The adapter deliberately does not use these documented write-affecting or risk-weakening options in its previews:

- `--accept-package-agreements`;
- `--accept-source-agreements`;
- `--force`;
- `--ignore-security-hash`;
- `--ignore-local-archive-malware-scan`;
- `--skip-dependencies`;
- `--override`;
- `--custom`;
- `--purge`.

## Output handling

Current WinGet command output is human-oriented and can be localized. ToolOS therefore does not parse table columns into asserted metadata in this slice. It preserves bounded raw provider output and derives only process-level facts:

- command launched or unavailable;
- exit code;
- timeout status;
- duration;
- exact selector and arguments used.

A successful `show` command produces `RESOLVED_EXACT`. A non-zero result or timeout produces `BLOCKED`. Failure to launch WinGet produces `UNAVAILABLE`.

## Safety and limitations

- The identity probe may read configured sources over the network.
- It does not accept source agreements automatically; a source that requires acceptance may remain blocked.
- A successful identity probe is time-scoped provider evidence, not proof that a future installer is healthy or benign.
- Install and uninstall previews are copyable but cannot be executed through ToolOS.
- Uninstall residual-state coverage is not yet implemented.
- Machine-scope previews identify likely machine write blast radius but do not request elevation.

## Next required capability

Write execution remains blocked until ToolOS has a durable action state machine with simulation, exact approval, workspace/environment locks where applicable, process-tree cancellation, installer evidence, uninstall/residual manifests, and explicit recovery or irreversibility records.
