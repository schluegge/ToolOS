# ToolOS

ToolOS is a Windows-first, ecosystem-wide local control plane for non-coders. It turns a goal into a transparent, evidence-backed sequence of actions across existing tools instead of rebuilding those tools.

## Implemented vertical slices

This repository currently implements the platform skeleton, trusted read-only discovery, and the first managed-machine provider slice:

- `toolos-daemon`: user-scoped Rust daemon with local socket IPC and SQLite state.
- `toolos`: CLI for health, system scan, project inspection, ZIP inspection, WinGet exact resolution, evidence, and event replay.
- `toolos-launcher`: starts the daemon when necessary and launches the desktop client.
- `toolos-system-adapter`: out-of-process JSON-RPC adapter for selected machine, project, and archive metadata.
- `toolos-winget-adapter`: out-of-process provider for exact WinGet package resolution and disabled command previews.
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

The preview commands are intentionally disabled. ToolOS does not accept package/source agreements, bypass hashes, skip dependencies, request elevation, install software, or uninstall software in this slice.

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

## Safety boundary

The current release permits read-only observations plus local plan, approval-receipt, and lock metadata. No installation, extraction, deletion, agreement acceptance, elevation, billing, credential extraction, browser stealth, CAPTCHA bypass, or arbitrary repository execution is implemented.
