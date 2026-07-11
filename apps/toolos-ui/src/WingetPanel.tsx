import { useState } from "react";
import {
  api,
  type ApprovalAcknowledgements,
  type CommandPreview,
  type PackageScope,
  type WingetActionPlan,
  type WingetPackageSelector,
  type WingetResolutionReport,
} from "./api";
import "./winget.css";
import "./action-plan.css";

type Props = {
  disabled?: boolean;
  onEvidence: () => Promise<void>;
};

type PanelState = "idle" | "loading" | "ready" | "error";

const emptyAcknowledgements: ApprovalAcknowledgements = {
  reviewed_exact_identity: false,
  accepts_declared_write_scope: false,
  understands_no_automatic_rollback: false,
};

export function WingetPanel({ disabled = false, onEvidence }: Props) {
  const [packageId, setPackageId] = useState("Git.Git");
  const [source, setSource] = useState("winget");
  const [version, setVersion] = useState("");
  const [scope, setScope] = useState<PackageScope | "">("");
  const [architecture, setArchitecture] = useState("");
  const [result, setResult] = useState<WingetResolutionReport | null>(null);
  const [plan, setPlan] = useState<WingetActionPlan | null>(null);
  const [confirmationPhrase, setConfirmationPhrase] = useState("");
  const [acknowledgements, setAcknowledgements] = useState(
    emptyAcknowledgements,
  );
  const [state, setState] = useState<PanelState>("idle");
  const [message, setMessage] = useState(
    "Resolve one exact package identity before creating an action plan.",
  );

  const selector = (): WingetPackageSelector => ({
    package_id: packageId.trim(),
    source: source.trim(),
    version: nullable(version),
    scope: scope || null,
    architecture: nullable(architecture),
  });

  const validateSelector = (value: WingetPackageSelector) => {
    if (!value.package_id || !value.source) {
      setState("error");
      setMessage("Package ID and source are required.");
      return false;
    }
    return true;
  };

  const resolve = async () => {
    const value = selector();
    if (!validateSelector(value)) return;

    setState("loading");
    setMessage("Running a non-interactive exact WinGet metadata query…");
    try {
      const response = await api.resolveWinget(value);
      setResult(response.snapshot);
      resetPlan();
      await onEvidence();
      setState("ready");
      setMessage(response.snapshot.single_safest_next_action);
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  };

  const createPlan = async (kind: "install" | "uninstall") => {
    const value = selector();
    if (!validateSelector(value)) return;

    setState("loading");
    setMessage(
      `Refreshing exact package identity and creating an expiring ${kind} plan…`,
    );
    try {
      const response =
        kind === "install"
          ? await api.planWingetInstall(value)
          : await api.planWingetUninstall(value);
      setResult(response.plan.package_resolution);
      setPlan(response.plan);
      setConfirmationPhrase("");
      setAcknowledgements(emptyAcknowledgements);
      await onEvidence();
      setState("ready");
      setMessage(
        "Plan created. Review the immutable command, recovery gaps, and exact approval phrase. Approval will not execute it.",
      );
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  };

  const approve = async () => {
    if (!plan) return;
    setState("loading");
    setMessage("Applying one-time approval to the stored action plan…");
    try {
      const response = await api.approveAction(
        plan.id,
        confirmationPhrase,
        acknowledgements,
      );
      setPlan(response.plan);
      await onEvidence();
      setState("ready");
      setMessage(response.single_safest_next_action);
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  };

  const reject = async () => {
    if (!plan) return;
    setState("loading");
    setMessage("Rejecting the stored plan…");
    try {
      const rejected = await api.rejectAction(plan.id);
      setPlan(rejected);
      setState("ready");
      setMessage("Plan rejected. No command was executed.");
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  };

  const resetPlan = () => {
    setPlan(null);
    setConfirmationPhrase("");
    setAcknowledgements(emptyAcknowledgements);
  };

  return (
    <section id="winget" className="panel action-panel winget-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Managed machine · WinGet</p>
          <h2>Resolve, plan, and approve without executing</h2>
          <p>
            Verifies one package ID against one source, persists an immutable
            time-limited action plan, and records explicit approval. Installation and
            uninstallation remain technically blocked.
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

          {result.status === "RESOLVED_EXACT" && !plan ? (
            <div className="plan-actions">
              <button type="button" onClick={() => void createPlan("install")}>
                Create install plan
              </button>
              <button
                className="secondary"
                type="button"
                onClick={() => void createPlan("uninstall")}
              >
                Create uninstall plan
              </button>
            </div>
          ) : null}

          {plan ? (
            <ActionPlanCard
              plan={plan}
              confirmationPhrase={confirmationPhrase}
              acknowledgements={acknowledgements}
              disabled={disabled || state === "loading"}
              onPhrase={setConfirmationPhrase}
              onAcknowledgements={setAcknowledgements}
              onApprove={() => void approve()}
              onReject={() => void reject()}
            />
          ) : null}

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
            <pre>
              {result.identity_evidence?.stdout ||
                result.identity_evidence?.stderr ||
                "No provider output was captured."}
            </pre>
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

function ActionPlanCard({
  plan,
  confirmationPhrase,
  acknowledgements,
  disabled,
  onPhrase,
  onAcknowledgements,
  onApprove,
  onReject,
}: {
  plan: WingetActionPlan;
  confirmationPhrase: string;
  acknowledgements: ApprovalAcknowledgements;
  disabled: boolean;
  onPhrase: (value: string) => void;
  onAcknowledgements: (value: ApprovalAcknowledgements) => void;
  onApprove: () => void;
  onReject: () => void;
}) {
  const waiting = plan.status === "WAITING_APPROVAL";
  const allConfirmed = Object.values(acknowledgements).every(Boolean);
  const phraseMatches = confirmationPhrase === plan.confirmation_phrase;

  return (
    <article className="action-plan-card">
      <div className="action-plan-heading">
        <div>
          <span className={`plan-status ${plan.status.toLowerCase()}`}>
            {planStatusLabel(plan.status)}
          </span>
          <h3>{plan.kind === "WINGET_INSTALL" ? "Install" : "Uninstall"} action plan</h3>
        </div>
        <code>{plan.id}</code>
      </div>

      <dl className="plan-facts">
        <div>
          <dt>Expires</dt>
          <dd>{formatDate(plan.expires_at)}</dd>
        </div>
        <div>
          <dt>Command hash</dt>
          <dd title={plan.command_sha256}>{plan.command_sha256.slice(0, 20)}…</dd>
        </div>
        <div>
          <dt>Execution</dt>
          <dd>{plan.execution_available ? "Available" : "Blocked"}</dd>
        </div>
        <div>
          <dt>Rollback</dt>
          <dd>{plan.rollback.reversibility}</dd>
        </div>
      </dl>

      <CommandCard title="Immutable planned command" command={plan.command} />

      <div className="blocked-gates">
        <strong>Execution gates still missing</strong>
        <ul>
          {plan.blocked_execution_gates.map((gate) => (
            <li key={gate}>{gate}</li>
          ))}
        </ul>
      </div>

      <details className="rollback-details">
        <summary>Show recovery coverage and gaps</summary>
        <div>
          <strong>Covered</strong>
          <ul>
            {plan.rollback.covered_surfaces.map((surface) => (
              <li key={surface}>{surface}</li>
            ))}
          </ul>
          <strong>Not covered</strong>
          <ul>
            {plan.rollback.uncovered_surfaces.map((surface) => (
              <li key={surface}>{surface}</li>
            ))}
          </ul>
          <p>{plan.rollback.single_safest_recovery_action}</p>
        </div>
      </details>

      {waiting ? (
        <div className="approval-form">
          <div className="approval-phrase">
            <span>Type this exact phrase</span>
            <code>{plan.confirmation_phrase}</code>
            <input
              value={confirmationPhrase}
              onChange={(event) => onPhrase(event.target.value)}
              aria-label="Exact approval phrase"
            />
          </div>
          <label>
            <input
              type="checkbox"
              checked={acknowledgements.reviewed_exact_identity}
              onChange={(event) =>
                onAcknowledgements({
                  ...acknowledgements,
                  reviewed_exact_identity: event.target.checked,
                })
              }
            />
            I reviewed the exact package ID, source, version, scope, and command.
          </label>
          <label>
            <input
              type="checkbox"
              checked={acknowledgements.accepts_declared_write_scope}
              onChange={(event) =>
                onAcknowledgements({
                  ...acknowledgements,
                  accepts_declared_write_scope: event.target.checked,
                })
              }
            />
            I accept the declared user-profile or machine write scope.
          </label>
          <label>
            <input
              type="checkbox"
              checked={acknowledgements.understands_no_automatic_rollback}
              onChange={(event) =>
                onAcknowledgements({
                  ...acknowledgements,
                  understands_no_automatic_rollback: event.target.checked,
                })
              }
            />
            I understand that automatic rollback is not implemented or proven.
          </label>
          <div className="approval-buttons">
            <button
              type="button"
              onClick={onApprove}
              disabled={disabled || !phraseMatches || !allConfirmed}
            >
              Approve plan only
            </button>
            <button
              className="secondary"
              type="button"
              onClick={onReject}
              disabled={disabled}
            >
              Reject plan
            </button>
          </div>
          <p className="approval-warning">
            Approval records intent and closes the approval gate. It does not run
            WinGet.
          </p>
        </div>
      ) : (
        <div className="approval-outcome">
          <strong>{planStatusLabel(plan.status)}</strong>
          <span>
            {plan.status === "APPROVED_AWAITING_EXECUTOR"
              ? "Approval is durable, but execution remains unavailable."
              : "No command was executed."}
          </span>
        </div>
      )}
    </article>
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

function planStatusLabel(status: WingetActionPlan["status"]) {
  switch (status) {
    case "WAITING_APPROVAL":
      return "Waiting approval";
    case "APPROVED_AWAITING_EXECUTOR":
      return "Approved · execution blocked";
    case "REJECTED":
      return "Rejected";
    case "EXPIRED":
      return "Expired";
  }
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
