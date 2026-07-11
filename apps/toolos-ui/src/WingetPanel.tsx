import { useState } from "react";
import {
  api,
  type CommandPreview,
  type PackageScope,
  type WingetPackageSelector,
  type WingetResolutionReport,
} from "./api";
import "./winget.css";

type Props = {
  disabled?: boolean;
  onEvidence: () => Promise<void>;
};

type PanelState = "idle" | "loading" | "ready" | "error";

export function WingetPanel({ disabled = false, onEvidence }: Props) {
  const [packageId, setPackageId] = useState("Git.Git");
  const [source, setSource] = useState("winget");
  const [version, setVersion] = useState("");
  const [scope, setScope] = useState<PackageScope | "">("");
  const [architecture, setArchitecture] = useState("");
  const [result, setResult] = useState<WingetResolutionReport | null>(null);
  const [state, setState] = useState<PanelState>("idle");
  const [message, setMessage] = useState(
    "Resolve one exact package identity before any installation workflow is allowed.",
  );

  const resolve = async () => {
    const selector: WingetPackageSelector = {
      package_id: packageId.trim(),
      source: source.trim(),
      version: nullable(version),
      scope: scope || null,
      architecture: nullable(architecture),
    };

    if (!selector.package_id || !selector.source) {
      setState("error");
      setMessage("Package ID and source are required.");
      return;
    }

    setState("loading");
    setMessage("Running a non-interactive exact WinGet metadata query…");
    try {
      const response = await api.resolveWinget(selector);
      setResult(response.snapshot);
      await onEvidence();
      setState("ready");
      setMessage(response.snapshot.single_safest_next_action);
    } catch (error) {
      setState("error");
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <section id="winget" className="panel action-panel winget-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Managed machine · WinGet</p>
          <h2>Resolve an exact native package</h2>
          <p>
            Verifies one package ID against one source and generates transparent
            install and uninstall commands. Command execution remains disabled.
          </p>
        </div>
      </div>

      <div className="winget-form">
        <label>
          <span>Package ID</span>
          <input
            value={packageId}
            onChange={(event) => setPackageId(event.target.value)}
            placeholder="Git.Git"
          />
        </label>
        <label>
          <span>Source</span>
          <input
            value={source}
            onChange={(event) => setSource(event.target.value)}
            placeholder="winget"
          />
        </label>
        <label>
          <span>Version (optional)</span>
          <input
            value={version}
            onChange={(event) => setVersion(event.target.value)}
            placeholder="Latest when empty"
          />
        </label>
        <label>
          <span>Scope (optional)</span>
          <select
            value={scope}
            onChange={(event) => setScope(event.target.value as PackageScope | "")}
          >
            <option value="">Provider default</option>
            <option value="user">User</option>
            <option value="machine">Machine</option>
          </select>
        </label>
        <label>
          <span>Architecture (optional)</span>
          <input
            value={architecture}
            onChange={(event) => setArchitecture(event.target.value)}
            placeholder="x64"
          />
        </label>
        <button
          type="button"
          onClick={() => void resolve()}
          disabled={disabled || state === "loading"}
        >
          Resolve exact package
        </button>
      </div>

      <div className={`winget-message ${state}`} role="status" aria-live="polite">
        {message}
      </div>

      {result ? (
        <div className="winget-result">
          <div className="winget-resolution-summary">
            <span className={`resolution ${result.status.toLowerCase()}`}>
              {resolutionLabel(result.status)}
            </span>
            <div>
              <strong>
                {result.selector.package_id} · {result.selector.source}
              </strong>
              <span>
                WinGet {result.provider_version ?? "version unavailable"} · observed {" "}
                {formatDate(result.observed_at)}
              </span>
            </div>
          </div>

          <CommandCard title="Identity probe" command={result.identity_probe} />
          <CommandCard title="Install preview" command={result.install_preview} />
          <CommandCard title="Uninstall preview" command={result.uninstall_preview} />

          <details className="provider-evidence">
            <summary>Show raw WinGet evidence</summary>
            <dl>
              <div>
                <dt>Exit code</dt>
                <dd>{result.identity_evidence?.exit_code ?? "Unavailable"}</dd>
              </div>
              <div>
                <dt>Duration</dt>
                <dd>{result.identity_evidence?.duration_ms ?? 0} ms</dd>
              </div>
              <div>
                <dt>Timed out</dt>
                <dd>{result.identity_evidence?.timed_out ? "Yes" : "No"}</dd>
              </div>
            </dl>
            <pre>{
              result.identity_evidence?.stdout ||
              result.identity_evidence?.stderr ||
              "No provider output was captured."
            }</pre>
          </details>

          <ul className="limitation-list">
            {result.limitations.map((limitation) => (
              <li key={limitation}>{limitation}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}

function CommandCard({
  title,
  command,
}: {
  title: string;
  command: CommandPreview;
}) {
  const [copyState, setCopyState] = useState("Copy");

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command.powershell);
      setCopyState("Copied");
      window.setTimeout(() => setCopyState("Copy"), 1500);
    } catch {
      setCopyState("Copy failed");
    }
  };

  return (
    <article className="command-card">
      <div className="command-card-heading">
        <div>
          <strong>{title}</strong>
          <span>{command.blast_radius}</span>
        </div>
        <button className="secondary" type="button" onClick={() => void copy()}>
          {copyState}
        </button>
      </div>
      <code>{command.powershell}</code>
      <div className="execution-lock">
        <strong>Execution disabled</strong>
        <span>
          {command.execution_enabled
            ? "Unexpected enabled state"
            : "Preview only; ToolOS cannot run this command in the current milestone."}
        </span>
      </div>
      {command.expected_side_effects.length ? (
        <ul>
          {command.expected_side_effects.map((effect) => (
            <li key={effect}>{effect}</li>
          ))}
        </ul>
      ) : null}
    </article>
  );
}

function nullable(value: string) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function resolutionLabel(status: WingetResolutionReport["status"]) {
  switch (status) {
    case "RESOLVED_EXACT":
      return "Exact identity resolved";
    case "BLOCKED":
      return "Resolution blocked";
    case "UNAVAILABLE":
      return "WinGet unavailable";
  }
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
