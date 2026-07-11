from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one marker, found {count}: {old[:100]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")


# Keep status classification small and independently testable.
old_status = '''    let status = match &containment.termination_reason {
        ProcessTerminationReason::ProcessExited if containment.process_tree_terminated() => {
            if process_evidence.exit_code == Some(0) {
                WingetExecutionStatus::ProviderSucceededPostStateUnverified
            } else {
                WingetExecutionStatus::ProviderFailed
            }
        }
        ProcessTerminationReason::TimedOut if containment.process_tree_terminated() => {
            WingetExecutionStatus::TimedOut
        }
        ProcessTerminationReason::ExplicitCancellation if containment.process_tree_terminated() => {
            WingetExecutionStatus::Cancelled
        }
        _ => WingetExecutionStatus::UnknownRequiresRecovery,
    };
'''
replace_once(
    "crates/toolos-winget/src/execution.rs",
    old_status,
    "    let status = classify_execution_status(&process_evidence, &containment);\n",
)
helper = '''
fn classify_execution_status(
    process_evidence: &ProcessEvidence,
    containment: &ProcessContainmentEvidence,
) -> WingetExecutionStatus {
    match &containment.termination_reason {
        ProcessTerminationReason::ProcessExited if containment.process_tree_terminated() => {
            if process_evidence.exit_code == Some(0) {
                WingetExecutionStatus::ProviderSucceededPostStateUnverified
            } else {
                WingetExecutionStatus::ProviderFailed
            }
        }
        ProcessTerminationReason::TimedOut if containment.process_tree_terminated() => {
            WingetExecutionStatus::TimedOut
        }
        ProcessTerminationReason::ExplicitCancellation | ProcessTerminationReason::DaemonShutdown
            if containment.process_tree_terminated() =>
        {
            WingetExecutionStatus::Cancelled
        }
        _ => WingetExecutionStatus::UnknownRequiresRecovery,
    }
}

'''
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "#[allow(clippy::too_many_arguments)]\npub fn build_execution_report(\n",
    helper + "#[allow(clippy::too_many_arguments)]\npub fn build_execution_report(\n",
)
execution_tests = '''

    fn process_evidence(exit_code: Option<i32>) -> ProcessEvidence {
        ProcessEvidence {
            executable: "winget".to_owned(),
            args: vec!["install".to_owned()],
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 1,
        }
    }

    fn containment(
        reason: ProcessTerminationReason,
        confirmed: bool,
        active_processes_after: Option<u32>,
    ) -> ProcessContainmentEvidence {
        ProcessContainmentEvidence {
            method: "WINDOWS_JOB_OBJECT_PROC_THREAD_ATTRIBUTE_JOB_LIST".to_owned(),
            root_process_id: Some(42),
            kill_on_job_close: true,
            assigned_at_creation: true,
            inherited_handle_list_restricted: true,
            termination_reason: reason,
            termination_requested: true,
            termination_confirmed: confirmed,
            active_processes_after,
            descendants_outlived_root: false,
            detail: None,
        }
    }

    #[test]
    fn confirmed_root_exit_classifies_provider_result() {
        let evidence = containment(ProcessTerminationReason::ProcessExited, true, Some(0));
        assert_eq!(
            classify_execution_status(&process_evidence(Some(0)), &evidence),
            WingetExecutionStatus::ProviderSucceededPostStateUnverified
        );
        assert_eq!(
            classify_execution_status(&process_evidence(Some(1)), &evidence),
            WingetExecutionStatus::ProviderFailed
        );
    }

    #[test]
    fn confirmed_timeout_and_cancellation_are_distinct_terminal_states() {
        assert_eq!(
            classify_execution_status(
                &process_evidence(None),
                &containment(ProcessTerminationReason::TimedOut, true, Some(0)),
            ),
            WingetExecutionStatus::TimedOut
        );
        assert_eq!(
            classify_execution_status(
                &process_evidence(None),
                &containment(
                    ProcessTerminationReason::ExplicitCancellation,
                    true,
                    Some(0),
                ),
            ),
            WingetExecutionStatus::Cancelled
        );
        assert_eq!(
            classify_execution_status(
                &process_evidence(None),
                &containment(ProcessTerminationReason::DaemonShutdown, true, Some(0)),
            ),
            WingetExecutionStatus::Cancelled
        );
    }

    #[test]
    fn unconfirmed_or_escaped_tree_requires_recovery() {
        assert_eq!(
            classify_execution_status(
                &process_evidence(Some(0)),
                &containment(ProcessTerminationReason::ProcessExited, false, Some(1)),
            ),
            WingetExecutionStatus::UnknownRequiresRecovery
        );
        assert_eq!(
            classify_execution_status(
                &process_evidence(Some(0)),
                &containment(
                    ProcessTerminationReason::DescendantsOutlivedRoot,
                    true,
                    Some(0),
                ),
            ),
            WingetExecutionStatus::UnknownRequiresRecovery
        );
    }
'''
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "    #[test]\n    fn execution_confirmation_is_plan_bound() {\n        assert_eq!(\n            execution_confirmation(\"Git.Git\", &\"a\".repeat(64)).expect(\"confirmation\"),\n            \"EXECUTE INSTALL Git.Git aaaaaaaaaaaa\"\n        );\n    }\n}\n",
    "    #[test]\n    fn execution_confirmation_is_plan_bound() {\n        assert_eq!(\n            execution_confirmation(\"Git.Git\", &\"a\".repeat(64)).expect(\"confirmation\"),\n            \"EXECUTE INSTALL Git.Git aaaaaaaaaaaa\"\n        );\n    }\n" + execution_tests + "}\n",
)

