import { useState } from "react";
import {
  api,
  type ResourceLock,
  type WingetInstallApprovalReceipt,
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
  const [state, setState] = useState<State>("idle");
  const [message, setMessage] = useState(
    "Create a short-lived dry-run plan that binds identity, installed-state evidence, command, approval, and lock.",
  );

  const createPlan = async () => {
    setState("loading");
    setReceipt(null);
    setLock(null);
    setConfirmation("");
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
              <strong>Approved, execution still disabled</strong>
              <span>Approval {receipt.approval_id}</span>
              <span>Local lock held until {formatDate(lock.expires_at)}</span>
              <code>{receipt.plan_hash}</code>
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
