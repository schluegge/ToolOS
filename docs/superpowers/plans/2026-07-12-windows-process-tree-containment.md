# Windows Process-Tree Containment Run Plan

> **Agent instruction:** Execute task-by-task with `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Do not start backlog work.

**Goal:** Complete ToolOS Issue #8 by proving that the mutating WinGet adapter and every descendant process are contained, cancellable, and either confirmed terminated or classified `UNKNOWN_REQUIRES_RECOVERY`.

**Branch/PR:** `build/winget-user-execution`, Draft PR #7.

**Architecture:** Add one isolated Rust crate with a safe public API and one Windows-only Win32 implementation. The daemon launches the mutating adapter directly into a Windows Job Object during `CreateProcessW`; WinGet descendants inherit that job. Read-only adapter calls keep the existing Tokio runner.

## Non-negotiable constraints

- PR #7 remains Draft and is not merged in this run.
- Work only on Issue #8. LocalStack, Awesome Stacks, Issues #9/#10, and unrelated infrastructure remain out of scope.
- Keep every existing WinGet policy restriction.
- All Win32 `unsafe` code lives only in `crates/toolos-process/src/windows.rs`; all public APIs are safe Rust.
- Adapter stdout remains JSON-RPC only. Live provider output may be mirrored only to adapter stderr.
- Containment is proven only when Job Object accounting reports `ActiveProcesses == 0`.
- Unconfirmed containment means `UNKNOWN_REQUIRES_RECOVERY`; skip post-state queries and retain the WinGet lock.
- Missing official API evidence means `BLOCKED_SOURCE_MISSING`, not guessed FFI.

## Files

Create:

- `crates/toolos-process/Cargo.toml`
- `crates/toolos-process/src/lib.rs`
- `crates/toolos-process/src/windows.rs`
- `crates/toolos-process/src/bin/toolos-process-fixture.rs`
- `crates/toolos-process/tests/windows_job_object.rs`

Modify:

- `Cargo.toml`, `Cargo.lock`
- `apps/toolos-daemon/Cargo.toml`, `apps/toolos-daemon/src/main.rs`
- `adapters/toolos-winget-adapter/src/main.rs`
- `crates/toolos-winget/src/lib.rs`, `crates/toolos-winget/src/execution.rs`
- `crates/toolos-storage/src/lib.rs`
- `apps/toolos-cli/src/main.rs`
- `apps/toolos-ui/src/api.ts`, `InstallPlanPanel.tsx`, `install-plan.css`
- `schemas/winget-install-execution.schema.json`
- `.github/workflows/verify.yml`
- `README.md`, `docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md`

---

## Task 1 — Baseline and source ledger

- [ ] Create an isolated worktree from `origin/build/winget-user-execution`.
- [ ] Record `git rev-parse HEAD`, `git status --short`, and `git diff --check`.
- [ ] Run the unchanged baseline:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
corepack enable
pnpm install --frozen-lockfile
pnpm --dir apps/toolos-ui check
pnpm --dir apps/toolos-ui build
cargo check --manifest-path apps/toolos-ui/src-tauri/Cargo.toml
```

Expected: every command exits `0`; otherwise stop as `BASELINE_BLOCKER`.

- [ ] Copy exact `windows-sys` signatures/constants/features for Job Objects, `STARTUPINFOEXW`, process attributes, pipes, process creation, waiting, termination, accounting, and handle cleanup into the run notes.
- [ ] Record the existing TypeScript drift: `api.ts` omits `APPROVED_AWAITING_EXECUTION` and still types an armed receipt as disabled. Repair it later; do not ignore it.

---

## Task 2 — Safe containment crate

Add workspace member `crates/toolos-process` and `windows-sys = "0.61"`.

Public contract:

```rust
pub enum ProcessStopReason { Exited, TimedOut, Cancelled, DaemonShutdown, ContainmentFailed }

pub struct ProcessContainmentEvidence {
    pub method: String,
    pub root_pid: Option<u32>,
    pub stop_reason: ProcessStopReason,
    pub active_processes_after_cleanup: Option<u32>,
    pub descendants_terminated: Option<bool>,
    pub containment_confirmed: bool,
    pub detail: String,
}

pub struct ContainedCommandSpec {
    pub executable: OsString,
    pub args: Vec<OsString>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

pub fn spawn_contained(spec: ContainedCommandSpec) -> Result<ContainedProcess, ContainmentError>;
```

