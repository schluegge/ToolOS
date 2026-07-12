import { invoke } from "@tauri-apps/api/core";

export type HealthReport = {
  service: string;
  version: string;
  status: string;
  started_at: string;
  checked_at: string;
  database_path: string;
  adapter_status: string;
};

export type ToolObservation = {
  tool_id: string;
  display_name: string;
  executable: string;
  discovered: boolean;
  resolved_path: string | null;
  execution_environment: string;
  limitations: string[];
};

export type MachineSnapshot = {
  os: string;
  architecture: string;
  hostname_token: string | null;
  environments: string[];
  tools: ToolObservation[];
  privacy_mode: string;
  observed_at: string;
};

export type ProjectSnapshot = {
  requested_path: string;
  canonical_path: string | null;
  exists: boolean;
  is_directory: boolean;
  repository_root: string | null;
  markers: string[];
  detected_stacks: string[];
  instruction_files: string[];
  limitations: string[];
  observed_at: string;
};

export type ArchiveFinding = {
  severity: "REVIEW" | "BLOCKER";
  code: string;
  entry_index: number | null;
  entry_name: string | null;
  message: string;
};

export type ZipEntryObservation = {
  index: number;
  name: string;
  enclosed_path: string | null;
  entry_kind: string;
  compression: string;
  compressed_size: number;
  uncompressed_size: number;
  expansion_ratio: number | null;
  encrypted: boolean;
  unix_mode: number | null;
  crc32: number;
};

export type ZipInspectionReport = {
  requested_path: string;
  canonical_path: string;
  archive_file_size: number;
  archive_entries: number;
  reported_entries: number;
  total_compressed_size: number;
  total_uncompressed_size: number;
  decision: "ACCEPT_STRUCTURE" | "REVIEW" | "BLOCK";
  findings: ArchiveFinding[];
  entries: ZipEntryObservation[];
  privacy_mode: string;
  observed_at: string;
  limitations: string[];
};

export type PackageScope = "user" | "machine";

export type WingetPackageSelector = {
  package_id: string;
  source: string;
  version: string | null;
  scope: PackageScope | null;
  architecture: string | null;
};

export type CommandPreview = {
  executable: string;
  args: string[];
  powershell: string;
  working_directory: string | null;
  environment_changes: string[];
  blast_radius: string;
  execution_enabled: boolean;
  expected_side_effects: string[];
  approval_requirements: string[];
};

export type ProcessEvidence = {
  executable: string;
  args: string[];
  exit_code: number | null;
  stdout: string;
  stderr: string;
  timed_out: boolean;
  duration_ms: number;
};

export type ProcessStopReason =
  | "EXITED"
  | "TIMED_OUT"
  | "CANCELLED"
  | "DAEMON_SHUTDOWN"
  | "CONTAINMENT_FAILED";

export type ProcessContainmentEvidence = {
  method: string;
  root_pid: number | null;
  stop_reason: ProcessStopReason;
  active_processes_after_cleanup: number | null;
  descendants_terminated: boolean | null;
  containment_confirmed: boolean;
  detail: string;
};

export type WingetResolutionReport = {
  provider_id: string;
  provider_version: string | null;
  status: "RESOLVED_EXACT" | "BLOCKED" | "UNAVAILABLE";
  selector: WingetPackageSelector;
  identity_probe: CommandPreview;
  identity_evidence: ProcessEvidence | null;
  install_preview: CommandPreview;
  uninstall_preview: CommandPreview;
  observed_at: string;
  limitations: string[];
  single_safest_next_action: string;
};

export type WingetInstalledStateReport = {
  provider_id: string;
  provider_version: string | null;
  status: "QUERY_COMPLETED" | "BLOCKED" | "UNAVAILABLE";
  selector: WingetPackageSelector;
  installed_probe: CommandPreview;
  installed_evidence: ProcessEvidence | null;
  observed_at: string;
  definitive_installed_match: boolean | null;
  limitations: string[];
  single_safest_next_action: string;
};

export type ApprovalChallenge = {
  required_phrase: string;
  expires_at: string;
};

export type InstallPlanStatus =
  | "AWAITING_APPROVAL"
  | "BLOCKED"
  | "APPROVED_EXECUTION_DISABLED"
  | "APPROVED_AWAITING_EXECUTION"
  | "EXECUTING"
  | "EXECUTION_SUCCEEDED_UNVERIFIED"
  | "EXECUTION_FAILED"
  | "EXECUTION_TIMED_OUT"
  | "EXECUTION_CANCELLED"
  | "UNKNOWN_REQUIRES_RECOVERY"
  | "EXPIRED";

