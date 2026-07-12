# ToolOS

ToolOS is a Windows-first, ecosystem-wide local control plane for non-coders. It turns a goal into a transparent, evidence-backed sequence of actions across existing tools instead of rebuilding those tools.

## Implemented vertical slices

This repository currently implements the platform skeleton, trusted read-only discovery, and the first governed managed-machine provider slice:

- `toolos-daemon`: user-scoped Rust daemon with local socket IPC, SQLite state, governed action plans, durable execution journals, startup reconciliation, active-execution control, and evidence persistence.
- `toolos`: CLI for health, system scan, project inspection, ZIP inspection, WinGet exact resolution, governed planning/approval/execution/cancellation, recovery inspection, disabled cleanup planning, evidence, and event replay.
- `toolos-launcher`: starts the daemon when necessary and launches the desktop client.
- `toolos-system-adapter`: out-of-process JSON-RPC adapter for selected machine, project, and archive metadata.
- `toolos-winget-adapter`: out-of-process provider for exact WinGet resolution, disabled previews, bounded live provider output, and the draft pinned user-scope execution slice.
- `toolos-process`: safe process-control API with a Windows Job Object implementation for contained mutable execution and native parent/grandchild tests.
- `toolos-ui`: Tauri 2 + React guided dashboard using the same daemon API, including execution cancellation, persisted recovery reports, residual-state differences, and approval-only cleanup planning.
- Typed capability, evidence, adapter, action, approval, quota, lesson, policy, archive-report, package-resolution, execution, containment, recovery-report, and cleanup-plan schemas.
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

Process-tree containment is covered by native Windows tests.

## Crash-safe WinGet recovery

Every governed execution now has a durable SQLite journal with five phases:

- `PREPARED`: approval was consumed atomically with the `EXECUTING` plan transition, but process creation was not yet attempted.
- `SPAWN_INTENT`: the next operation is process creation; a crash from this point is ambiguous.
- `SPAWNED`: root process identity and containment method are durable.
- `PROVIDER_FINISHED`: bounded provider and containment evidence are durable before final plan persistence.
- `FINALIZED`: plan status, journal resolution, and lock decision are committed.

The daemon reconciles every unresolved journal before serving IPC requests. `PREPARED` is resolved as `RECOVERED_NO_PROCESS_STARTED` and releases the lock. A `PROVIDER_FINISHED` journal with confirmed zero active processes is finalized from its persisted evidence without replaying WinGet. `SPAWN_INTENT`, `SPAWNED`, malformed records, or unconfirmed containment become `UNKNOWN_REQUIRES_RECOVERY`; the WinGet lock is retained and further mutable package actions are blocked.

Recovery captures a bounded pre-state manifest and, when available, a read-only post-state manifest. It compares provider version, exact installed-state evidence, and a SHA-256/count fingerprint of the daemon `PATH` without storing PATH contents. These observations do not prove attribution or complete absence of file, registry, service, task, driver, process, or package residuals.

For unresolved reports, ToolOS can create an immutable cleanup plan containing inspection steps and an exact uninstall preview. Cleanup approval is separate and remains `APPROVED_EXECUTION_DISABLED`. There is intentionally no cleanup execution endpoint in Issue #9, and recovery never replays an interrupted installer.

Restart simulations reopen the same SQLite database after seeded crashes at `PREPARED`, `SPAWN_INTENT`, `SPAWNED`, and `PROVIDER_FINISHED`, then verify the resulting plan status, recovery report, lock decision, and mutation gate. Production merge remains blocked only by locale-stable installed-state and application-health verification in Issue #10.

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
# Inspect durable recovery state after an interrupted execution:
cargo run -p toolos-cli -- winget-recovery-list
cargo run -p toolos-cli -- winget-recovery-get --execution-id <EXECUTION_UUID>
# Cleanup planning and approval record intent only; execution remains disabled:
cargo run -p toolos-cli -- winget-recovery-cleanup-plan --execution-id <EXECUTION_UUID>
cargo run -p toolos-cli -- winget-recovery-cleanup-approve --cleanup-plan-id <CLEANUP_PLAN_UUID> --plan-hash <SHA256> --confirmation "<EXACT CLEANUP APPROVAL PHRASE>"
cargo run -p toolos-cli -- evidence
```

Run the Tauri desktop shell:

```powershell
pnpm --dir apps/toolos-ui tauri dev
```

Detailed Windows setup and verification are in [`docs/operations/development.md`](docs/operations/development.md).

## Architecture

The UI and CLI never perform unrestricted shell actions. They send typed JSON-RPC requests over a local socket to the daemon. The daemon owns policy, evidence, persistence, execution journaling, startup reconciliation, active-execution control, and adapter supervision. System and WinGet providers are separate processes using line-delimited JSON-RPC over stdio. The mutating WinGet adapter is additionally supervised by the Windows Job Object runner.

See:

- [`docs/architecture/ADR-0001-platform.md`](docs/architecture/ADR-0001-platform.md)
- [`docs/architecture/ADR-0002-safe-zip-inspection.md`](docs/architecture/ADR-0002-safe-zip-inspection.md)
- [`docs/provider-decisions/ADR-0003-winget-exact-preview.md`](docs/provider-decisions/ADR-0003-winget-exact-preview.md)
- [`docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md`](docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md)

## Safety boundary

The current draft permits read-only observations, governed metadata, and one narrow executable slice: an exact version-pinned, architecture-pinned, user-scope WinGet install after fresh revalidation and two separate short-lived confirmations. The mutable process tree is contained and cancellable on Windows. Interrupted executions are journaled and reconciled before further mutable actions; ambiguous states retain the lock and require explicit recovery review. Cleanup planning is approval-only and execution-disabled. Definitive application verification is not yet complete. ToolOS never auto-accepts agreements, requests elevation, adds installer overrides, bypasses hashes, forces execution, skips dependencies, or claims application health from an exit code. Extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, machine-scope installation, and arbitrary repository execution remain unimplemented.
