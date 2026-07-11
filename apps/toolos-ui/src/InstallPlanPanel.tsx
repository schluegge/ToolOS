import { useState } from "react";
import {
  api,
  type ResourceLock,
  type WingetInstallApprovalReceipt,
  type WingetInstallExecutionReport,
  type WingetInstallPlan,
  type WingetPackageSelector,
} from "./api";
import "./install-plan.css";

type Props = {
  selector: WingetPackageSelector;
  disabled?: boolean;
  onEvidence: () => Promise<void>;
};

type State = "idle" | "loading" | "ready" | "error";

export function InstallPlanPanel({ selector, disabled = false, onEvidence }: Props) {
  const [plan, setPlan] = useState<WingetInstallPlan | null>(null);
  const [receipt, setReceipt] = useState<WingetInstallApprovalReceipt | null>(null);
  const [lock, setLock] = useState<ResourceLock | null>(null);
  const [confirmation, setConfirmation] = useState("");
  const [executionConfirmation, setExecutionConfirmation] = useState("");
  const [execution, setExecution] = useState<WingetInstallExecutionReport | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [state, setState] = useState<State>("idle");
  const [message, setMessage] = useState(
    "Create a short-lived dry-run plan that binds identity, installed-state evidence, command, approval, and lock.",
  );

  const createPlan = async () => {
    setState("loading");
    setReceipt(null);
    setLock(null);
    setConfirmation("");
    setExecutionConfirmation("");
    setExecution(null);
    setCancelRequested(false);
    setMessage("Resolving identity and installed state, then hashing an immutable plan…");
    try {
      const result = await api.createWingetInstallPlan(selector);
      setPlan(result.plan);
      await onEvidence();
      setState("ready");
      setMessage(result.plan.single_safest_next_action);
    } catch (error) {
      setState("error");
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const approve = async () => {
    if (!plan) return;
    setState("loading");
    setMessage("Validating the exact plan hash and reserving the local WinGet lock…");
    try {
      const result = await api.approveWingetInstallPlan(
        plan.plan_id,
        plan.plan_hash,
        confirmation,
      );
      setPlan(result.plan);
      setReceipt(result.receipt);
      setLock(result.lock);
      await onEvidence();
      setState("ready");
      setMessage(result.plan.single_safest_next_action);
    } catch (error) {
      setState("error");
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const execute = async () => {
    if (!plan || !receipt) return;
    setState("loading");
    setCancelRequested(false);
    setMessage("Revalidating the pinned plan, consuming the one-time receipt, and invoking WinGet inside a Windows Job Object…");
    try {
      const result = await api.executeWingetInstallPlan(
        plan.plan_id,
        receipt.approval_id,
        executionConfirmation,
      );
      setPlan(result.plan);
      setExecution(result.report);
      setCancelRequested(false);
      if (result.report.status !== "UNKNOWN_REQUIRES_RECOVERY") {
        setLock(null);
      }
      await onEvidence();
      setState("ready");
      setMessage(result.report.single_safest_next_action);
    } catch (error) {
      setCancelRequested(false);
      setState("error");
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const cancelExecution = async () => {
    if (!plan) return;
    setCancelRequested(true);
    try {
      const result = await api.cancelWingetInstallPlan(plan.plan_id);
      if (result.cancel_requested) {
        setMessage(
          "Cancellation requested. ToolOS is terminating the complete Windows Job Object process tree and will report whether termination was confirmed.",
        );
      } else {
        setCancelRequested(false);
        setMessage(
          result.active
            ? "A cancellation request already exists for this execution."
            : "No active governed execution was found for this plan.",
        );
      }
    } catch (error) {
      setCancelRequested(false);
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const copyExecutionPhrase = async () => {
    if (receipt?.execution_confirmation) {
      await navigator.clipboard.writeText(receipt.execution_confirmation);
    }
  };

  const copyPhrase = async () => {
    const phrase = plan?.approval_challenge?.required_phrase;
    if (phrase) await navigator.clipboard.writeText(phrase);
  };

  return (
    <section className="install-plan" aria-label="Governed installation plan">
      <div className="install-plan-heading">
        <div>
          <p className="eyebrow">Governed dry run</p>
          <h3>Build an immutable install plan</h3>
          <p>
            This writes only ToolOS metadata. It never invokes <code>winget install</code>,
            accepts agreements, requests elevation, or bypasses hashes.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void createPlan()}
          disabled={disabled || state === "loading"}
        >
          Create governed plan
        </button>
      </div>

      <div className={`install-plan-message ${state}`} role="status" aria-live="polite">
        {message}
      </div>

      {plan ? (
        <div className="install-plan-body">
          <div className="plan-facts">
            <Fact label="Status" value={plan.status} />
            <Fact label="Plan ID" value={plan.plan_id} mono />
            <Fact label="Plan hash" value={plan.plan_hash} mono />
            <Fact label="Expires" value={formatDate(plan.expires_at)} />
            <Fact label="Lock" value={plan.lock_key} mono />
            <Fact label="Execution" value={plan.execution_enabled ? "Enabled" : "Disabled"} />
          </div>

          <article className="plan-command">
            <strong>Immutable install command</strong>
            <code>{plan.install_preview.powershell}</code>
            <span>{plan.install_preview.blast_radius}</span>
          </article>

          {plan.blockers.length ? (
            <PlanList title="Blocking conditions" items={plan.blockers} tone="blocked" />
          ) : null}
          <PlanList
            title="Required before future execution"
            items={plan.pre_execution_requirements}
          />
          <PlanList title="Verification contract" items={plan.verification} />
          <PlanList title="Rollback boundary" items={plan.rollback} />

          {plan.approval_challenge ? (
            <div className="approval-box">
              <div>
                <strong>Short-lived approval phrase</strong>
                <span>Valid until {formatDate(plan.approval_challenge.expires_at)}</span>
              </div>
              <code>{plan.approval_challenge.required_phrase}</code>
              <div className="approval-controls">
                <button className="secondary" type="button" onClick={() => void copyPhrase()}>
                  Copy phrase
                </button>
                <input
                  value={confirmation}
                  onChange={(event) => setConfirmation(event.target.value)}
                  placeholder="Paste the exact phrase"
                  aria-label="Approval phrase"
                />
                <button
                  type="button"
                  onClick={() => void approve()}
                  disabled={disabled || state === "loading" || !confirmation.trim()}
                >
                  Approve plan only
                </button>
              </div>
            </div>
          ) : null}

          {receipt && lock ? (
            <div className="approval-receipt">
              <strong>Approved; a separate exact execution phrase is still required</strong>
              <span>Approval {receipt.approval_id}</span>
              <span>Local lock held until {formatDate(lock.expires_at)}</span>
              <code>{receipt.plan_hash}</code>
            </div>
          ) : null}

          {receipt && lock ? (
            <div className="execution-box">
              <div>
                <strong>Execute the pinned user-scope plan</strong>
                <span>
                  Real machine mutation. Requires explicit version, architecture, user scope,
                  fresh provider evidence, the active lock, and this one-time receipt.
                </span>
              </div>
              <code>{receipt.execution_confirmation}</code>
              <div className="approval-controls">
                <button
                  className="secondary"
                  type="button"
                  onClick={() => void copyExecutionPhrase()}
                >
                  Copy execution phrase
                </button>
                <input
                  value={executionConfirmation}
                  onChange={(event) => setExecutionConfirmation(event.target.value)}
                  placeholder="Paste the exact execution phrase"
                  aria-label="Execution phrase"
                />
                <button
                  type="button"
                  onClick={() => void execute()}
                  disabled={
                    disabled ||
                    state === "loading" ||
                    !executionConfirmation.trim() ||
                    !plan.execution_enabled ||
                    !receipt.execution_enabled ||
                    plan.selector.scope !== "user" ||
                    !plan.selector.version ||
                    !plan.selector.architecture
                  }
                >
                  Execute approved install
                </button>
                {state === "loading" ? (
                  <button
                    className="secondary cancel-execution"
                    type="button"
                    onClick={() => void cancelExecution()}
                    disabled={cancelRequested}
                  >
                    {cancelRequested ? "Cancellation requested" : "Cancel process tree"}
                  </button>
                ) : null}
              </div>
            </div>
          ) : null}

          {execution ? (
            <div
              className={`execution-result ${
                execution.status === "UNKNOWN_REQUIRES_RECOVERY" ? "unknown" : ""
              }`}
            >
              <strong>{execution.status}</strong>
              <span>{execution.verification_claim}</span>
              <span>Exit code: {execution.process_evidence.exit_code ?? "Unavailable"}</span>
              <span>Duration: {execution.process_evidence.duration_ms} ms</span>
              <span>Containment: {execution.containment.method}</span>
              <span>Root PID: {execution.containment.root_process_id ?? "Unavailable"}</span>
              <span>Termination reason: {execution.containment.termination_reason}</span>
              <span>
                Process tree terminated: {execution.containment.termination_confirmed ? "Yes" : "No"}
              </span>
              <span>
                Active job processes after: {execution.containment.active_processes_after ?? "Unknown"}
              </span>
              <pre>
                {execution.process_evidence.stdout ||
                  execution.process_evidence.stderr ||
                  "No provider output was captured."}
              </pre>
            </div>
          ) : null}

          <PlanList title="Known limitations" items={plan.limitations} tone="muted" />
        </div>
      ) : null}
    </section>
  );
}

function Fact({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <span>{label}</span>
      {mono ? <code>{value}</code> : <strong>{value}</strong>}
    </div>
  );
}

function PlanList({
  title,
  items,
  tone = "normal",
}: {
  title: string;
  items: string[];
  tone?: "normal" | "blocked" | "muted";
}) {
  return (
    <article className={`plan-list ${tone}`}>
      <strong>{title}</strong>
      <ul>
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </article>
  );
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