export type WingetInstallPlan = {
  plan_id: string;
  plan_hash: string;
  status: InstallPlanStatus;
  selector: WingetPackageSelector;
  resolution: WingetResolutionReport;
  installed_state: WingetInstalledStateReport;
  install_preview: CommandPreview;
  approval_allowed: boolean;
  approval_challenge: ApprovalChallenge | null;
  lock_key: string;
  created_at: string;
  expires_at: string;
  execution_enabled: boolean;
  blockers: string[];
  pre_execution_requirements: string[];
  verification: string[];
  rollback: string[];
  limitations: string[];
  single_safest_next_action: string;
};

export type WingetInstallApprovalReceipt = {
  approval_id: string;
  plan_id: string;
  plan_hash: string;
  package_id: string;
  approved_at: string;
  expires_at: string;
  lock_key: string;
  lock_expires_at: string;
  status: "APPROVED_AWAITING_EXECUTION";
  execution_enabled: true;
  execution_confirmation: string;
  limitations: string[];
};

export type WingetExecutionStatus =
  | "PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED"
  | "PROVIDER_FAILED"
  | "TIMED_OUT_CONTAINED"
  | "CANCELLED_CONTAINED"
  | "UNKNOWN_REQUIRES_RECOVERY";

export type WingetInstallExecutionReport = {
  execution_id: string;
  plan_id: string;
  approval_id: string;
  plan_hash: string;
  status: WingetExecutionStatus;
  selector: WingetPackageSelector;
  command: CommandPreview;
  started_at: string;
  completed_at: string;
  process_evidence: ProcessEvidence;
  containment_evidence: ProcessContainmentEvidence;
  preflight_resolution: WingetResolutionReport;
  preflight_installed_state: WingetInstalledStateReport;
  post_install_state: WingetInstalledStateReport | null;
  execution_attempted: boolean;
  verification_claim: string;
  limitations: string[];
  single_safest_next_action: string;
};

export type ResourceLock = {
  resource_key: string;
  holder_plan_id: string;
  acquired_at: string;
  expires_at: string;
};

export type WingetApprovalResult = {
  plan: WingetInstallPlan;
  receipt: WingetInstallApprovalReceipt;
  lock: ResourceLock;
  evidence: EvidenceRecord;
};

export type WingetExecutionResult = {
  plan: WingetInstallPlan;
  report: WingetInstallExecutionReport;
  evidence: EvidenceRecord;
};

export type EvidenceRecord = {
  id: string;
  trace_id: string;
  kind: string;
  scope: string;
  claim: string;
  provider: string;
  observed_at: string;
  payload: unknown;
  limitations: string[];
  content_sha256: string;
};

type ObservationResult<T> = {
  snapshot: T;
  evidence: EvidenceRecord;
};

async function daemonRequest<T>(
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  return invoke<T>("daemon_request", { method, params });
}

export const api = {
  health: () => daemonRequest<HealthReport>("daemon.ping"),
  scanMachine: () =>
    daemonRequest<ObservationResult<MachineSnapshot>>("machine.inspect"),
  inspectProject: (path: string) =>
    daemonRequest<ObservationResult<ProjectSnapshot>>("project.inspect", { path }),
  inspectArchive: (path: string) =>
    daemonRequest<ObservationResult<ZipInspectionReport>>("archive.inspect", {
      path,
    }),
  resolveWinget: (selector: WingetPackageSelector) =>
    daemonRequest<ObservationResult<WingetResolutionReport>>(
      "winget.resolve",
      selector,
    ),
  createWingetInstallPlan: (selector: WingetPackageSelector) =>
    daemonRequest<{ plan: WingetInstallPlan; evidence: EvidenceRecord }>(
      "winget.install.plan",
      selector,
    ),
  approveWingetInstallPlan: (
    planId: string,
    planHash: string,
    confirmation: string,
  ) =>
    daemonRequest<WingetApprovalResult>("winget.install.approve", {
      plan_id: planId,
      plan_hash: planHash,
      confirmation,
    }),
  executeWingetInstallPlan: (
    executionId: string,
    planId: string,
    approvalId: string,
    confirmation: string,
  ) =>
    daemonRequest<WingetExecutionResult>("winget.install.execute", {
      execution_id: executionId,
      plan_id: planId,
      approval_id: approvalId,
      confirmation,
    }),
  cancelWingetInstallExecution: (executionId: string) =>
    daemonRequest<{
      execution_id: string;
      plan_id: string;
      cancel_requested: boolean;
    }>("winget.install.cancel", {
      execution_id: executionId,
    }),
  getWingetInstallPlan: (planId: string) =>
    daemonRequest<WingetInstallPlan>("winget.install.plan.get", {
      plan_id: planId,
    }),
  wingetInstallLock: () =>
    daemonRequest<{
      resource_key: string;
      active: boolean;
      lock: ResourceLock | null;
    }>("winget.install.lock"),
  listEvidence: (limit = 20) =>
    daemonRequest<EvidenceRecord[]>("evidence.list", { limit }),
};
