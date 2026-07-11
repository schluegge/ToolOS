# ADR-0004: Durable approval before WinGet execution

- Status: Accepted
- Date: 2026-07-11
- Milestone: C — Managed machine
- Execution status: blocked

## Context

Exact WinGet package resolution and command previews exist, but an ephemeral button click is not sufficient approval for a write-capable machine action. ToolOS must preserve the exact command, identity evidence, blast radius, recovery limitations, and user decision across UI or daemon restarts. It must also prevent stale, changed, repeated, or partial approvals from becoming execution authorization.

Enabling actual installation now would be unsafe because ToolOS has not yet proven Windows process-tree cancellation, pre-install residual-state capture, or package-specific rollback coverage.

## Decision

ToolOS introduces durable, time-limited WinGet action plans. Planning and approval are implemented; execution remains unavailable.

Each plan:

- refreshes exact WinGet package resolution before creation;
- stores the complete provider resolution report;
- stores one install or uninstall command preview;
- hashes the canonical serialized command with SHA-256;
- receives a random UUID and exact confirmation phrase;
- expires after fifteen minutes;
- starts in `WAITING_APPROVAL`;
- requires three explicit acknowledgements;
- transitions atomically to `APPROVED_AWAITING_EXECUTOR`, `REJECTED`, or `EXPIRED`;
- cannot be approved twice because SQLite updates require the expected prior status;
- always keeps `execution_available` set to `false`;
- records blocked execution gates and rollback coverage gaps.

## Approval requirements

Approval succeeds only when all conditions hold:

1. the plan exists;
2. the plan is still `WAITING_APPROVAL`;
3. the current time is before `expires_at`;
4. the supplied plan UUID matches;
5. the exact confirmation phrase matches;
6. all three acknowledgements are true;
7. the stored command still hashes to the stored SHA-256 value;
8. the SQLite compare-and-swap update finds exactly one row in the expected state.

The acknowledgements are:

- exact package identity and command reviewed;
- declared user-profile or machine write scope accepted;
- absence of proven automatic rollback understood.

## Persistent model

SQLite migration 4 adds `action_plan` with:

- plan UUID;
- trace UUID;
- action kind;
- current state;
- creation, expiry, and update timestamps;
- complete JSON plan record.

The complete record remains the portable source of truth. Indexed columns support state and recency queries without duplicating plan semantics.

## Recovery manifest

Current recovery status is `PARTIAL_UNVERIFIED`.

Covered:

- exact package selector;
- proposed uninstall command;
- command hash;
- provider evidence;
- action and approval events.

Not covered:

- installer-created files and user data;
- caches and shared dependencies;
- services and scheduled tasks;
- PATH and registry changes;
- incomplete or interactive uninstallers;
- elevation boundaries;
- restoration after partial installation;
- Windows process-tree cancellation.

## Consequences

Positive:

- approval is durable, inspectable, time-limited, and bound to one immutable command;
- restart or UI loss does not erase the decision record;
- stale and repeated approval attempts are rejected;
- the UI can show one exact next action and concrete recovery gaps;
- execution remains impossible until missing safety gates are implemented.

Negative:

- an approved plan cannot yet perform useful machine writes;
- fresh package resolution is repeated when creating a plan;
- plans may expire while the user reviews them;
- cancellation and rollback remain the next critical engineering work.

## Next gate

Actual WinGet execution must not be enabled until a Windows process supervisor proves child-process-tree termination, cancellation state transitions, timeout handling, and cleanup against a controlled fake installer. After that, ToolOS must add pre/post package state manifests and package-specific rollback evidence before enabling general machine writes.
