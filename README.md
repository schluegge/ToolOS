# ToolOS

ToolOS is a Windows-first, ecosystem-wide local control plane for non-coders. It turns a goal into a transparent, evidence-backed sequence of actions across existing tools instead of rebuilding those tools.

## Implemented vertical slices

This repository currently implements the platform skeleton, trusted read-only discovery, and the first managed-machine provider slice:

- `toolos-daemon`: user-scoped Rust daemon with local socket IPC and SQLite state.
- `toolos`: CLI for health, system scan, project inspection, ZIP inspection, WinGet exact resolution, evidence, and event replay.
- `toolos-launcher`: starts the daemon when necessary and launches the desktop client.
- `toolos-system-adapter`: out-of-process JSON-RPC adapter for selected machine, project, and archive metadata.
- `toolos-winget-adapter`: out-of-process provider for exact WinGet resolution, disabled previews, and the draft pinned user-scope execution slice.
- `toolos-ui`: Tauri 2 + React guided dashboard using the same daemon API.
- Typed capability, evidence, adapter, action, approval, quota, lesson, policy, archive-report, and package-resolution schemas.
- Correlated event and evidence persistence.

This is not the complete twelve-phase product. The implemented scope includes Milestone A, selected Milestone B capabilities, and the first read-only Milestone C provider slice.

## Read-only inspections

The system scan detects the host OS/architecture and whether representative executables are resolvable on `PATH`, including Git, Rust, Node, Python, PowerShell, WinGet, WSL, Docker, Podman, Ollama, and several coding-agent CLIs. It does not execute those tools and does not read browser profiles, secrets, AppData content, or repository install hooks.

The project inspector checks the selected directory identity and known stack marker files such as `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, solution/project files, and common agent instruction files.

The ZIP inspector reads only selected archive metadata. It checks for unsafe or ambiguous extraction paths, Windows path collisions, symbolic-link entries, encrypted entries, executable/script extensions, overlapping compressed ranges, declared size limits, and high expansion ratios. It does not extract entries, decompress their contents, execute anything, or claim that accepted structure is malware-free or trustworthy.

## Exact WinGet package resolution

The WinGet provider accepts one package ID and one source, plus optional version, scope, and architecture filters. It runs a non-interactive exact `winget show` query, captures bounded process evidence, and creates exact install and uninstall command previews.

Preview commands remain intentionally non-executable artifacts. The draft execution path can invoke only an exact version- and architecture-pinned user-scope install after fresh revalidation and two separate short-lived confirmations; it never auto-accepts agreements, bypasses hashes, skips dependencies, requests elevation, or enables uninstall. Production merge remains blocked by Issues #8 and #9.

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
cargo run -p toolos-cli -- winget-installed --id Git.Git --source winget --scope user
cargo run -p toolos-cli -- winget-install-plan --id Git.Git --source winget --scope user --architecture x64
# Then approve only the exact plan/hash/phrase returned by the previous command:
cargo run -p toolos-cli -- winget-install-approve --plan-id <UUID> --plan-hash <SHA256> --confirmation "<EXACT PHRASE>"
# Execution is restricted to explicit user scope plus pinned version and architecture:
cargo run -p toolos-cli -- winget-install-execute --plan-id <UUID> --approval-id <UUID> --confirmation "<EXACT EXECUTION PHRASE>"
# While an execution is active, request cancellation of the entire contained process tree:
cargo run -p toolos-cli -- winget-install-cancel --plan-id <UUID>
cargo run -p toolos-cli -- evidence
```

Run the Tauri desktop shell:

```powershell
pnpm --dir apps/toolos-ui tauri dev
```

Detailed Windows setup and verification are in [`docs/operations/development.md`](docs/operations/development.md).

## Architecture

The UI and CLI never perform unrestricted shell actions. They send typed JSON-RPC requests over a local socket to the daemon. The daemon owns policy, evidence, persistence, and adapter supervision. System and WinGet providers are separate processes using line-delimited JSON-RPC over stdio.

See:

- [`docs/architecture/ADR-0001-platform.md`](docs/architecture/ADR-0001-platform.md)
- [`docs/architecture/ADR-0002-safe-zip-inspection.md`](docs/architecture/ADR-0002-safe-zip-inspection.md)
- [`docs/provider-decisions/ADR-0003-winget-exact-preview.md`](docs/provider-decisions/ADR-0003-winget-exact-preview.md)
- [`docs/provider-decisions/ADR-0005-windows-job-containment.md`](docs/provider-decisions/ADR-0005-windows-job-containment.md)

## Safety boundary

The current draft permits read-only observations, governed metadata, and one narrow executable slice: an exact version-pinned, architecture-pinned, user-scope WinGet install after fresh revalidation and two separate short-lived confirmations. On Windows, the adapter and all descendants are assigned atomically to a kill-on-close Job Object; timeout and cancellation are terminal only when ToolOS confirms zero active job processes. Unconfirmed containment becomes `UNKNOWN_REQUIRES_RECOVERY` and retains the package-manager lock. ToolOS never auto-accepts agreements, requests elevation, adds installer overrides, bypasses hashes, forces execution, skips dependencies, or claims application health from an exit code. Extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, machine-scope installation, and arbitrary repository execution remain unimplemented.
