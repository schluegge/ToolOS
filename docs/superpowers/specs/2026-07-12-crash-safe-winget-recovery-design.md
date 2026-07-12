# Crash-Safe WinGet Recovery Design

## Goal

Make every interrupted governed WinGet execution recoverable from durable evidence before ToolOS accepts another machine mutation.

## Decision

ToolOS will use a phase-based execution journal stored in SQLite. The journal is created atomically with the transition to `EXECUTING`, records a spawn intent before `CreateProcessW`, records process identity after spawn, and records the complete provider/containment result before final plan persistence.

Journal phases:

- `PREPARED`: approval consumed and journal committed; spawn has not been attempted.
- `SPAWN_INTENT`: the next operation is process creation; a crash from this point is ambiguous.
- `SPAWNED`: root process identity and containment method are durable.
- `PROVIDER_FINISHED`: bounded provider and containment evidence are durable but the action plan may not yet be finalized.
- `FINALIZED`: action plan, journal, and resource-lock decision are committed.

## Recovery rules

- `PREPARED` is automatically resolved as `RECOVERED_NO_PROCESS_STARTED`; no machine process could have started, so the lock may be released.
- `PROVIDER_FINISHED` is deterministically finalized from the persisted provider result. No execution is replayed.
- `SPAWN_INTENT`, `SPAWNED`, malformed journals, and unconfirmed containment become `UNKNOWN_REQUIRES_RECOVERY`; the WinGet lock is retained indefinitely.
- A restart-time read-only WinGet query may enrich the report, but localized output cannot automatically prove package health or attribute residuals to this execution.
- No files, services, registry entries, PATH entries, package registrations, or other residuals are deleted automatically.

## Residual-state manifest

The bounded provider-specific manifest records:

- exact package selector;
- provider identity/version;
- complete pre-execution installed-state report;
- SHA-256 and entry count of the daemon PATH environment without storing path contents;
- observable surfaces and explicitly unobserved surfaces.

A post-state manifest uses the same structure. The diff reports provider-version changes, PATH-fingerprint changes, installed-evidence changes, definitive installed-match values when available, and limitations. Observed changes are not automatically attributed to the package.

## Recovery gate

Daemon startup reconciles all unresolved journals before serving requests. While any journal remains `UNKNOWN_REQUIRES_RECOVERY`, `winget.install.approve` and `winget.install.execute` fail closed. Read-only inspection, recovery reporting, and planning-only cleanup workflows remain available.

## Read-only recovery API

- `winget.recovery.list`
- `winget.recovery.get`
- `winget.recovery.cleanup.plan`
- `winget.recovery.cleanup.approve`

The cleanup plan is immutable, separately approved, and execution-disabled. It may show an exact uninstall preview and inspection steps, but ToolOS does not execute cleanup in Issue #9.

## Evidence and testing

Required tests cover migration from the pre-journal schema, crash after `PREPARED`, crash after `SPAWN_INTENT`, crash after `SPAWNED`, crash after `PROVIDER_FINISHED`, restart mutation blocking, retained-lock behavior, deterministic finalization, residual diffing, and cleanup-plan approval without execution.

Issue #10 remains responsible for locale-stable installed-state and application-health verification.