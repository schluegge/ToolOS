# Windows Process-Tree Containment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete ToolOS Issue #8 by proving that the mutating WinGet adapter and every descendant installer process are contained, cancellable, and either confirmed terminated or reported as `UNKNOWN_REQUIRES_RECOVERY`.

**Architecture:** Keep the existing immutable-plan and approval flow. Add one small Rust crate that owns the only permitted Win32 `unsafe` boundary and launches the mutating adapter directly into a Windows Job Object during `CreateProcessW`. The daemon persists containment metadata before waiting, supports explicit cancellation, and releases the package-manager lock only after `ActiveProcesses == 0` is confirmed. Read-only adapter calls remain on the existing Tokio path.

**Tech Stack:** Rust 1.88+, Tokio 1, `windows-sys` 0.61, Win32 Job Objects, `STARTUPINFOEXW`, Tauri 2, React/TypeScript, SQLite/rusqlite, GitHub Actions `windows-latest`.

## Global Constraints

- Work only on branch `build/winget-user-execution` and PR #7.
- Keep PR #7 in Draft state throughout this run.
- Do not merge PR #7 during this run.
- Do not work on LocalStack, Awesome Stacks, Issue #9, Issue #10, or unrelated infrastructure.
- Machine-scope install, agreement auto-acceptance, elevation, arbitrary shell execution, uninstall execution, force, hash bypass, dependency skip, custom installer arguments, override, manifest, header, and reboot allowance remain blocked.
- The public process API must be safe Rust. All Win32 `unsafe` calls must be isolated inside `crates/toolos-process/src/windows.rs`.
- `stdout` remains the adapter JSON-RPC channel. Live provider output may be mirrored only to adapter `stderr`.
- No success claim may be based only on exit code. Process containment is proven only when the Job Object reports zero active processes.
- If containment cannot be confirmed, the action plan must become `UNKNOWN_REQUIRES_RECOVERY`, post-install querying must be skipped, and the WinGet lock must remain held.
- Do not weaken existing CI, tests, schema validation, or approval gates.
- Use exact official API signatures already copied from `windows-rs`; if any required signature or constant remains uncertain, stop with `BLOCKED_SOURCE_MISSING` before writing that call.

---

## File Map

**Create**

- `crates/toolos-process/Cargo.toml` — isolated process-containment crate and Windows-only `windows-sys` dependency.
- `crates/toolos-process/src/lib.rs` — safe cross-platform API, public types, and non-Windows fail-closed implementation.
- `crates/toolos-process/src/windows.rs` — only Win32 implementation and only permitted `unsafe` boundary.
- `crates/toolos-process/src/bin/toolos-process-fixture.rs` — deterministic parent/grandchild fixture used by native Windows tests.
- `crates/toolos-process/tests/windows_job_object.rs` — real Windows process-tree integration tests.

**Modify**

- `Cargo.toml` — add `crates/toolos-process` workspace member and `windows-sys = "0.61"` workspace dependency.
- `Cargo.lock` — generated dependency lock update.
- `apps/toolos-daemon/Cargo.toml` — depend on `toolos-process`.
- `apps/toolos-daemon/src/main.rs` — active-execution registry, contained execution path, cancellation RPC, fail-closed status mapping, event/evidence persistence.
- `adapters/toolos-winget-adapter/src/main.rs` — bounded live tee for the mutating WinGet command while preserving JSON-RPC stdout.
- `crates/toolos-winget/src/lib.rs` — containment evidence types and new terminal/recovery plan states.
- `crates/toolos-winget/src/execution.rs` — status derivation and recovery-safe report construction.
- `crates/toolos-storage/src/lib.rs` — terminal finish that conditionally releases the resource lock.
- `apps/toolos-cli/src/main.rs` — explicit execution-cancel command and status rendering.
- `apps/toolos-ui/src/api.ts` — synchronize existing approval types, add containment types/statuses and cancel API.
- `apps/toolos-ui/src/InstallPlanPanel.tsx` — cancel control and recovery-state rendering.
- `apps/toolos-ui/src/install-plan.css` — minimal styling for cancel/recovery state only.
- `schemas/winget-install-execution.schema.json` — containment evidence and new statuses.
- `.github/workflows/verify.yml` — native Windows process-containment test job.
- `docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md` — exact containment and residual recovery boundary.
- `README.md` — current capability and remaining blocker wording.

