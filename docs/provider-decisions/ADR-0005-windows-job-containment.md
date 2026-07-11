# ADR-0005: Contain mutable Windows provider trees with Job Objects

## Status

Proposed for the governed WinGet execution slice. The implementation is accepted only when the native Windows process-tree tests and the complete repository CI are green.

## Context

Terminating only the immediate WinGet or adapter process is not sufficient. WinGet can launch an installer, and that installer can create additional processes. A timeout, cancellation, daemon failure, or closed IPC connection must not leave descendants running outside ToolOS supervision.

Assigning a process to a Job Object after it has started also leaves an escape interval in which it could create an uncontained child.

## Decision

ToolOS isolates the Win32 boundary in the `toolos-windows-job` crate. The rest of the workspace keeps `unsafe_code = "forbid"`.

For every mutable Windows provider invocation ToolOS:

1. creates a Job Object;
2. enables `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`;
3. creates stdin, stdout, and stderr pipes with only the child ends inheritable;
4. supplies those handles through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`;
5. supplies the Job Object through `PROC_THREAD_ATTRIBUTE_JOB_LIST` so the root process is assigned during `CreateProcessW`, not after launch;
6. captures bounded transport output while the process tree runs;
7. distinguishes normal exit, timeout, explicit cancellation, daemon shutdown, descendants outliving the root, and containment failure;
8. uses `TerminateJobObject` for timeout or cancellation;
9. queries `JobObjectBasicAccountingInformation` and requires `ActiveProcesses == 0` before claiming that termination completed.

Children inherit membership in the Job Object. ToolOS does not enable silent breakaway. Closing the final Job Object handle is a final kill-on-close safety net.

## Evidence contract

Each execution report records:

- containment method;
- root process ID;
- whether kill-on-close was configured;
- whether assignment occurred at process creation;
- whether inherited handles were restricted;
- termination reason;
- whether termination was requested and confirmed;
- active process count after termination;
- whether descendants outlived the root;
- a bounded diagnostic detail when containment failed.

A provider exit code is interpreted only when the process tree is confirmed empty. Timeout and cancellation become terminal states only when the process tree is confirmed empty. Every other case becomes `UNKNOWN_REQUIRES_RECOVERY`; ToolOS skips the post-install query and retains the package-manager lock.

## Native verification

The Windows CI fixture starts a parent process that launches a nested child. Tests prove that:

- explicit cancellation terminates the nested child;
- timeout terminates the nested child;
- daemon-shutdown cancellation is represented separately and terminates the nested child;
- dropping the final Job Object owner prevents the nested child from surviving.

The test fails when the nested child survives long enough to write its survivor marker.

## Remaining boundary

Process-tree containment does not prove rollback or a clean machine state. Partial files, services, registry entries, package registrations, or user data may remain after timeout or cancellation. Crash-safe residual-state journaling and restart reconciliation remain tracked by Issue #9. Installed-state and application-health verification remain tracked by Issue #10.