# TypeScript contract parity.
replace_once(
    "apps/toolos-ui/src/api.ts",
    '''export type ProcessEvidence = {
  executable: string;
  args: string[];
  exit_code: number | null;
  stdout: string;
  stderr: string;
  timed_out: boolean;
  duration_ms: number;
};
''',
    '''export type ProcessEvidence = {
  executable: string;
  args: string[];
  exit_code: number | null;
  stdout: string;
  stderr: string;
  timed_out: boolean;
  duration_ms: number;
};

export type ProcessTerminationReason =
  | "PROCESS_EXITED"
  | "TIMED_OUT"
  | "EXPLICIT_CANCELLATION"
  | "DAEMON_SHUTDOWN"
  | "DESCENDANTS_OUTLIVED_ROOT"
  | "CONTAINMENT_FAILURE"
  | "UNAVAILABLE";

export type ProcessContainmentEvidence = {
  method: string;
  root_process_id: number | null;
  kill_on_job_close: boolean;
  assigned_at_creation: boolean;
  inherited_handle_list_restricted: boolean;
  termination_reason: ProcessTerminationReason;
  termination_requested: boolean;
  termination_confirmed: boolean;
  active_processes_after: number | null;
  descendants_outlived_root: boolean;
  detail: string | null;
};
''',
)
replace_once(
    "apps/toolos-ui/src/api.ts",
    '''    | "APPROVED_EXECUTION_DISABLED"
    | "EXECUTING"
    | "EXECUTION_SUCCEEDED_UNVERIFIED"
    | "EXECUTION_FAILED"
    | "EXPIRED";
''',
    '''    | "APPROVED_EXECUTION_DISABLED"
    | "APPROVED_AWAITING_EXECUTION"
    | "EXECUTING"
    | "EXECUTION_SUCCEEDED_UNVERIFIED"
    | "EXECUTION_FAILED"
    | "EXECUTION_CANCELLED"
    | "UNKNOWN_REQUIRES_RECOVERY"
    | "EXPIRED";
''',
)
replace_once(
    "apps/toolos-ui/src/api.ts",
    '''  status: "APPROVED_EXECUTION_DISABLED";
  execution_enabled: false;
''',
    '''  status: "APPROVED_AWAITING_EXECUTION";
  execution_enabled: true;
''',
)
replace_once(
    "apps/toolos-ui/src/api.ts",
    '''export type WingetExecutionStatus =
  | "PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED"
  | "PROVIDER_FAILED"
  | "TIMED_OUT";
''',
    '''export type WingetExecutionStatus =
  | "PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED"
  | "PROVIDER_FAILED"
  | "TIMED_OUT"
  | "CANCELLED"
  | "UNKNOWN_REQUIRES_RECOVERY";
''',
)
replace_once(
    "apps/toolos-ui/src/api.ts",
    "  process_evidence: ProcessEvidence;\n  preflight_resolution: WingetResolutionReport;\n",
    "  process_evidence: ProcessEvidence;\n  containment: ProcessContainmentEvidence;\n  preflight_resolution: WingetResolutionReport;\n",
)
replace_once(
    "apps/toolos-ui/src/api.ts",
    '''  getWingetInstallPlan: (planId: string) =>
''',
    '''  cancelWingetInstallPlan: (planId: string) =>
    daemonRequest<{
      plan_id: string;
      active: boolean;
      cancel_requested: boolean;
    }>("winget.install.cancel", { plan_id: planId }),
  getWingetInstallPlan: (planId: string) =>
''',
)

