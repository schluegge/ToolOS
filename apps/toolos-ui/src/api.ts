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

export type ApprovalAcknowledgements = {
  reviewed_exact_identity: boolean;
  accepts_declared_write_scope: boolean;
  understands_no_automatic_rollback: boolean;
};

export type RollbackManifest = {
  reversibility: string;
  uninstall_preview: CommandPreview;
  covered_surfaces: string[];
  uncovered_surfaces: string[];
  single_safest_recovery_action: string;
};

export type WingetActionPlan = {
  id: string;
  trace_id: string;
  kind: "WINGET_INSTALL" | "WINGET_UNINSTALL";
  status:
    | "WAITING_APPROVAL"
    | "APPROVED_AWAITING_EXECUTOR"
    | "REJECTED"
    | "EXPIRED";
  package_resolution: WingetResolutionReport;
  command: CommandPreview;
  command_sha256: string;
  confirmation_phrase: string;
  created_at: string;
  expires_at: string;
  approved_at: string | null;
  acknowledgements: ApprovalAcknowledgements | null;
  execution_available: boolean;
  blocked_execution_gates: string[];
  rollback: RollbackManifest;
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

type PlanResult = {
  plan: WingetActionPlan;
  resolution_evidence: EvidenceRecord;
  plan_evidence: EvidenceRecord;
};

type ApprovalResult = {
  plan: WingetActionPlan;
  outcome: "APPROVED_NOT_EXECUTED";
  single_safest_next_action: string;
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
  planWingetInstall: (selector: WingetPackageSelector) =>
    daemonRequest<PlanResult>("winget.plan.install", selector),
  planWingetUninstall: (selector: WingetPackageSelector) =>
    daemonRequest<PlanResult>("winget.plan.uninstall", selector),
  approveAction: (
    planId: string,
    confirmationPhrase: string,
    acknowledgements: ApprovalAcknowledgements,
  ) =>
    daemonRequest<ApprovalResult>("actions.approve", {
      plan_id: planId,
      confirmation_phrase: confirmationPhrase,
      acknowledgements,
    }),
  rejectAction: (planId: string) =>
    daemonRequest<WingetActionPlan>("actions.reject", { plan_id: planId }),
  listActions: (limit = 20) =>
    daemonRequest<WingetActionPlan[]>("actions.list", { limit }),
  listEvidence: (limit = 20) =>
    daemonRequest<EvidenceRecord[]>("evidence.list", { limit }),
};
