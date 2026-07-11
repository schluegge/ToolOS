# ADR-0004: Execute only pinned user-scope WinGet plans

## Status

Accepted for the first executable ToolOS provider slice.

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
  skip, custom installer arguments, override, manifest, header, or reboot allowance.

Machine-scope execution remains blocked. Agreement prompts fail closed rather than being
accepted by ToolOS.

## Evidence and verification boundary

ToolOS persists the exact command, bounded stdout/stderr, exit code, duration, fresh
preflight reports, and a post-install WinGet query. WinGet exit code zero is recorded as
provider success but not as a definitive installed-state or application-health verdict,
because the current stable CLI output used by this slice is locale-dependent.

## Failure and recovery boundary

The one-time approval is consumed before process launch and the plan enters `EXECUTING`.
A daemon crash therefore fails closed and cannot silently replay the install. The local
WinGet lock is extended through the bounded execution window. On timeout, ToolOS does not
claim that every installer child process terminated; the operator must inspect WinGet logs
and running processes before creating a new plan.