---

### Task 1: Freeze the Baseline and Repair Existing Surface Drift

**Files:**
- Inspect: `apps/toolos-ui/src/api.ts`
- Inspect: `crates/toolos-winget/src/lib.rs`
- Inspect: `schemas/winget-install-execution.schema.json`
- Inspect: `apps/toolos-daemon/src/main.rs`

**Produces:** A baseline note in the implementation PR comment listing exact pre-existing mismatches; no production code change yet.

- [ ] **Step 1: Create an isolated worktree**

```powershell
git fetch origin
git worktree add ..\ToolOS-issue8 origin/build/winget-user-execution
Set-Location ..\ToolOS-issue8
git switch -c build/winget-user-execution-issue8
```

Expected: a clean worktree based on the current PR head.

- [ ] **Step 2: Record source identity**

```powershell
git rev-parse HEAD
git status --short
git diff --check
```

Expected: one commit SHA, empty status, and no diff errors.

- [ ] **Step 3: Run the unchanged baseline**

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

Expected: all commands exit `0`. If any command fails before implementation, record it as `BASELINE_BLOCKER` and stop.

- [ ] **Step 4: Record known existing contract drift**

Verify and record that `apps/toolos-ui/src/api.ts` still omits `APPROVED_AWAITING_EXECUTION` and incorrectly types the approval receipt as `APPROVED_EXECUTION_DISABLED`/`execution_enabled: false`, while Rust uses the armed state. This drift must be fixed in Task 7, not silently ignored.

- [ ] **Step 5: Commit only a baseline note if one is added**

```powershell
git status --short
```

Expected: no code changes from Task 1.

---

### Task 2: Add the Safe Process-Containment Contract

**Files:**
- Create: `crates/toolos-process/Cargo.toml`
- Create: `crates/toolos-process/src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessStopReason {
    Exited,
    TimedOut,
    Cancelled,
    DaemonShutdown,
    ContainmentFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProcessContainmentEvidence {
    pub method: String,
    pub root_pid: Option<u32>,
    pub stop_reason: ProcessStopReason,
    pub active_processes_after_cleanup: Option<u32>,
    pub descendants_terminated: Option<bool>,
    pub containment_confirmed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct ContainedCommandSpec {
    pub executable: OsString,
    pub args: Vec<OsString>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

pub struct ContainedProcess {
    execution_id: Uuid,
    root_pid: u32,
    control: ContainedProcessControl,
    completion: JoinHandle<Result<ContainedProcessOutput, ContainmentError>>,
}

impl ContainedProcess {
    pub fn execution_id(&self) -> Uuid;
    pub fn root_pid(&self) -> u32;
    pub fn control(&self) -> ContainedProcessControl;
    pub async fn wait(self) -> Result<ContainedProcessOutput, ContainmentError>;
}

#[derive(Clone)]
pub struct ContainedProcessControl;

impl ContainedProcessControl {
    pub fn cancel(&self, reason: ProcessStopReason) -> Result<(), ContainmentError>;
}

pub fn spawn_contained(spec: ContainedCommandSpec) -> Result<ContainedProcess, ContainmentError>;
```

- [ ] **Step 1: Write compile-fail expectations first**

Add unit tests in `crates/toolos-process/src/lib.rs` proving:

```rust
#[test]
fn non_windows_mutating_execution_fails_closed() {
    #[cfg(not(windows))]
    assert!(matches!(
        spawn_contained(test_spec()),
        Err(ContainmentError::UnsupportedPlatform)
    ));
}
```

- [ ] **Step 2: Run the new crate test before implementation**

