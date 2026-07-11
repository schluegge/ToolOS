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
  listEvidence: (limit = 20) =>
    daemonRequest<EvidenceRecord[]>("evidence.list", { limit }),
};
