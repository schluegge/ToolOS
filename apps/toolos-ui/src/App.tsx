import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  type EvidenceRecord,
  type HealthReport,
  type MachineSnapshot,
  type ProjectSnapshot,
  type ZipInspectionReport,
} from "./api";
import { RecoveryPanel } from "./RecoveryPanel";
import { WingetPanel } from "./WingetPanel";

type LoadState = "idle" | "loading" | "ready" | "error";

function App() {
  const [health, setHealth] = useState<HealthReport | null>(null);
  const [machine, setMachine] = useState<MachineSnapshot | null>(null);
  const [project, setProject] = useState<ProjectSnapshot | null>(null);
  const [archive, setArchive] = useState<ZipInspectionReport | null>(null);
  const [evidence, setEvidence] = useState<EvidenceRecord[]>([]);
  const [projectPath, setProjectPath] = useState(".");
  const [archivePath, setArchivePath] = useState("");
  const [status, setStatus] = useState<LoadState>("loading");
  const [message, setMessage] = useState(
    "Connecting to the local ToolOS daemon…",
  );

  const refreshEvidence = useCallback(async () => {
    const records = await api.listEvidence(20);
    setEvidence(records);
  }, []);

  const connect = useCallback(async () => {
    setStatus("loading");
    setMessage("Checking daemon, database, and adapter…");
    try {
      const report = await api.health();
      setHealth(report);
      await refreshEvidence();
      setStatus("ready");
      setMessage("Local control plane is reachable.");
    } catch (error) {
      setStatus("error");
      setMessage(formatError(error));
    }
  }, [refreshEvidence]);

  useEffect(() => {
    void connect();
  }, [connect]);

  const scanMachine = async () => {
    setStatus("loading");
    setMessage("Inspecting metadata and PATH-visible executables…");
    try {
      const result = await api.scanMachine();
      setMachine(result.snapshot);
      await refreshEvidence();
      setStatus("ready");
      setMessage("Machine scan completed without launching detected tools.");
    } catch (error) {
      setStatus("error");
      setMessage(formatError(error));
    }
  };

  const inspectProject = async () => {
    const trimmed = projectPath.trim();
    if (!trimmed) {
      setStatus("error");
      setMessage("Enter a project path before inspection.");
      return;
    }
    setStatus("loading");
    setMessage("Inspecting project identity and top-level markers…");
    try {
      const result = await api.inspectProject(trimmed);
      setProject(result.snapshot);
      await refreshEvidence();
      setStatus("ready");
      setMessage("Project inspection completed without running project code.");
    } catch (error) {
      setStatus("error");
      setMessage(formatError(error));
    }
  };

  const inspectArchive = async () => {
    const trimmed = archivePath.trim();
    if (!trimmed) {
      setStatus("error");
      setMessage("Enter a ZIP archive path before inspection.");
      return;
    }
    setStatus("loading");
    setMessage("Inspecting ZIP paths and declared sizes without extraction…");
    try {
      const result = await api.inspectArchive(trimmed);
      setArchive(result.snapshot);
      await refreshEvidence();
      setStatus("ready");
      setMessage(archiveOutcomeMessage(result.snapshot.decision));
    } catch (error) {
      setStatus("error");
      setMessage(formatError(error));
    }
  };

  const foundTools = useMemo(
    () => machine?.tools.filter((tool) => tool.discovered) ?? [],
    [machine],
  );

  const blockerCount = useMemo(
    () => archive?.findings.filter((finding) => finding.severity === "BLOCKER").length ?? 0,
    [archive],
  );

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="ToolOS navigation">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            T
          </span>
          <div>
            <strong>ToolOS</strong>
            <span>local control plane</span>
          </div>
        </div>
        <nav>
          <a className="active" href="#overview">
            Overview
          </a>
          <a href="#machine">Machine</a>
          <a href="#project">Project</a>
          <a href="#archive">Archive</a>
          <a href="#winget">WinGet</a>
          <a href="#recovery">Recovery</a>
          <a href="#evidence">Evidence</a>
        </nav>
        <div className="safety-note">
          <span>Current safety boundary</span>
          <strong>Governed evidence-gated writes</strong>
          <p>Only exact pinned user-scope WinGet execution is enabled. Recovery cleanup remains approval-only and execution-disabled.</p>
        </div>
      </aside>

      <main>
        <header id="overview" className="topbar">
          <div>
            <p className="eyebrow">Milestone C · managed machine</p>
            <h1>Resolve tools and packages exactly before changing the machine.</h1>
          </div>
          <button
            className="secondary"
            type="button"
            onClick={() => void connect()}
            disabled={status === "loading"}
          >
            Reconnect
          </button>
        </header>

        <section
          className={`status-banner ${status}`}
          role="status"
          aria-live="polite"
        >
          <span className="status-dot" aria-hidden="true" />
          <div>
            <strong>{statusLabel(status)}</strong>
            <p>{message}</p>
          </div>
        </section>

        <section className="metric-grid" aria-label="Platform status">
          <Metric
            label="Daemon"
            value={health?.status ?? "Unknown"}
            detail={health?.version ? `v${health.version}` : "Not connected"}
          />
          <Metric
            label="System adapter"
            value={health?.adapter_status ?? "Unknown"}
            detail="Out-of-process JSON-RPC"
          />
          <Metric
            label="Evidence records"
            value={String(evidence.length)}
            detail="Most recent 20"
          />
          <Metric
            label="Archive decision"
            value={archive ? decisionLabel(archive.decision) : "Not inspected"}
            detail={archive ? `${blockerCount} blocker findings` : "Metadata-only"}
          />
        </section>

        <section id="machine" className="panel action-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Machine discovery</p>
              <h2>Find usable tool surfaces</h2>
              <p>
                Checks host metadata and executable resolution on PATH. Presence is
                kept separate from health, configuration, and authentication.
              </p>
            </div>
            <button
              type="button"
              onClick={() => void scanMachine()}
              disabled={status === "loading"}
            >
              Scan this machine
            </button>
          </div>
          {machine ? (
            <div className="scan-result">
              <div className="result-summary">
                <strong>
                  {machine.os} / {machine.architecture}
                </strong>
                <span>
                  {foundTools.length} of {machine.tools.length} candidates discovered
                </span>
              </div>
              <div className="tool-grid">
                {machine.tools.map((tool) => (
                  <article
                    className={`tool-card ${tool.discovered ? "found" : "missing"}`}
                    key={tool.tool_id}
                  >
                    <div>
                      <span className="presence" aria-hidden="true" />
                      <strong>{tool.display_name}</strong>
                    </div>
                    <p>
                      {tool.discovered
                        ? tool.resolved_path
                        : "Not resolvable on PATH"}
                    </p>
                    <small>{tool.execution_environment}</small>
                  </article>
                ))}
              </div>
            </div>
          ) : (
            <EmptyState text="No machine evidence has been collected in this session." />
          )}
        </section>

        <section id="project" className="panel action-panel">
          <div className="panel-heading project-heading">
            <div>
              <p className="eyebrow">Project identity</p>
              <h2>Inspect a selected workspace</h2>
              <p>
                Verifies the path, repository root, stack markers, and agent instruction
                files without executing the project.
              </p>
            </div>
            <div className="path-control">
              <label htmlFor="project-path">Project path</label>
              <div>
                <input
                  id="project-path"
                  value={projectPath}
                  onChange={(event) => setProjectPath(event.target.value)}
                />
                <button
                  type="button"
                  onClick={() => void inspectProject()}
                  disabled={status === "loading"}
                >
                  Inspect project
                </button>
              </div>
            </div>
          </div>
          {project ? (
            <dl className="project-grid">
              <ProjectFact term="Exists" value={project.exists ? "Yes" : "No"} />
              <ProjectFact
                term="Repository root"
                value={project.repository_root ?? "Not detected"}
              />
              <ProjectFact
                term="Stacks"
                value={
                  project.detected_stacks.join(", ") ||
                  "No known top-level markers"
                }
              />
              <ProjectFact
                term="Instruction files"
                value={project.instruction_files.join(", ") || "None detected"}
              />
              <ProjectFact
                term="Markers"
                value={project.markers.join(", ") || "None detected"}
              />
              <ProjectFact
                term="Observed"
                value={formatDate(project.observed_at)}
              />
            </dl>
          ) : (
            <EmptyState text="Select a path to establish project identity before future actions." />
          )}
        </section>

        <section id="archive" className="panel action-panel">
          <div className="panel-heading project-heading">
            <div>
              <p className="eyebrow">Archive safety</p>
              <h2>Inspect a ZIP before extraction</h2>
              <p>
                Reads central-directory metadata and checks traversal, Windows path
                collisions, symlinks, declared sizes, and compression ratios. No entry is
                extracted or executed.
              </p>
            </div>
            <div className="path-control">
              <label htmlFor="archive-path">ZIP archive path</label>
              <div>
                <input
                  id="archive-path"
                  value={archivePath}
                  placeholder="C:\\path\\archive.zip"
                  onChange={(event) => setArchivePath(event.target.value)}
                />
                <button
                  type="button"
                  onClick={() => void inspectArchive()}
                  disabled={status === "loading"}
                >
                  Inspect ZIP
                </button>
              </div>
            </div>
          </div>
          {archive ? (
            <div className="archive-result">
              <div className="archive-summary">
                <span className={`decision ${archive.decision.toLowerCase()}`}>
                  {decisionLabel(archive.decision)}
                </span>
                <div>
                  <strong>{archive.archive_entries} entries evaluated</strong>
                  <span>
                    {formatBytes(archive.total_compressed_size)} compressed · {" "}
                    {formatBytes(archive.total_uncompressed_size)} declared after extraction
                  </span>
                </div>
              </div>

              {archive.findings.length ? (
                <div className="finding-list" aria-label="Archive findings">
                  {archive.findings.map((finding, index) => (
                    <article
                      className={finding.severity.toLowerCase()}
                      key={`${finding.code}-${finding.entry_index ?? "archive"}-${index}`}
                    >
                      <div>
                        <strong>{finding.code}</strong>
                        <span>{finding.severity}</span>
                      </div>
                      <p>{finding.message}</p>
                      {finding.entry_name ? <code>{finding.entry_name}</code> : null}
                    </article>
                  ))}
                </div>
              ) : (
                <EmptyState text="No structural blockers or review findings were detected." />
              )}

              <details className="archive-details">
                <summary>Show inspected entry metadata</summary>
                <div className="entry-list">
                  {archive.entries.map((entry) => (
                    <article key={`${entry.index}-${entry.name}`}>
                      <strong>{entry.name}</strong>
                      <span>
                        {entry.entry_kind} · {formatBytes(entry.uncompressed_size)} · {" "}
                        {entry.compression}
                      </span>
                    </article>
                  ))}
                </div>
              </details>

              <ul className="limitation-list">
                {archive.limitations.map((limitation) => (
                  <li key={limitation}>{limitation}</li>
                ))}
              </ul>
            </div>
          ) : (
            <EmptyState text="Select a ZIP file to evaluate its structure before any future extraction workflow." />
          )}
        </section>

        <WingetPanel
          disabled={status === "loading"}
          onEvidence={refreshEvidence}
        />

        <RecoveryPanel
          disabled={status === "loading"}
          onEvidence={refreshEvidence}
        />

        <section id="evidence" className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Structured evidence</p>
              <h2>Recent observations</h2>
              <p>
                Every machine, project, archive, and package claim carries scope, provider,
                timestamp, limitations, and a content hash.
              </p>
            </div>
          </div>
          {evidence.length ? (
            <div className="evidence-list">
              {evidence.map((record) => (
                <article key={record.id}>
                  <div>
                    <strong>{record.claim}</strong>
                    <span>
                      {record.kind} · {formatDate(record.observed_at)}
                    </span>
                  </div>
                  <code title={record.content_sha256}>
                    {record.content_sha256.slice(0, 16)}…
                  </code>
                  <p>{record.scope}</p>
                </article>
              ))}
            </div>
          ) : (
            <EmptyState text="Run a machine, project, archive, or package inspection to create evidence." />
          )}
        </section>
      </main>
    </div>
  );
}

function Metric({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}

function ProjectFact({ term, value }: { term: string; value: string }) {
  return (
    <div>
      <dt>{term}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="empty-state">{text}</div>;
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value < 0) {
    return String(value);
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let amount = value;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount >= 10 || unit === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[unit]}`;
}

function decisionLabel(decision: ZipInspectionReport["decision"]) {
  switch (decision) {
    case "ACCEPT_STRUCTURE":
      return "Structure acceptable";
    case "REVIEW":
      return "Manual review";
    case "BLOCK":
      return "Blocked";
  }
}

function archiveOutcomeMessage(decision: ZipInspectionReport["decision"]) {
  switch (decision) {
    case "ACCEPT_STRUCTURE":
      return "ZIP structure passed the current metadata checks. This is not a content trust verdict.";
    case "REVIEW":
      return "ZIP inspection completed with findings that require review before extraction.";
    case "BLOCK":
      return "ZIP inspection found structural blockers. Extraction should remain blocked.";
  }
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function statusLabel(status: LoadState) {
  switch (status) {
    case "loading":
      return "Working";
    case "ready":
      return "Ready";
    case "error":
      return "Action blocked";
    default:
      return "Idle";
  }
}

export default App;
