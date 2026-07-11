# ToolOS

ToolOS is a Windows-first, ecosystem-wide local control plane for non-coders. It turns a goal into a transparent, evidence-backed sequence of actions across existing tools instead of rebuilding those tools.

## Implemented vertical slices

This repository currently implements the platform skeleton, trusted read-only discovery, and the first managed-machine governance slices:

- `toolos-daemon`: user-scoped Rust daemon with local socket IPC, SQLite evidence, and durable action plans.
- `toolos`: CLI for health, system scan, project inspection, ZIP inspection, WinGet resolution, planning, approval, rejection, evidence, and event replay.
- `toolos-launcher`: starts the daemon when necessary and launches the desktop client.
- `toolos-system-adapter`: out-of-process JSON-RPC adapter for selected machine, project, and archive metadata.
- `toolos-winget-adapter`: out-of-process provider for exact WinGet package resolution and disabled command previews.
- `toolos-ui`: Tauri 2 + React guided dashboard using the same daemon API.
- Typed capability, evidence, adapter, action-plan, approval, rollback, archive-report, and package-resolution schemas.
- Correlated event, evidence, and compare-and-swap action-state persistence.

This is not the complete twelve-phase product. The implemented scope includes Milestone A, selected Milestone B capabilities, and read-only/planning portions of Milestone C.

## Read-only inspections

The system scan detects the host OS/architecture and whether representative executables are resolvable on `PATH`, including Git, Rust, Node, Python, PowerShell, WinGet, WSL, Docker, Podman, Ollama, and several coding-agent CLIs. It does not execute those tools and does not read browser profiles, secrets, AppData content, or repository install hooks.

The project inspector checks the selected directory identity and known stack marker files such as `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, solution/project files, and common agent instruction files.

The ZIP inspector reads only selected archive metadata. It checks for unsafe or ambiguous extraction paths, Windows path collisions, symbolic-link entries, encrypted entries, executable/script extensions, overlapping compressed ranges, declared size limits, and high expansion ratios. It does not extract entries, decompress their contents, execute anything, or claim that accepted structure is malware-free or trustworthy.

## Exact WinGet resolution and durable approval

The WinGet provider accepts one package ID and one source, plus optional version, scope, and architecture filters. It runs a non-interactive exact `winget show` query, captures bounded process evidence, and creates exact install and uninstall command previews.

A fresh successful resolution can be converted into an immutable action plan. Each plan:

- expires after fifteen minutes;
- stores the exact resolution, command, SHA-256 command hash, blast radius, and rollback gaps;
- requires an exact generated confirmation phrase;
- requires three explicit risk acknowledgements;
- uses an atomic state transition to prevent repeated approval;
- remains technically non-executable even after approval.

ToolOS does not accept package/source agreements, bypass hashes, skip dependencies, request elevation, install software, or uninstall software in this slice. Actual write execution remains blocked until Windows process-tree cancellation and recovery coverage are implemented and proven.

## Development

Prerequisites are Rust stable, Node.js, pnpm, and platform requirements for Tauri 2. WinGet resolution additionally requires Windows Package Manager on the target Windows machine.

```powershell
corepack enable
pnpm install
cargo build --workspace
cargo test --workspace
pnpm --dir apps/toolos-ui build
cargo run -p toolos-daemon
```

In another terminal:

```powershell
cargo run -p toolos-cli -- doctor
cargo run -p toolos-cli -- scan
cargo run -p toolos-cli -- inspect .
cargo run -p toolos-cli -- archive C:\path\archive.zip
cargo run -p toolos-cli -- winget-resolve --id Git.Git --source winget --scope user --architecture x64
cargo run -p toolos-cli -- winget-plan-install --id Git.Git --source winget --scope user --architecture x64
cargo run -p toolos-cli -- actions
cargo run -p toolos-cli -- evidence
```

The plan response contains the exact phrase required by `action-approve`. Approval still does not run WinGet.

Run the Tauri desktop shell:

```powershell
pnpm --dir apps/toolos-ui tauri dev
```

Detailed Windows setup and verification are in [`docs/operations/development.md`](docs/operations/development.md).

## Architecture

The UI and CLI never perform unrestricted shell actions. They send typed JSON-RPC requests over a local socket to the daemon. The daemon owns policy, evidence, persistence, adapter supervision, and durable action-plan transitions. System and WinGet providers are separate processes using line-delimited JSON-RPC over stdio.

See:

- [`docs/architecture/ADR-0001-platform.md`](docs/architecture/ADR-0001-platform.md)
- [`docs/architecture/ADR-0002-safe-zip-inspection.md`](docs/architecture/ADR-0002-safe-zip-inspection.md)
- [`docs/provider-decisions/ADR-0003-winget-exact-preview.md`](docs/provider-decisions/ADR-0003-winget-exact-preview.md)
- [`docs/architecture/ADR-0004-durable-action-approval.md`](docs/architecture/ADR-0004-durable-action-approval.md)

## Safety boundary

The current release remains read-only with durable planning and approval records. No installation, extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, or arbitrary repository execution is implemented.
