from __future__ import annotations

from pathlib import Path

ROOT = Path.cwd()


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one marker, found {count}: {old[:120]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")


# Domain contract: preserve the legacy disabled state, but never treat it as executable.
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "    ApprovedExecutionDisabled,\n    Executing,\n",
    "    ApprovedExecutionDisabled,\n    ApprovedAwaitingExecution,\n    Executing,\n",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "Review the exact command, raw identity and installed-state evidence, then enter the approval phrase before the plan expires. Approval still cannot execute the installer.",
    "Review the exact command, raw identity and installed-state evidence, then enter the approval phrase before the plan expires. Approval arms only a separate receipt-bound execution step; it does not itself invoke the installer.",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "Approval changes only ToolOS metadata and a local lock; machine execution remains disabled.",
    "Approval changes only ToolOS metadata and a local lock; a second receipt-bound execution phrase is required before machine mutation.",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "        status: InstallPlanStatus::ApprovedExecutionDisabled,\n        execution_enabled: false,\n",
    "        status: InstallPlanStatus::ApprovedAwaitingExecution,\n        execution_enabled: true,\n",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "        assert!(!receipt.execution_enabled);\n",
    "        assert_eq!(receipt.status, InstallPlanStatus::ApprovedAwaitingExecution);\n        assert!(receipt.execution_enabled);\n",
)

# Persistence: only newly armed approvals can transition to EXECUTING.
replace_once(
    "crates/toolos-storage/src/lib.rs",
    '                "APPROVED_EXECUTION_DISABLED",\n',
    '                "APPROVED_AWAITING_EXECUTION",\n',
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    '        if status != "APPROVED_EXECUTION_DISABLED" {\n',
    '        if status != "APPROVED_AWAITING_EXECUTION" {\n',
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    '        assert_eq!(saved.status, "APPROVED_EXECUTION_DISABLED");\n',
    '        assert_eq!(saved.status, "APPROVED_AWAITING_EXECUTION");\n',
)

# Daemon: arm explicitly, consume the arm before launch, and reject legacy receipts.
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "    approved_plan.status = InstallPlanStatus::ApprovedExecutionDisabled;\n    approved_plan.approval_challenge = None;\n",
    "    approved_plan.status = InstallPlanStatus::ApprovedAwaitingExecution;\n    approved_plan.execution_enabled = true;\n    approved_plan.approval_challenge = None;\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "Execution remains disabled until the separate receipt-bound execution phrase is confirmed. Only exact, pinned, user-scope plans can execute.",
    "The plan is armed for one separate receipt-bound execution phrase. Approval itself did not invoke WinGet.",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "Governed WinGet install plan approved while execution remained disabled",
    "Governed WinGet install plan armed for one separate receipt-bound execution step",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    '            "execution_enabled": false\n',
    '            "execution_enabled": true\n',
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "    plan.status = InstallPlanStatus::Executing;\n",
    "    plan.status = InstallPlanStatus::Executing;\n    plan.execution_enabled = false;\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "    if plan.status != InstallPlanStatus::ApprovedExecutionDisabled {\n        return Err(anyhow!(\n            \"install plan is not approved for a separate execution step\"\n        ));\n    }\n",
    "    if plan.status != InstallPlanStatus::ApprovedAwaitingExecution\n        || receipt.status != InstallPlanStatus::ApprovedAwaitingExecution\n        || !plan.execution_enabled\n        || !receipt.execution_enabled\n    {\n        return Err(anyhow!(\n            \"install plan and receipt are not armed for a separate execution step\"\n        ));\n    }\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    '            "status": "IMPLEMENTED_EXECUTION_DISABLED"\n',
    '            "status": "IMPLEMENTED_ARMS_SEPARATE_EXECUTION"\n',
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    '        InstallPlanStatus::ApprovedExecutionDisabled => "APPROVED_EXECUTION_DISABLED",\n        InstallPlanStatus::Executing => "EXECUTING",\n',
    '        InstallPlanStatus::ApprovedExecutionDisabled => "APPROVED_EXECUTION_DISABLED",\n        InstallPlanStatus::ApprovedAwaitingExecution => "APPROVED_AWAITING_EXECUTION",\n        InstallPlanStatus::Executing => "EXECUTING",\n',
)

# UI: never show an executable control unless both plan and receipt are armed.
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    "                    !executionConfirmation.trim() ||\n                    plan.selector.scope !== \"user\" ||\n",
    "                    !executionConfirmation.trim() ||\n                    !plan.execution_enabled ||\n                    !receipt.execution_enabled ||\n                    plan.selector.scope !== \"user\" ||\n",
)

# Documentation must describe the actual draft contract and the merge blockers.
replace_once(
    "README.md",
    "- `toolos-winget-adapter`: out-of-process provider for exact WinGet package resolution and disabled command previews.\n",
    "- `toolos-winget-adapter`: out-of-process provider for exact WinGet resolution, disabled previews, and the draft pinned user-scope execution slice.\n",
)
replace_once(
    "README.md",
    "The preview commands are intentionally disabled. ToolOS does not accept package/source agreements, bypass hashes, skip dependencies, request elevation, install software, or uninstall software in this slice.\n",
    "Preview commands remain intentionally non-executable artifacts. The draft execution path can invoke only an exact version- and architecture-pinned user-scope install after fresh revalidation and two separate short-lived confirmations; it never auto-accepts agreements, bypasses hashes, skips dependencies, requests elevation, or enables uninstall. Production merge remains blocked by Issues #8 and #9.\n",
)
replace_once(
    "docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md",
    "Accepted for the first executable ToolOS provider slice.\n",
    "Proposed and verified as a draft implementation. Production merge is blocked by Issues #8 and #9; complete verification is tracked in Issue #10.\n",
)
replace_once(
    "docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md",
    "The one-time approval is consumed before process launch and the plan enters `EXECUTING`.\nA daemon crash therefore fails closed and cannot silently replay the install. The local\nWinGet lock is extended through the bounded execution window. On timeout, ToolOS does not\nclaim that every installer child process terminated; the operator must inspect WinGet logs\nand running processes before creating a new plan.\n",
    "Approval moves a plan to `APPROVED_AWAITING_EXECUTION`; the legacy\n`APPROVED_EXECUTION_DISABLED` state is never executable. The one-time arm is consumed before\nprocess launch and the plan enters `EXECUTING`, preventing silent replay. Production use is\nstill blocked until Windows process-tree containment is proven (Issue #8) and restart-time\nresidual-state reconciliation exists (Issue #9). Locale-stable installed-state and application\nhealth verification remain separate work (Issue #10).\n",
)