```powershell
cargo test -p toolos-process
```

Expected: fail because the crate/API does not yet exist.

- [ ] **Step 3: Add workspace membership and dependency**

Add `"crates/toolos-process"` to `[workspace].members` and:

```toml
windows-sys = "0.61"
```

under `[workspace.dependencies]`.

- [ ] **Step 4: Create the crate manifest**

`crates/toolos-process/Cargo.toml` must use target-specific Windows features:

```toml
[package]
name = "toolos-process"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
schemars.workspace = true
serde.workspace = true
thiserror.workspace = true
tokio.workspace = true
uuid.workspace = true

[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true, features = [
  "Win32_Foundation",
  "Win32_Security",
  "Win32_Storage_FileSystem",
  "Win32_System_Console",
  "Win32_System_JobObjects",
  "Win32_System_Pipes",
  "Win32_System_Threading",
] }

[lints.clippy]
all = "warn"

[lints.rust]
unsafe_op_in_unsafe_fn = "deny"
```

Do not inherit the workspace `unsafe_code = "forbid"` lint in this one crate; every other crate continues inheriting it.

- [ ] **Step 5: Implement the safe public API and non-Windows fail-closed path**

No public function may be `unsafe`. `src/lib.rs` may only select `mod windows` under `#[cfg(windows)]` and return `UnsupportedPlatform` elsewhere.

- [ ] **Step 6: Run crate tests**

```powershell
cargo test -p toolos-process
cargo clippy -p toolos-process --all-targets -- -D warnings
```

Expected: pass on non-Windows without claiming containment support.

