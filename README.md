# ToolOS

ToolOS is a Windows-first, ecosystem-wide local control plane for non-coders. It turns a goal into a transparent, evidence-backed sequence of actions across existing tools instead of rebuilding those tools.

## Implemented vertical slices

This repository currently implements the platform skeleton, trusted read-only discovery, and the first governed managed-machine provider slice:

- `toolos-daemon`: user-scoped Rust daemon with local socket IPC, SQLite state, governed action plans, active-execution control, and evidence persistence.
- `toolos`: CLI for health, system scan, project inspection, ZIP inspection, WinGet exact resolution, governed planning/approval/execution/cancellation, evidence, and event replay.
- `toolos-launcher`: starts the daemon when necessary and launches the desktop client.
- `toolos-system-adapter`: out-of-process JSON-RPC adapter for selected machine, project, and archive metadata.
- `toolos-winget-adapter`: out-of-process provider for exact WinGet resolution, disabled previews, bounded live provider output, and the draft pinned user-scope execution slice.
- `toolos-process`: safe process-control API with a Windows Job Object implementation for contained mutable execution and native parent/grandchild tests.
- `toolos-ui`: Tauri 2 + React guided dashboard using the same daemon API, including execution cancellation and recovery-state rendering.
- Typed capability, evidence, adapter, action, approval, quota, lesson, policy, archive-report, package-resolution, execution, and containment schemas.
- Correlated event and evidence persistence.

This is not the complete twelve-phase product. The implemented scope includes Milestone A, selected Milestone B capabilities, and the first governed Milestone C provider slice.

## Read-only inspections

The system scan detects the host OS/architecture and whether representative executables are resolvable on `PATH`, including Git, Rust, Node, Python, PowerShell, WinGet, WSL, Docker, Podman, Ollama, and several coding-agent CLIs. It does not execute those tools and does not read browser profiles, secrets, AppData content, or repository install hooks.

The project inspector checks the selected directory identity and known stack marker files such as `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, solution/project files, and common agent instruction files.

The ZIP inspector reads only selected archive metadata. It checks for unsafe or ambiguous extraction paths, Windows path collisions, symbolic-link entries, encrypted entries, executable/script extensions, overlapping compressed ranges, declared size limits, and high expansion ratios. It does not extract entries, decompress their contents, execute anything, or claim that accepted structure is malware-free or trustworthy.

## Exact and contained WinGet execution

The WinGet provider accepts one package ID and one source, plus optional version, scope, and architecture filters. It runs a non-interactive exact `winget show` query, captures bounded process evidence, and creates exact install and uninstall command previews.

Preview commands remain intentionally non-executable artifacts. The draft execution path can invoke only an exact version- and architecture-pinned user-scope install after fresh revalidation and two separate short-lived confirmations. It never auto-accepts agreements, bypasses hashes, skips dependencies, requests elevation, enables uninstall, or accepts arbitrary installer arguments.

The mutating adapter is assigned to a Windows Job Object during `CreateProcessW`, before it can launch WinGet or descendants. Timeout and explicit cancellation terminate the Job Object. ToolOS accepts a contained terminal state only after Job Object accounting reports zero active processes. If termination cannot be confirmed, the plan becomes `UNKNOWN_REQUIRES_RECOVERY`, the post-state query is skipped, and the WinGet lock remains held.

Provider stdout/stderr is retained with independent bounds. During execution, observed WinGet output is also mirrored to the adapter's stderr transport so output emitted before timeout or cancellation is not lost when the adapter cannot return final JSON.

Process-tree containment is covered by native Windows tests. Production merge remains blocked by crash/restart residual-state reconciliation in Issue #9 and locale-stable installed-state/application-health verification in Issue #10.

## Development

Prerequisites are Rust stable, Node.js, pnpm, and platform requirements for Tauri 2. WinGet resolution and execution additionally require Windows Package Manager on the target Windows machine.

```powershell
corepack enable
pnpm install --frozen-lockfile
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
cargo run -p toolos-cli -- winget-install-plan --id Git.Git --source winget --version <VERSION> --scope user --architecture x64
# Approve only the exact plan/hash/phrase returned by the previous command:
cargo run -p toolos-cli -- winget-install-approve --plan-id <PLAN_UUID> --plan-hash <SHA256> --confirmation "<EXACT APPROVAL PHRASE>"
# Execution prints its execution ID before blocking. Save it for cancellation from another terminal:
cargo run -p toolos-cli -- winget-install-execute --plan-id <PLAN_UUID> --approval-id <APPROVAL_UUID> --confirmation "<EXACT EXECUTION PHRASE>"
# Optional cancellation from another terminal:
cargo run -p toolos-cli -- winget-install-cancel --execution-id <EXECUTION_UUID>
cargo run -p toolos-cli -- evidence
```

Run the Tauri desktop shell:

```powershell
pnpm --dir apps/toolos-ui tauri dev
```

Detailed Windows setup and verification are in [`docs/operations/development.md`](docs/operations/development.md).

## Architecture

The UI and CLI never perform unrestricted shell actions. They send typed JSON-RPC requests over a local socket to the daemon. The daemon owns policy, evidence, persistence, active-execution control, and adapter supervision. System and WinGet providers are separate processes using line-delimited JSON-RPC over stdio. The mutating WinGet adapter is additionally supervised by the Windows Job Object runner.

See:

- [`docs/architecture/ADR-0001-platform.md`](docs/architecture/ADR-0001-platform.md)
- [`docs/architecture/ADR-0002-safe-zip-inspection.md`](docs/architecture/ADR-0002-safe-zip-inspection.md)
- [`docs/provider-decisions/ADR-0003-winget-exact-preview.md`](docs/provider-decisions/ADR-0003-winget-exact-preview.md)
- [`docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md`](docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md)

## Safety boundary

The current draft permits read-only observations, governed metadata, and one narrow executable slice: an exact version-pinned, architecture-pinned, user-scope WinGet install after fresh revalidation and two separate short-lived confirmations. The mutable process tree is contained and cancellable on Windows, but database recovery after abrupt daemon loss and definitive application verification are not yet complete. ToolOS never auto-accepts agreements, requests elevation, adds installer overrides, bypasses hashes, forces execution, skips dependencies, or claims application health from an exit code. Extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, machine-scope installation, and arbitrary repository execution remain unimplemented.
