# ADR-0004: Execute only pinned user-scope WinGet plans

## Status

Proposed and verified as a draft implementation. Production merge is blocked by Issues #8 and #9; complete verification is tracked in Issue #10.

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

Approval moves a plan to `APPROVED_AWAITING_EXECUTION`; the legacy
`APPROVED_EXECUTION_DISABLED` state is never executable. The one-time arm is consumed before
process launch and the plan enters `EXECUTING`, preventing silent replay. Production use is
still blocked until Windows process-tree containment is proven (Issue #8) and restart-time
residual-state reconciliation exists (Issue #9). Locale-stable installed-state and application
health verification remain separate work (Issue #10).
