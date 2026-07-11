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
  listEvidence: (limit = 20) =>
    daemonRequest<EvidenceRecord[]>("evidence.list", { limit }),
};
