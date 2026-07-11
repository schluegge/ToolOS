# ADR-0004: Governed WinGet install plan and short-lived approval

## Status

Accepted for the planning-only milestone.

## Decision

ToolOS may create and approve an immutable WinGet installation plan, but it may not execute that plan in this milestone.

Plan creation performs the existing exact `winget show` resolution and exact installed-state query, then hashes the selector, evidence, disabled command preview, timestamps, and lock key. Plans expire after ten minutes.

Approval requires the exact generated phrase and plan hash. The SQLite transaction validates the unexpired plan, checks the phrase, reserves `package-manager:winget`, updates the plan state, and stores the approval receipt atomically. The approval and lock expire after at most five minutes.

## Safety boundary

- `winget install` is never invoked.
- `--accept-package-agreements` and `--accept-source-agreements` are never added.
- Hash bypasses, dependency skipping, force, overrides, custom installer arguments, and silent elevation are absent.
- The ToolOS lock is local coordination only; it cannot block external WinGet processes.
- Approval authorizes one immutable hash, not a package name or future regenerated command.

## Source evidence

The official Windows Package Manager documentation confirms exact ID/source selection with `winget install --id ... --exact --source ...` and documents version, scope, architecture, interactivity, agreement, hash-bypass, dependency, force, silent, and logging options. It does not expose a true no-side-effect install dry-run in the CLI surface used here. ToolOS therefore defines dry run as generating and persisting the plan without invoking the install command.

## Deferred

A future execution milestone must revalidate identity, installed state, approval expiry, lock ownership, agreement state, elevation requirements, and post-install healthchecks immediately before machine mutation.
