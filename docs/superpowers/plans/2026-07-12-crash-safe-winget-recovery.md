# Crash-Safe WinGet Recovery Implementation Plan

> Execute task-by-task. PR #7 remains Draft. Issue #10 and LocalStack remain out of scope.

## Goal

Complete Issue #9 with durable execution journaling, restart-time reconciliation, read-only recovery evidence, and separately approved execution-disabled cleanup plans.

## Task 1 — Domain contracts

Create `crates/toolos-winget/src/recovery.rs` with:

- `ExecutionJournalPhase`: `Prepared`, `SpawnIntent`, `Spawned`, `ProviderFinished`, `Finalized`.
- `RecoveryStatus`: `RecoveredNoProcessStarted`, `RecoveredFromPersistedProviderResult`, `FailedResidualsPresent`, `UnknownRequiresRecovery`.
- `WingetResidualStateManifest`, `WingetResidualStateDiff`, `WingetRecoveryReport`.
- `WingetRecoveryCleanupPlan`, status, approval challenge, hash and phrase helpers.
- PATH fingerprint helper storing only SHA-256 and entry count.
- Unit tests for phase/status serialization, residual diffing, cleanup-plan hash stability, and no executable cleanup command.

## Task 2 — SQLite journal and migration

Add migration 5 for `execution_journal` with IDs, phase, plan/approval/hash/resource, provider identity, command, pre-state, process identity, provider result, recovery policy, timestamps, resolution status/report, and indexes.

Add storage methods:

- create journal atomically inside `begin_action_execution`;
- mark spawn intent;
- mark spawned;
- mark provider finished;
- list/get unresolved journals;
- resolve prepared journal and release lock atomically;
- finalize persisted provider result atomically;
- mark unknown and retain lock indefinitely;
- list/get recovery reports.

Tests: migrate old database, every phase transition, invalid transition rejection, prepared recovery, provider-finished recovery, unknown lock retention.

## Task 3 — Runtime journaling

In `winget_install_execute`:

1. Build pre-state manifest.
2. Begin execution and journal atomically.
3. Persist `SPAWN_INTENT` before process creation.
4. Persist `SPAWNED` with root PID immediately after creation.
5. Persist `PROVIDER_FINISHED` before post-state/final plan transaction.
6. Finalize plan, lock decision, and journal together.

No replay after failure.

## Task 4 — Startup reconciliation gate

Before serving IPC, reconcile every unresolved journal:

- `PREPARED` → recovered no process started, release lock.
- `PROVIDER_FINISHED` → rebuild/finalize report from persisted evidence.
- `SPAWN_INTENT`, `SPAWNED`, malformed/unconfirmed → unknown, retain lock.

Capture a read-only post-state manifest when available. Persist one recovery evidence record per journal. Reject new install approval/execution while unknown recovery records exist.

## Task 5 — Recovery APIs, CLI, and UI

Add list/get recovery methods and a visible Recovery panel. Show phase, status, lock decision, pre/post diff, limitations, and safest next action.

Add cleanup-plan create/approve endpoints. Plans are immutable, separately approved, and `execution_enabled=false`; no cleanup execution endpoint exists.

## Task 6 — Schemas and documentation

Add recovery-report and cleanup-plan schemas, validate in CI, update README/ADR, and explicitly retain Issue #10 as the remaining merge blocker.

## Task 7 — Verification

Required final gates:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
JSON schema validation
React typecheck/build
native Windows Tauri compile
Windows process-containment suite
```

Close Issue #9 only with final-SHA evidence. PR #7 remains Draft until Issue #10 is complete.