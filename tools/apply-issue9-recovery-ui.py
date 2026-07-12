from pathlib import Path


def patch(path_text: str, replacements: list[tuple[str, str, str]]) -> None:
    path = Path(path_text)
    source = path.read_text(encoding="utf-8")
    for old, new, label in replacements:
        count = source.count(old)
        if count != 1:
            raise RuntimeError(f"{path_text} {label}: expected one match, found {count}")
        source = source.replace(old, new, 1)
    path.write_text(source, encoding="utf-8", newline="\n")


patch(
    "apps/toolos-ui/src/api.ts",
    [
        (
            '''  | "EXECUTION_CANCELLED"
  | "UNKNOWN_REQUIRES_RECOVERY"
''',
            '''  | "EXECUTION_CANCELLED"
  | "RECOVERED_NO_PROCESS_STARTED"
  | "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT"
  | "UNKNOWN_REQUIRES_RECOVERY"
''',
            "recovered plan statuses",
        ),
        (
            '''export type WingetExecutionResult = {
  plan: WingetInstallPlan;
  report: WingetInstallExecutionReport;
  evidence: EvidenceRecord;
};

export type EvidenceRecord = {
''',
            '''export type WingetExecutionResult = {
  plan: WingetInstallPlan;
  report: WingetInstallExecutionReport;
  evidence: EvidenceRecord;
};

export type ExecutionJournalPhase =
  | "PREPARED"
  | "SPAWN_INTENT"
  | "SPAWNED"
  | "PROVIDER_FINISHED"
  | "FINALIZED";

export type RecoveryStatus =
  | "RECOVERED_NO_PROCESS_STARTED"
  | "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT"
  | "FAILED_RESIDUALS_PRESENT"
  | "UNKNOWN_REQUIRES_RECOVERY";

export type WingetResidualStateManifest = {
  selector: WingetPackageSelector;
  captured_at: string;
  provider_id: string;
  provider_version: string | null;
  installed_state: WingetInstalledStateReport;
  path_environment_sha256: string;
  path_entry_count: number;
  observable_surfaces: string[];
  unobserved_surfaces: string[];
};

export type WingetResidualStateDiff = {
  provider_version_changed: boolean;
  path_fingerprint_changed: boolean;
  installed_evidence_changed: boolean;
  definitive_installed_match_before: boolean | null;
  definitive_installed_match_after: boolean | null;
  observed_changes: string[];
  limitations: string[];
};

export type WingetRecoveryReport = {
  recovery_id: string;
  execution_id: string;
  plan_id: string;
  journal_phase: ExecutionJournalPhase;
  status: RecoveryStatus;
  observed_at: string;
  pre_state: WingetResidualStateManifest;
  post_state: WingetResidualStateManifest | null;
  residual_diff: WingetResidualStateDiff | null;
  lock_retained: boolean;
  mutable_operations_blocked: boolean;
  limitations: string[];
  single_safest_next_action: string;
};

export type RecoveryCleanupPlanStatus =
  | "AWAITING_APPROVAL"
  | "APPROVED_EXECUTION_DISABLED"
  | "EXPIRED";

export type WingetRecoveryCleanupPlan = {
  cleanup_plan_id: string;
  recovery_execution_id: string;
  plan_hash: string;
  status: RecoveryCleanupPlanStatus;
  selector: WingetPackageSelector;
  uninstall_preview: CommandPreview;
  created_at: string;
  expires_at: string;
  approval_allowed: boolean;
  approval_challenge: ApprovalChallenge | null;
  execution_enabled: false;
  inspection_steps: string[];
  limitations: string[];
  single_safest_next_action: string;
};

export type WingetRecoveryCleanupApprovalReceipt = {
  approval_id: string;
  cleanup_plan_id: string;
  recovery_execution_id: string;
  plan_hash: string;
  approved_at: string;
  status: "APPROVED_EXECUTION_DISABLED";
  execution_enabled: false;
  limitations: string[];
};

export type EvidenceRecord = {
''',
            "recovery API types",
        ),
        (
            '''  wingetInstallLock: () =>
    daemonRequest<{
      resource_key: string;
      active: boolean;
      lock: ResourceLock | null;
    }>("winget.install.lock"),
  listEvidence: (limit = 20) =>
''',
            '''  wingetInstallLock: () =>
    daemonRequest<{
      resource_key: string;
      active: boolean;
      lock: ResourceLock | null;
    }>("winget.install.lock"),
  listWingetRecoveryReports: () =>
    daemonRequest<WingetRecoveryReport[]>("winget.recovery.list"),
  getWingetRecoveryReport: (executionId: string) =>
    daemonRequest<WingetRecoveryReport>("winget.recovery.get", {
      execution_id: executionId,
    }),
  createWingetRecoveryCleanupPlan: (executionId: string) =>
    daemonRequest<{ plan: WingetRecoveryCleanupPlan; evidence: EvidenceRecord }>(
      "winget.recovery.cleanup.plan",
      { execution_id: executionId },
    ),
  approveWingetRecoveryCleanupPlan: (
    cleanupPlanId: string,
    planHash: string,
    confirmation: string,
  ) =>
    daemonRequest<{
      plan: WingetRecoveryCleanupPlan;
      receipt: WingetRecoveryCleanupApprovalReceipt;
      evidence: EvidenceRecord;
    }>("winget.recovery.cleanup.approve", {
      cleanup_plan_id: cleanupPlanId,
      plan_hash: planHash,
      confirmation,
    }),
  listEvidence: (limit = 20) =>
''',
            "recovery API methods",
        ),
    ],
)

patch(
    "apps/toolos-ui/src/App.tsx",
    [
        (
            'import { WingetPanel } from "./WingetPanel";\n',
            'import { RecoveryPanel } from "./RecoveryPanel";\nimport { WingetPanel } from "./WingetPanel";\n',
            "recovery panel import",
        ),
        (
            '''          <a href="#winget">WinGet</a>
          <a href="#evidence">Evidence</a>
''',
            '''          <a href="#winget">WinGet</a>
          <a href="#recovery">Recovery</a>
          <a href="#evidence">Evidence</a>
''',
            "recovery navigation",
        ),
        (
            '''          <strong>Planning and local approval only</strong>
          <p>No installs, extraction, deletes, agreement acceptance, elevation, credentials, billing, or repository scripts.</p>
''',
            '''          <strong>Governed evidence-gated writes</strong>
          <p>Only exact pinned user-scope WinGet execution is enabled. Recovery cleanup remains approval-only and execution-disabled.</p>
''',
            "current safety boundary",
        ),
        (
            '''        <WingetPanel
          disabled={status === "loading"}
          onEvidence={refreshEvidence}
        />

        <section id="evidence" className="panel">
''',
            '''        <WingetPanel
          disabled={status === "loading"}
          onEvidence={refreshEvidence}
        />

        <RecoveryPanel
          disabled={status === "loading"}
          onEvidence={refreshEvidence}
        />

        <section id="evidence" className="panel">
''',
            "recovery panel placement",
        ),
    ],
)

patch(
    ".github/workflows/verify.yml",
    [
        (
            '''      - run: python -m json.tool schemas/winget-install-execution.schema.json > /dev/null
''',
            '''      - run: python -m json.tool schemas/winget-install-execution.schema.json > /dev/null
      - run: python -m json.tool schemas/winget-recovery-report.schema.json > /dev/null
      - run: python -m json.tool schemas/winget-recovery-cleanup-plan.schema.json > /dev/null
''',
            "recovery schema validation",
        )
    ],
)