- [ ] **Step 7: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/toolos-process
git commit -m "feat: define fail-closed process containment API"
```

---

### Task 3: Implement Windows Job-Object Ownership at Process Creation

**Files:**
- Create: `crates/toolos-process/src/windows.rs`

**Consumes:** `ContainedCommandSpec`, `ContainedProcess`, and `ProcessContainmentEvidence` from Task 2.

**Required Win32 contract:**

- Create the Job Object with `CreateJobObjectW`.
- Set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` through `SetInformationJobObject` and `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`.
- Create inheritable stdin/stdout/stderr pipes and clear inheritance on parent-side pipe handles.
- Build one `STARTUPINFOEXW` attribute list.
- Add the Job Object with `PROC_THREAD_ATTRIBUTE_JOB_LIST` so the child is assigned during `CreateProcessW`, not afterward.
- Add only the child stdin/stdout/stderr handles with `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.
- Call `CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT`, `CREATE_UNICODE_ENVIRONMENT`, and `bInheritHandles = true`.
- Never set a breakaway flag.
- Close the child thread handle immediately after successful creation.
- Query `JobObjectBasicAccountingInformation` before declaring containment complete.
- On timeout/cancel, call `TerminateJobObject`, wait for the root process, then poll Job Object accounting until `ActiveProcesses == 0` or a bounded confirmation timeout expires.
- Closing the final Job Object handle must remain a last-resort kill-on-close backstop.

- [ ] **Step 1: Add unit tests for command-line quoting**

Implement and test a Windows command-line encoder covering empty arguments, spaces, embedded quotes, and trailing backslashes. Expected examples:

```rust
assert_eq!(quote_windows_arg(OsStr::new("")), "\"\"");
assert_eq!(quote_windows_arg(OsStr::new("a b")), "\"a b\"");
assert_eq!(quote_windows_arg(OsStr::new("plain")), "plain");
```

- [ ] **Step 2: Implement RAII handle wrappers**

Create private wrappers whose `Drop` calls `CloseHandle`. Do not expose raw handles publicly. Every ownership transfer must use an explicit constructor or `into_raw` method.

- [ ] **Step 3: Implement attribute-list allocation and cleanup**

Use the documented two-call `InitializeProcThreadAttributeList` pattern. Always call `DeleteProcThreadAttributeList` after `CreateProcessW`, including error paths.

- [ ] **Step 4: Implement contained spawn**

The child must not execute outside the Job Object. If `PROC_THREAD_ATTRIBUTE_JOB_LIST` cannot be installed, return `ContainmentError::ContainmentUnavailable`; do not fall back to `AssignProcessToJobObject` for the mutating path.

- [ ] **Step 5: Implement bounded concurrent pipe readers**

Read stdout and stderr on separate threads or blocking tasks. Keep reading after the root process exits until pipe EOF so descendant output inherited through the Job Object is not silently lost. Bound retained output independently to the configured byte limits and append one truncation marker.

- [ ] **Step 6: Implement stop and confirmation handling**

The completion result must always include one of:

```text
EXITED + containment_confirmed=true
TIMED_OUT + containment_confirmed=true
CANCELLED + containment_confirmed=true
DAEMON_SHUTDOWN + containment_confirmed=true (proved by external fixture)
CONTAINMENT_FAILED + containment_confirmed=false
```

- [ ] **Step 7: Run Windows compile gates**

```powershell
cargo check -p toolos-process --all-targets
cargo clippy -p toolos-process --all-targets -- -D warnings
```

Expected: pass on Windows.

- [ ] **Step 8: Commit**

```powershell
git add crates/toolos-process/src/windows.rs crates/toolos-process/src/lib.rs
git commit -m "feat: launch process trees inside Windows Job Objects"
```

---

### Task 4: Prove Real Descendant Termination on Windows

**Files:**
- Create: `crates/toolos-process/src/bin/toolos-process-fixture.rs`
- Create: `crates/toolos-process/tests/windows_job_object.rs`

**Produces:** Real OS-level evidence, not mocked lifecycle tests.

- [ ] **Step 1: Create the deterministic fixture binary**

The fixture must support these exact modes:

```text
grandchild --marker <path> --delay-ms <u64>
parent --fixture <exe> --marker <path> --delay-ms <u64>
host-exit --fixture <exe> --marker <path> --delay-ms <u64>
output --stdout-bytes <usize> --stderr-bytes <usize>
```

`parent` starts `grandchild`; `grandchild` waits then writes the marker. `host-exit` starts the parent through `toolos-process`, signals readiness, then terminates its own host process without graceful cleanup.

- [ ] **Step 2: Write the normal-exit test**

Assert:

- root PID is present;
- stdout and stderr are captured;
- stop reason is `Exited`;
- `active_processes_after_cleanup == Some(0)`;
- `containment_confirmed == true`.

- [ ] **Step 3: Write the timeout descendant test**

Use a marker delay longer than the process timeout. After the contained run returns, wait beyond the original marker delay and assert the marker does not exist.

- [ ] **Step 4: Write the explicit-cancel descendant test**

Spawn the contained parent, wait for readiness, call `control.cancel(ProcessStopReason::Cancelled)`, then assert the grandchild marker never appears.

- [ ] **Step 5: Write the daemon-shutdown/last-handle-close test**

Launch `host-exit`, wait for that host process to exit, then wait beyond the grandchild marker delay and assert the marker does not exist. This proves kill-on-job-close independently of graceful daemon code.

- [ ] **Step 6: Write the bounded-output test**

Generate more than 64 KiB on each stream. Assert both streams contain their truncation marker and remain below 66 KiB.

- [ ] **Step 7: Run the real Windows tests repeatedly**

```powershell
1..5 | ForEach-Object {
  cargo test -p toolos-process --test windows_job_object -- --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
```

Expected: five consecutive passes. Any intermittent marker creation is a containment failure, not a flaky test to suppress.

- [ ] **Step 8: Commit**

```powershell
git add crates/toolos-process/src/bin crates/toolos-process/tests
git commit -m "test: prove Windows descendant termination"
```

---

### Task 5: Integrate Containment into the Mutating Daemon Path

**Files:**
- Modify: `apps/toolos-daemon/Cargo.toml`
- Modify: `apps/toolos-daemon/src/main.rs`
- Modify: `crates/toolos-storage/src/lib.rs`

**Interfaces:**

```rust
struct ActiveExecution {
    plan_id: Uuid,
    control: ContainedProcessControl,
}

struct AppState {
    // existing fields
    active_executions: Arc<tokio::sync::Mutex<HashMap<Uuid, ActiveExecution>>>,
}
```

Add RPC:

```text
winget.install.cancel
params: { "execution_id": "<UUID>" }
result: { "execution_id": "<UUID>", "cancel_requested": true }
```

- [ ] **Step 1: Write failing daemon status tests**

Extract pure mapping functions and test:

```rust
confirmed normal exit code 0 -> EXECUTION_SUCCEEDED_UNVERIFIED
confirmed provider nonzero -> EXECUTION_FAILED
confirmed timeout -> EXECUTION_TIMED_OUT
confirmed explicit cancel -> EXECUTION_CANCELLED
unconfirmed containment -> UNKNOWN_REQUIRES_RECOVERY
```

- [ ] **Step 2: Add the process crate dependency**

```toml
toolos-process = { path = "../../crates/toolos-process" }
```

- [ ] **Step 3: Split read-only and mutating adapter invocation**

Keep current `invoke_adapter` for read-only methods. Add `invoke_mutating_adapter_contained` used only for `winget.install.execute`.

- [ ] **Step 4: Persist containment start before awaiting completion**

Immediately after spawn, append an event containing:

```json
{
  "execution_id": "...",
  "plan_id": "...",
  "containment_method": "WINDOWS_JOB_OBJECT",
  "root_pid": 1234
}
```

Register the execution in `active_executions` before awaiting the result.

- [ ] **Step 5: Add explicit cancellation RPC**

The RPC must reject unknown/non-active execution IDs. It sends only `ProcessStopReason::Cancelled`; it does not directly alter the database state.

- [ ] **Step 6: Enforce fail-closed post-processing**

Run `winget_installed` after execution only when `containment_confirmed == true` and `active_processes_after_cleanup == Some(0)`. Otherwise skip post-state inspection.

- [ ] **Step 7: Preserve the lock for unknown containment**

Extend `ActionExecutionFinish` with:

```rust
pub release_resource_lock: bool,
```

Use `false` only for `UNKNOWN_REQUIRES_RECOVERY`. Storage must update the plan/evidence state atomically while retaining the lock row.

- [ ] **Step 8: Remove active registry entries in one `finally`-equivalent path**

Use a guard or one explicit cleanup section so success, failure, timeout, cancellation, adapter parse failure, and containment failure cannot leak active registry entries.

- [ ] **Step 9: Run daemon/storage tests**

```powershell
cargo test -p toolos-storage
cargo test -p toolos-daemon
cargo clippy -p toolos-daemon -p toolos-storage --all-targets -- -D warnings
```

Expected: pass.

- [ ] **Step 10: Commit**

```powershell
git add apps/toolos-daemon crates/toolos-storage
git commit -m "feat: contain and cancel mutating WinGet execution"
```

---

### Task 6: Preserve Provider Output During Forced Termination

**Files:**
- Modify: `adapters/toolos-winget-adapter/src/main.rs`

**Constraint:** Adapter stdout is JSON-RPC only. Provider live output is mirrored to adapter stderr.

- [ ] **Step 1: Write bounded stream collector tests**

Test separate stdout/stderr retention, independent 64 KiB truncation, UTF-8 loss handling, and that the mirror prefix cannot be mistaken for JSON-RPC output.

- [ ] **Step 2: Add `run_command_with_live_tee` for execution only**

Its contract:

```rust
async fn run_command_with_live_tee(
    executable: &str,
    args: &[String],
    timeout: Duration,
) -> Result<ProcessEvidence, String>;
```

Use it only from `execute_install_request`. Keep read-only probes on the existing simpler `run_command` path.

- [ ] **Step 3: Read both child streams concurrently**

For each chunk:

- retain bounded bytes for the final `ProcessEvidence`;
- write a bounded mirror line to adapter stderr with prefix `[winget stdout]` or `[winget stderr]`;
- never write provider bytes to adapter stdout.

- [ ] **Step 4: Verify forced-kill evidence at the daemon boundary**

Add a process fixture adapter test where the child emits output, then hangs. Cancel the contained adapter and assert the daemon-level captured stderr still contains the emitted marker.

- [ ] **Step 5: Run tests**

```powershell
cargo test -p toolos-winget-adapter
cargo clippy -p toolos-winget-adapter --all-targets -- -D warnings
```

Expected: pass.

- [ ] **Step 6: Commit**

```powershell
git add adapters/toolos-winget-adapter/src/main.rs
git commit -m "feat: preserve WinGet output across forced termination"
```

---

### Task 7: Synchronize Domain, Schema, CLI, and UI Contracts

**Files:**
- Modify: `crates/toolos-winget/src/lib.rs`
- Modify: `crates/toolos-winget/src/execution.rs`
- Modify: `apps/toolos-cli/src/main.rs`
- Modify: `apps/toolos-ui/src/api.ts`
- Modify: `apps/toolos-ui/src/InstallPlanPanel.tsx`
- Modify: `apps/toolos-ui/src/install-plan.css`
- Modify: `schemas/winget-install-execution.schema.json`

**Required statuses:**

```rust
pub enum WingetExecutionStatus {
    ProviderSucceededPostStateUnverified,
    ProviderFailed,
    TimedOutContained,
    CancelledContained,
    UnknownRequiresRecovery,
}

pub enum InstallPlanStatus {
    AwaitingApproval,
    Blocked,
    ApprovedExecutionDisabled,
    ApprovedAwaitingExecution,
    Executing,
    ExecutionSucceededUnverified,
    ExecutionFailed,
    ExecutionTimedOut,
    ExecutionCancelled,
    UnknownRequiresRecovery,
    Expired,
}
```

Add `containment_evidence: ProcessContainmentEvidence` to `WingetInstallExecutionReport`.

- [ ] **Step 1: Write failing report-mapping tests**

Cover every status above and assert exact `single_safest_next_action` text. `UnknownRequiresRecovery` must instruct the user not to create another package plan and to run recovery inspection.

- [ ] **Step 2: Replace the obsolete timeout limitation**

Remove the current claim that ToolOS cannot prove descendants terminated. Replace it with dynamic wording based on `containment_confirmed`.

- [ ] **Step 3: Repair pre-existing TypeScript approval drift**

`WingetInstallPlan.status` and `WingetInstallApprovalReceipt.status` must include/use `APPROVED_AWAITING_EXECUTION`, and an armed receipt must type `execution_enabled: true`.

- [ ] **Step 4: Add cancellation API and control**

Add:

```ts
cancelWingetInstallExecution: (executionId: string) =>
  daemonRequest<{ execution_id: string; cancel_requested: boolean }>(
    "winget.install.cancel",
    { execution_id: executionId },
  ),
```

The UI displays one `Cancel execution` button only while the current execution is active. Disable it after one click and show `Cancellation requested`.

- [ ] **Step 5: Render recovery state distinctly**

`UNKNOWN_REQUIRES_RECOVERY` must display the retained-lock warning and the exact safest next action. Do not label it as failed, cancelled, or completed.

- [ ] **Step 6: Update the JSON schema**

Require containment method, root PID when spawn succeeded, stop reason, active-process count, descendants-terminated verdict, confirmation boolean, and detail string.

- [ ] **Step 7: Validate contracts**

```powershell
cargo test -p toolos-winget
python -m json.tool schemas/winget-install-execution.schema.json > $null
pnpm --dir apps/toolos-ui check
pnpm --dir apps/toolos-ui build
```

Expected: pass.

- [ ] **Step 8: Commit**

```powershell
git add crates/toolos-winget apps/toolos-cli apps/toolos-ui schemas
git commit -m "feat: expose containment and cancellation states"
```

---

### Task 8: Add Permanent Windows CI and Documentation Evidence

**Files:**
- Modify: `.github/workflows/verify.yml`
- Modify: `docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md`
- Modify: `README.md`

- [ ] **Step 1: Add the Windows containment CI job**

Add:

```yaml
  windows-process-containment:
    name: Windows process containment
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

Do not remove or weaken the existing Linux Rust, React, or Tauri-Windows jobs.

- [ ] **Step 2: Update ADR-0004**

Document:

- Job assignment occurs during `CreateProcessW`;
- kill-on-job-close is a backstop;
- timeout/cancel requires confirmed zero active processes;
- daemon crash kills descendants but database reconciliation remains Issue #9;
- installed-state/application health remains Issue #10.

- [ ] **Step 3: Update README capability wording**

State that the draft path has proven process-tree containment on Windows, but PR #7 remains Draft because crash-state reconciliation and verification remain open.

- [ ] **Step 4: Run local final gates**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/toolos-ui check
pnpm --dir apps/toolos-ui build
cargo check --manifest-path apps/toolos-ui/src-tauri/Cargo.toml
git diff --check
```

Expected: all exit `0`.

- [ ] **Step 5: Commit**

```powershell
git add .github/workflows/verify.yml README.md docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md
git commit -m "ci: prove Windows process-tree containment"
```

---

### Task 9: Final Forensic Review and Issue Decision

**Files:**
- Review all changed files.
- Update GitHub Issue #8 and PR #7 only after CI completes.

- [ ] **Step 1: Push the implementation branch**

```powershell
git push -u origin build/winget-user-execution-issue8
```

- [ ] **Step 2: Update PR #7 head only through a reviewed fast-forward or merge into `build/winget-user-execution`**

Do not force-push. Preserve the existing PR history.

- [ ] **Step 3: Verify all four CI groups on the final SHA**

Required conclusions:

```text
Rust workspace: success
React frontend: success
Tauri Windows compile: success
Windows process containment: success
```

- [ ] **Step 4: Review exact acceptance evidence**

The final PR comment must list:

- final commit SHA;
- Windows CI run ID;
- exact native test names;
- five-run local repetition result or state `NOT_EXECUTED` if unavailable;
- containment method;
- proof that parent and grandchild markers stayed absent after timeout, explicit cancel, and host exit;
- bounded-output proof;
- known limitation that database recovery after daemon crash remains Issue #9.

- [ ] **Step 5: Decide Issue #8 honestly**

Close Issue #8 only when every acceptance item is evidenced on the final SHA. Otherwise leave it open and add exactly one concrete blocker with its failing test/log location.

- [ ] **Step 6: Keep PR #7 Draft**

Even if Issue #8 closes, PR #7 remains Draft because Issues #9 and #10 are still merge blockers.

---

## Run Exit Contract

The run may end in only one of these states:

### `ISSUE_8_VERIFIED`

All required Windows tests and repository CI pass on the final commit; Issue #8 is closed with exact evidence; PR #7 remains Draft.

### `BLOCKED_SOURCE_MISSING`

An exact required Win32 signature, constant, feature, or platform contract is not established by primary-source evidence. No guessed FFI call is committed.

### `BLOCKED_IMPLEMENTATION`

One reproducible implementation or test blocker remains. The run reports exactly:

```text
Goal
Current final commit
Failing command
Expected result
Actual result
Relevant log/file path
Root cause proven or still unknown
Single next action
```

No unrelated backlog work may be started after a blocker.

## Self-Review Result

- Scope is limited to Issue #8.
- LocalStack remains backlog-only.
- Issue #9 crash reconciliation is not falsely implemented here.
- Issue #10 package/application verification is not conflated with process containment.
- Existing approval and execution restrictions remain intact.
- Every new executable status has an explicit plan-state mapping.
- Unknown containment retains the lock and skips post-state queries.
- Native Windows descendant tests are mandatory and permanent in CI.