# UI supports live cancellation and presents containment proof.
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    "  const [execution, setExecution] = useState<WingetInstallExecutionReport | null>(null);\n  const [state, setState] = useState<State>(\"idle\");\n",
    "  const [execution, setExecution] = useState<WingetInstallExecutionReport | null>(null);\n  const [cancelRequested, setCancelRequested] = useState(false);\n  const [state, setState] = useState<State>(\"idle\");\n",
)
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    "    setExecution(null);\n    setMessage(\"Resolving identity and installed state, then hashing an immutable plan…\");\n",
    "    setExecution(null);\n    setCancelRequested(false);\n    setMessage(\"Resolving identity and installed state, then hashing an immutable plan…\");\n",
)
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    "    setState(\"loading\");\n    setMessage(\"Revalidating the pinned plan, consuming the one-time receipt, and invoking WinGet…\");\n",
    "    setState(\"loading\");\n    setCancelRequested(false);\n    setMessage(\"Revalidating the pinned plan, consuming the one-time receipt, and invoking WinGet inside a Windows Job Object…\");\n",
)
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    "      setExecution(result.report);\n      setLock(null);\n      await onEvidence();\n",
    "      setExecution(result.report);\n      setCancelRequested(false);\n      if (result.report.status !== \"UNKNOWN_REQUIRES_RECOVERY\") {\n        setLock(null);\n      }\n      await onEvidence();\n",
)
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    "    } catch (error) {\n      setState(\"error\");\n      setMessage(error instanceof Error ? error.message : String(error));\n    }\n  };\n\n  const copyExecutionPhrase = async () => {\n",
    '''    } catch (error) {
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
''',
)
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    '''                <button
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
''',
    '''                <button
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
''',
)
replace_once(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    '''            <div className="execution-result">
              <strong>{execution.status}</strong>
              <span>{execution.verification_claim}</span>
              <span>Exit code: {execution.process_evidence.exit_code ?? "Unavailable"}</span>
              <span>Duration: {execution.process_evidence.duration_ms} ms</span>
''',
    '''            <div
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
''',
)
replace_once(
    "apps/toolos-ui/src/install-plan.css",
    '''.execution-result {
  border-color: rgba(89, 190, 139, 0.55);
}
''',
    '''.execution-result {
  border-color: rgba(89, 190, 139, 0.55);
}

.execution-result.unknown,
.cancel-execution {
  border-color: rgba(232, 101, 101, 0.7);
}
''',
)

# README and ADR expose the actual safety contract and command.
replace_once(
    "README.md",
    '''cargo run -p toolos-cli -- winget-install-execute --plan-id <UUID> --approval-id <UUID> --confirmation "<EXACT EXECUTION PHRASE>"
cargo run -p toolos-cli -- evidence
''',
    '''cargo run -p toolos-cli -- winget-install-execute --plan-id <UUID> --approval-id <UUID> --confirmation "<EXACT EXECUTION PHRASE>"
# While an execution is active, request cancellation of the entire contained process tree:
cargo run -p toolos-cli -- winget-install-cancel --plan-id <UUID>
cargo run -p toolos-cli -- evidence
''',
)
replace_once(
    "README.md",
    "The current release permits read-only observations, governed metadata, and one narrow executable slice: an exact version-pinned, architecture-pinned, user-scope WinGet install after fresh revalidation and two separate short-lived confirmations. ToolOS never auto-accepts agreements, requests elevation, adds installer overrides, bypasses hashes, forces execution, skips dependencies, or claims application health from an exit code. Extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, machine-scope installation, and arbitrary repository execution remain unimplemented.\n",
    "The current draft permits read-only observations, governed metadata, and one narrow executable slice: an exact version-pinned, architecture-pinned, user-scope WinGet install after fresh revalidation and two separate short-lived confirmations. On Windows, the adapter and all descendants are assigned atomically to a kill-on-close Job Object; timeout and cancellation are terminal only when ToolOS confirms zero active job processes. Unconfirmed containment becomes `UNKNOWN_REQUIRES_RECOVERY` and retains the package-manager lock. ToolOS never auto-accepts agreements, requests elevation, adds installer overrides, bypasses hashes, forces execution, skips dependencies, or claims application health from an exit code. Extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, machine-scope installation, and arbitrary repository execution remain unimplemented.\n",
)
replace_once(
    "README.md",
    "- [`docs/provider-decisions/ADR-0003-winget-exact-preview.md`](docs/provider-decisions/ADR-0003-winget-exact-preview.md)\n",
    "- [`docs/provider-decisions/ADR-0003-winget-exact-preview.md`](docs/provider-decisions/ADR-0003-winget-exact-preview.md)\n- [`docs/provider-decisions/ADR-0005-windows-job-containment.md`](docs/provider-decisions/ADR-0005-windows-job-containment.md)\n",
)
replace_once(
    "docs/provider-decisions/ADR-0004-winget-pinned-user-execution.md",
    "- Issue #8 proves Windows process-tree containment and descendant termination on timeout;\n",
    "- ADR-0005 and the Windows integration tests prove process-tree containment and descendant termination on timeout/cancellation;\n",
)
