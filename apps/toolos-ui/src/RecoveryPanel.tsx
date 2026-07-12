import { useCallback, useEffect, useState } from "react";
import {
  api,
  type WingetRecoveryCleanupPlan,
  type WingetRecoveryReport,
} from "./api";
import "./recovery-panel.css";

type Props = {
  disabled: boolean;
  onEvidence: () => Promise<void>;
};

type LoadState = "idle" | "loading" | "ready" | "error";

export function RecoveryPanel({ disabled, onEvidence }: Props) {
  const [reports, setReports] = useState<WingetRecoveryReport[]>([]);
  const [state, setState] = useState<LoadState>("idle");
  const [message, setMessage] = useState("No recovery scan has run in this session.");
  const [cleanupPlan, setCleanupPlan] = useState<WingetRecoveryCleanupPlan | null>(null);
  const [confirmation, setConfirmation] = useState("");
  const [approved, setApproved] = useState(false);

  const refresh = useCallback(async () => {
    setState("loading");
    setMessage("Loading persisted recovery reports…");
    try {
      const result = await api.listWingetRecoveryReports();
      setReports(result);
      setState("ready");
      setMessage(
        result.length
          ? `${result.length} recovery report${result.length === 1 ? "" : "s"} loaded.`
          : "No interrupted WinGet execution requires recovery.",
      );
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const createCleanupPlan = async (executionId: string) => {
    setState("loading");
    setMessage("Creating an immutable execution-disabled cleanup plan…");
    try {
      const result = await api.createWingetRecoveryCleanupPlan(executionId);
      setCleanupPlan(result.plan);
      setConfirmation("");
      setApproved(false);
      await onEvidence();
      setState("ready");
      setMessage("Cleanup plan created. No cleanup command was executed.");
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  };

  const approveCleanupPlan = async () => {
    if (!cleanupPlan) return;
    setState("loading");
    setMessage("Recording cleanup intent without enabling execution…");
    try {
      const result = await api.approveWingetRecoveryCleanupPlan(
        cleanupPlan.cleanup_plan_id,
        cleanupPlan.plan_hash,
        confirmation,
      );
      setCleanupPlan(result.plan);
      setApproved(true);
      await onEvidence();
      setState("ready");
      setMessage("Approval recorded. Cleanup execution remains disabled.");
    } catch (error) {
      setState("error");
      setMessage(formatError(error));
    }
  };

  return (
    <section id="recovery" className="panel recovery-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Crash recovery</p>
          <h2>Reconcile interrupted package executions</h2>
          <p>
            ToolOS reads durable execution journals before accepting another machine write.
            Ambiguous states retain the WinGet lock until reviewed.
          </p>
        </div>
        <button
          className="secondary"
          type="button"
          onClick={() => void refresh()}
          disabled={disabled || state === "loading"}
        >
          Refresh recovery
        </button>
      </div>

      <div className={`recovery-message ${state}`} role="status" aria-live="polite">
        {message}
      </div>

      {reports.length ? (
        <div className="recovery-list">
          {reports.map((report) => (
            <article className={report.status.toLowerCase()} key={report.execution_id}>
              <div className="recovery-title">
                <div>
                  <strong>{label(report.status)}</strong>
                  <code>{report.execution_id}</code>
                </div>
                <span>{report.journal_phase}</span>
              </div>

              <dl className="recovery-facts">
                <div>
                  <dt>Lock</dt>
                  <dd>{report.lock_retained ? "Retained" : "Released"}</dd>
                </div>
                <div>
                  <dt>New writes</dt>
                  <dd>{report.mutable_operations_blocked ? "Blocked" : "Allowed"}</dd>
                </div>
                <div>
                  <dt>Observed</dt>
                  <dd>{new Date(report.observed_at).toLocaleString()}</dd>
                </div>
                <div>
                  <dt>Package</dt>
                  <dd>{report.pre_state.selector.package_id}</dd>
                </div>
              </dl>

              {report.residual_diff ? (
                <div className="residual-diff">
                  <strong>Observed residual-state differences</strong>
                  <ul>
                    {report.residual_diff.observed_changes.map((change) => (
                      <li key={change}>{change}</li>
                    ))}
                  </ul>
                </div>
              ) : (
                <p className="muted">No post-state manifest was available.</p>
              )}

              <p className="safest-action">{report.single_safest_next_action}</p>

              {(report.status === "UNKNOWN_REQUIRES_RECOVERY" ||
                report.status === "FAILED_RESIDUALS_PRESENT") &&
              !cleanupPlan ? (
                <button
                  type="button"
                  onClick={() => void createCleanupPlan(report.execution_id)}
                  disabled={disabled || state === "loading"}
                >
                  Create disabled cleanup plan
                </button>
              ) : null}
            </article>
          ))}
        </div>
      ) : (
        <div className="empty-state">No recovery reports are persisted.</div>
      )}

      {cleanupPlan ? (
        <article className="cleanup-plan" aria-label="Recovery cleanup plan">
          <div className="recovery-title">
            <div>
              <strong>Execution-disabled cleanup plan</strong>
              <code>{cleanupPlan.cleanup_plan_id}</code>
            </div>
            <span>{cleanupPlan.status}</span>
          </div>
          <p>
            This plan records review and approval only. ToolOS has no cleanup executor in
            this milestone.
          </p>
          <code className="command-preview">
            {cleanupPlan.uninstall_preview.powershell}
          </code>
          <ul>
            {cleanupPlan.inspection_steps.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ul>

          {cleanupPlan.approval_challenge ? (
            <div className="cleanup-approval">
              <label htmlFor="cleanup-confirmation">Enter the exact approval phrase</label>
              <code>{cleanupPlan.approval_challenge.required_phrase}</code>
              <input
                id="cleanup-confirmation"
                value={confirmation}
                onChange={(event) => setConfirmation(event.target.value)}
                autoComplete="off"
              />
              <button
                type="button"
                onClick={() => void approveCleanupPlan()}
                disabled={
                  disabled ||
                  state === "loading" ||
                  confirmation.trim() !== cleanupPlan.approval_challenge.required_phrase
                }
              >
                Approve intent — execution stays disabled
              </button>
            </div>
          ) : (
            <p className="approved-note">
              {approved ? "Approval recorded. Execution remains disabled." : "Plan is not approvable."}
            </p>
          )}
        </article>
      ) : null}
    </section>
  );
}

function label(status: WingetRecoveryReport["status"]): string {
  switch (status) {
    case "RECOVERED_NO_PROCESS_STARTED":
      return "Recovered: no process started";
    case "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT":
      return "Recovered from persisted provider result";
    case "FAILED_RESIDUALS_PRESENT":
      return "Residuals require review";
    case "UNKNOWN_REQUIRES_RECOVERY":
      return "Unknown state — recovery required";
  }
}

function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
