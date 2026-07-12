# ADR-0004: Execute only pinned user-scope WinGet plans

## Status

Proposed and verified as a draft implementation. Windows process-tree containment is implemented and exercised by native tests on the Issue #8 branch. Production merge remains blocked by Issue #9 recovery reconciliation and Issue #10 installed-state/application-health verification.

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

## Failure and recovery boundary

Approval moves a plan to `APPROVED_AWAITING_EXECUTION`; the legacy
`APPROVED_EXECUTION_DISABLED` state is never executable. The one-time arm is consumed before
process launch and the plan enters `EXECUTING`, preventing silent replay.

Closing the final Job Object handle terminates contained descendants. This proves process-tree
termination after abrupt host exit, but it does not reconcile the persisted `EXECUTING` database
state, partial files, services, registry entries, PATH changes, or package registrations after a
daemon crash. Restart-time execution journaling and residual-state reconciliation remain Issue #9.
Locale-stable installed-state and application-health verification remain Issue #10.

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

## Merge gate

PR #7 remains Draft until all of the following are true:

- Issue #8 final-SHA CI and repeated native process-tree evidence are complete;
- Issue #9 proves crash-safe restart reconciliation and residual-state reporting;
- Issue #10 defines a locale-stable installed-state verdict and package-health boundary;
- the final branch passes Rust formatting, Clippy with warnings denied, workspace tests, JSON
  schema validation, React typecheck/build, native Windows process tests, and native Windows Tauri
  compilation.
