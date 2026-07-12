# ADR-0004: Execute only pinned user-scope WinGet plans

## Status

Proposed and verified as a draft implementation. Windows process-tree containment and crash-safe execution reconciliation are implemented and exercised by native process tests plus restart simulations. Production merge remains blocked by Issue #10 installed-state/application-health verification.

## Decision

ToolOS may invoke `winget install` only when all of the following are true:

- the exact package ID and source were resolved immediately before execution;
- version, architecture, and `user` scope are explicit;
- the derived command is byte-for-byte equal to the immutable approved preview;
- the plan hash, receipt, second execution phrase, expiry, and local lock all match;
- the receipt is consumed atomically before process launch;
- the command contains `--id`, `--exact`, `--source`, `--version`, `--scope user`,
  `--architecture`, `--no-upgrade`, and `--disable-interactivity`;
- the command contains no agreement acceptance, force, security-hash bypass, dependency
  skip, custom installer arguments, override, manifest, header, or reboot allowance;
- the mutating adapter is assigned to a Windows Job Object during `CreateProcessW` through
  `PROC_THREAD_ATTRIBUTE_JOB_LIST`, before it can launch WinGet or installer descendants;
- only the intended stdin/stdout/stderr handles are inherited through
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`;
- the Job Object uses `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` as the daemon-crash backstop.

Machine-scope execution remains blocked. Agreement prompts fail closed rather than being
accepted by ToolOS. The mutating path does not fall back to starting an uncontained process and
assigning it to a Job Object afterward.

## Cancellation and process evidence

The client supplies an `execution_id` before the blocking execution request. A second client can
send `winget.install.cancel` for that ID. Timeout and explicit cancellation terminate the complete
Job Object, wait for the root process, and query Job Object accounting until
`ActiveProcesses == 0` or the bounded confirmation window expires.

The executing adapter continuously reads WinGet stdout and stderr. It retains bounded final
streams and mirrors observed provider chunks to adapter stderr. Adapter stdout remains reserved
for one JSON-RPC response. Therefore output observed before timeout or cancellation remains in the
daemon-level evidence even when the adapter cannot emit its final response.

ToolOS persists:

- containment method and root PID;
- requested stop reason;
- active-process count after cleanup;
- descendant-termination and containment-confirmation verdicts;
- bounded stdout/stderr, exit code, duration, fresh preflight reports, and optional post-state
  evidence.

## Status boundary

A terminal contained status requires both `containment_confirmed == true` and
`active_processes_after_cleanup == 0`.

- normal provider exit zero becomes `PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED`;
- normal provider nonzero becomes `PROVIDER_FAILED`;
- confirmed timeout becomes `TIMED_OUT_CONTAINED`;
- confirmed explicit cancellation becomes `CANCELLED_CONTAINED`;
- any unconfirmed containment becomes `UNKNOWN_REQUIRES_RECOVERY`.

For `UNKNOWN_REQUIRES_RECOVERY`, ToolOS skips the post-install query and retains the WinGet lock.
It must not silently classify the action as failed, cancelled, or complete.

WinGet exit code zero is provider evidence, not a definitive installed-state or
application-health verdict, because the stable CLI output used by this slice is locale-dependent.

## Durable execution journal

Approval moves a plan to `APPROVED_AWAITING_EXECUTION`; the legacy
`APPROVED_EXECUTION_DISABLED` state is never executable. The one-time arm is consumed atomically
with the plan transition to `EXECUTING` and creation of a durable SQLite journal.

The journal uses these ordered phases:

- `PREPARED`: approval was consumed and the journal committed, but process creation was not yet
  attempted;
- `SPAWN_INTENT`: the next operation is `CreateProcessW`, so a crash is ambiguous;
- `SPAWNED`: root PID and containment method are durable;
- `PROVIDER_FINISHED`: complete bounded provider and containment evidence are durable before
  post-state or final plan persistence;
- `FINALIZED`: plan state, journal resolution, and resource-lock decision are committed.

No recovery path replays the interrupted installation.

## Startup reconciliation

The daemon reconciles every unresolved journal before serving IPC requests:

- `PREPARED` becomes `RECOVERED_NO_PROCESS_STARTED`; because process creation was not attempted,
  the WinGet lock is released;
- `PROVIDER_FINISHED` with confirmed containment and zero active processes is finalized from the
  persisted provider result without replaying WinGet;
- `SPAWN_INTENT`, `SPAWNED`, malformed records, missing provider evidence, or unconfirmed
  containment become `UNKNOWN_REQUIRES_RECOVERY`; the lock is retained indefinitely;
- new WinGet approval and execution requests fail closed while a blocking recovery record exists.

When available, startup reconciliation performs a read-only exact installed-state query. Failure of
that query does not prevent fail-closed reconciliation and does not cause a speculative success or
cleanup claim.

## Residual-state evidence

Before process creation, ToolOS stores a bounded provider-specific manifest containing the exact
selector, provider identity/version, full pre-execution installed-state report, and a SHA-256 plus
entry count for the daemon `PATH` environment. PATH contents are not stored.

A post-state manifest, when available, is compared against the pre-state for:

- provider-version change;
- PATH fingerprint or entry-count change;
- exact installed-state evidence change;
- definitive installed-match values when the provider can supply them.

Observed changes are not automatically attributed to the package. Unchanged observations do not
prove that files, registry values, services, tasks, drivers, processes, PATH edits, or package
registrations are absent.

## Cleanup boundary

For `UNKNOWN_REQUIRES_RECOVERY` or a future evidence-backed residual status, ToolOS may create an
immutable cleanup plan containing inspection steps and an exact uninstall preview. Cleanup has a
separate short-lived approval phrase and persists an approval receipt.

Both the cleanup plan and receipt have `execution_enabled == false`. Issue #9 intentionally exposes
no cleanup execution endpoint. Approval records intent only; ToolOS does not delete files, edit the
registry, stop services, kill processes, or invoke uninstall as part of recovery.

## Verification evidence

The permanent Windows CI job must run:

- workspace Clippy with warnings denied;
- process-runner unit tests;
- native parent/grandchild tests for normal exit, timeout, explicit cancellation, abrupt host exit,
  and bounded output;
- the complete Rust workspace test suite on Windows.

The native tests must prove that delayed grandchild survival markers remain absent after timeout,
explicit cancellation, and closure of the final Job Object handle. The explicit-cancel test also
proves that stderr emitted before termination remains captured.

Recovery tests seed valid action-plan, approval, lock, and journal data, close the SQLite database,
reopen it as a restarted daemon, and reconcile these crash points:

- `PREPARED` releases the lock and unblocks writes;
- `SPAWN_INTENT` retains the lock and blocks writes;
- `SPAWNED` retains the lock and blocks writes;
- `PROVIDER_FINISHED` finalizes from persisted evidence without replay and releases the lock when
  containment is confirmed.

Migration tests verify that databases created before the journal and cleanup tables are upgraded.
Storage tests verify ordered compare-and-set phase transitions, atomic start/finalization, retained
locks, and permanently execution-disabled cleanup approval.

## Merge gate

PR #7 remains Draft until all of the following are true:

- Issue #8 final-SHA CI and repeated native process-tree evidence are complete;
- Issue #9 final-SHA CI, restart reconciliation, residual-state reporting, and disabled cleanup
  planning are complete;
- Issue #10 defines a locale-stable installed-state verdict and package-health boundary;
- the final branch passes Rust formatting, Clippy with warnings denied, workspace tests, recovery
  schema validation, React typecheck/build, native Windows process tests, and native Windows Tauri
  compilation.