`ContainedProcess` exposes `execution_id()`, `root_pid()`, `control()`, and async `wait()`. `ContainedProcessControl::cancel(reason)` is safe and idempotent.

- [ ] Write non-Windows tests first: mutating execution must return `UnsupportedPlatform`.
- [ ] Implement `src/lib.rs` and target-specific dependencies.
- [ ] Do not inherit workspace `unsafe_code = "forbid"` in this one crate; use `unsafe_op_in_unsafe_fn = "deny"`. Every other crate keeps the workspace lint.
- [ ] Run:

```powershell
cargo test -p toolos-process
cargo clippy -p toolos-process --all-targets -- -D warnings
```

- [ ] Commit: `feat: define fail-closed process containment API`.

---

## Task 3 — Windows Job Object implementation

Implement in `crates/toolos-process/src/windows.rs`:

- `CreateJobObjectW`.
- `SetInformationJobObject` with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
- Inheritable stdin/stdout/stderr pipes; parent handles non-inheritable.
- `STARTUPINFOEXW` attribute list containing `PROC_THREAD_ATTRIBUTE_JOB_LIST` and `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.
- `CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT`, Unicode environment, handle inheritance enabled, and no breakaway flag.
- No fallback to post-start `AssignProcessToJobObject` for the mutating path.
- RAII wrappers for every HANDLE and attribute list.
- Independent bounded stdout/stderr readers.
- Timeout/cancel: `TerminateJobObject`, wait root, then poll accounting until zero active processes or bounded confirmation timeout.
- Last Job Object handle close remains the daemon-crash backstop.

Tests before implementation:

- Windows argument quoting: empty, spaces, quotes, trailing backslashes.
- Output truncation independently at 64 KiB per stream.
- Handle/error paths do not leak or claim containment.

Run and commit:

```powershell
cargo check -p toolos-process --all-targets
cargo clippy -p toolos-process --all-targets -- -D warnings
git commit -am "feat: launch process trees inside Windows Job Objects"
```

---

## Task 4 — Real Windows descendant tests

Fixture modes:

```text
grandchild --marker <path> --delay-ms <n>
parent --fixture <exe> --marker <path> --delay-ms <n>
host-exit --fixture <exe> --marker <path> --delay-ms <n>
output --stdout-bytes <n> --stderr-bytes <n>
```

Required native tests:

- [ ] Normal exit captures both streams and reports zero active processes.
- [ ] Timeout kills parent and grandchild; marker remains absent after the original delay.
- [ ] Explicit cancellation kills parent and grandchild; marker remains absent.
- [ ] Abrupt host exit closes the last Job Object handle; grandchild marker remains absent.
- [ ] Oversized output is bounded and marked truncated.

Repeat the suite five times:

```powershell
1..5 | ForEach-Object {
  cargo test -p toolos-process --test windows_job_object -- --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
```

Any intermittent surviving marker is a product failure, not a flaky test to suppress.

Commit: `test: prove Windows descendant termination`.

---

## Task 5 — Daemon integration and cancellation

Add to `AppState` an active-execution registry keyed by `execution_id`, storing `plan_id` and `ContainedProcessControl`.

- [ ] Keep `invoke_adapter` for read-only calls.
- [ ] Add `invoke_mutating_adapter_contained`; use it only for `winget.install.execute`.
- [ ] Persist `execution_id`, containment method, and root PID before awaiting completion.
- [ ] Add RPC `winget.install.cancel` with `{ execution_id }`; reject unknown/inactive IDs.
- [ ] Remove registry entries through one cleanup path for all outcomes.
- [ ] Run post-install `winget.installed` only after confirmed zero active processes.
- [ ] Extend storage finish input with `release_resource_lock: bool`.
- [ ] Set it `false` only for `UNKNOWN_REQUIRES_RECOVERY`; atomically update plan/report while retaining the lock.

Pure status tests must prove:

```text
confirmed exit 0 -> EXECUTION_SUCCEEDED_UNVERIFIED
confirmed nonzero -> EXECUTION_FAILED
confirmed timeout -> EXECUTION_TIMED_OUT
confirmed cancel -> EXECUTION_CANCELLED
unconfirmed containment -> UNKNOWN_REQUIRES_RECOVERY
```

Run:

```powershell
cargo test -p toolos-storage -p toolos-daemon
cargo clippy -p toolos-storage -p toolos-daemon --all-targets -- -D warnings
```

Commit: `feat: contain and cancel mutating WinGet execution`.

---

## Task 6 — Preserve provider output on forced kill

In the WinGet adapter, add `run_command_with_live_tee` only for execution.

- [ ] Read child stdout/stderr concurrently.
- [ ] Retain each stream up to 64 KiB for final `ProcessEvidence`.
- [ ] Mirror chunks to adapter stderr with `[winget stdout]` / `[winget stderr]` prefixes.
- [ ] Never write provider bytes to adapter stdout.
- [ ] Add a hanging fixture: emit marker, then wait. Cancel the contained adapter and prove the daemon-captured stderr still contains the marker.

Run and commit:

```powershell
cargo test -p toolos-winget-adapter
cargo clippy -p toolos-winget-adapter --all-targets -- -D warnings
git commit -am "feat: preserve WinGet output across forced termination"
```

---

## Task 7 — Domain, schema, CLI, and UI parity

Add execution statuses:

```text
PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED
PROVIDER_FAILED
TIMED_OUT_CONTAINED
CANCELLED_CONTAINED
UNKNOWN_REQUIRES_RECOVERY
```

Add plan statuses:

```text
EXECUTION_TIMED_OUT
EXECUTION_CANCELLED
UNKNOWN_REQUIRES_RECOVERY
```

- [ ] Add `containment_evidence` to `WingetInstallExecutionReport`.
- [ ] Replace the obsolete limitation claiming descendants cannot be proven terminated with dynamic evidence-backed wording.
- [ ] Repair `api.ts` approval state/boolean drift.
- [ ] Add `cancelWingetInstallExecution(executionId)`.
- [ ] Show one cancel button only while active; disable after one request.
- [ ] Render `UNKNOWN_REQUIRES_RECOVERY` distinctly with retained-lock warning and safest next action.
- [ ] Update the JSON schema and exact status mappings in CLI/daemon/UI.

Run:

```powershell
cargo test -p toolos-winget
python -m json.tool schemas/winget-install-execution.schema.json > $null
pnpm --dir apps/toolos-ui check
pnpm --dir apps/toolos-ui build
```

Commit: `feat: expose containment and cancellation states`.

---

## Task 8 — Permanent CI and documentation

Add a non-optional `windows-process-containment` job to `.github/workflows/verify.yml`:

```yaml
runs-on: windows-latest
steps:
  - uses: actions/checkout@v4
  - uses: dtolnay/rust-toolchain@stable
    with:
      components: rustfmt, clippy
  - uses: Swatinem/rust-cache@v2
  - run: cargo clippy -p toolos-process --all-targets -- -D warnings
  - run: cargo test -p toolos-process --test windows_job_object -- --nocapture
  - run: cargo test --workspace
```

Do not weaken the existing Linux Rust, React, or Tauri-Windows jobs.

Update ADR/README to state:

- assignment occurs during process creation;
- timeout/cancel requires zero active processes;
- daemon crash kills descendants, but database reconciliation remains Issue #9;
- package/application verification remains Issue #10;
- PR #7 remains Draft.

Final local gates:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/toolos-ui check
pnpm --dir apps/toolos-ui build
cargo check --manifest-path apps/toolos-ui/src-tauri/Cargo.toml
git diff --check
```

Commit: `ci: prove Windows process-tree containment`.

---

## Task 9 — Final evidence and decision

- [ ] Push without force-pushing.
- [ ] Verify final-SHA success for Rust workspace, React frontend, Tauri Windows compile, and Windows process containment.
- [ ] PR comment must include final SHA, Windows run ID, exact native test names, repetition result or `NOT_EXECUTED`, containment method, marker-negative proofs, bounded-output proof, and Issue #9 limitation.
- [ ] Close Issue #8 only when every acceptance item is proven on the final SHA.
- [ ] Otherwise leave Issue #8 open and report exactly one failing command/log and one next action.
- [ ] Keep PR #7 Draft even if Issue #8 closes.

## Allowed run exits

**`ISSUE_8_VERIFIED`** — all native tests and repository CI pass; Issue #8 closes; PR #7 remains Draft.

**`BLOCKED_SOURCE_MISSING`** — an exact required Win32 contract is unproven; no guessed FFI is committed.

**`BLOCKED_IMPLEMENTATION`** — one reproducible blocker remains. Report goal, final SHA, failing command, expected/actual result, evidence path, proven/unknown root cause, and one next action. Do not start unrelated work.
