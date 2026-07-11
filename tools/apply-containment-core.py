from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one marker, found {count}: {old[:100]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")


replace_once(
    "crates/toolos-winget/src/lib.rs",
    "mod execution;\npub use execution::*;\n",
    "mod containment;\npub use containment::*;\nmod execution;\npub use execution::*;\n",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "    ExecutionSucceededUnverified,\n    ExecutionFailed,\n    Expired,\n",
    "    ExecutionSucceededUnverified,\n    ExecutionFailed,\n    ExecutionCancelled,\n    UnknownRequiresRecovery,\n    Expired,\n",
)

replace_once(
    "crates/toolos-winget/src/execution.rs",
    "    ProcessEvidence, WingetInstalledStateReport, WingetResolutionReport,\n",
    "    ProcessContainmentEvidence, ProcessEvidence, ProcessTerminationReason,\n    WingetInstalledStateReport, WingetResolutionReport,\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "    ProviderFailed,\n    TimedOut,\n",
    "    ProviderFailed,\n    TimedOut,\n    Cancelled,\n    UnknownRequiresRecovery,\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "    pub process_evidence: ProcessEvidence,\n    pub preflight_resolution: WingetResolutionReport,\n",
    "    pub process_evidence: ProcessEvidence,\n    pub containment: ProcessContainmentEvidence,\n    pub preflight_resolution: WingetResolutionReport,\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "    process_evidence: ProcessEvidence,\n    preflight_resolution: WingetResolutionReport,\n",
    "    process_evidence: ProcessEvidence,\n    containment: ProcessContainmentEvidence,\n    preflight_resolution: WingetResolutionReport,\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "    let status = if process_evidence.timed_out {\n        WingetExecutionStatus::TimedOut\n    } else if process_evidence.exit_code == Some(0) {\n        WingetExecutionStatus::ProviderSucceededPostStateUnverified\n    } else {\n        WingetExecutionStatus::ProviderFailed\n    };\n",
    "    let status = match &containment.termination_reason {\n        ProcessTerminationReason::ProcessExited if containment.process_tree_terminated() => {\n            if process_evidence.exit_code == Some(0) {\n                WingetExecutionStatus::ProviderSucceededPostStateUnverified\n            } else {\n                WingetExecutionStatus::ProviderFailed\n            }\n        }\n        ProcessTerminationReason::TimedOut if containment.process_tree_terminated() => {\n            WingetExecutionStatus::TimedOut\n        }\n        ProcessTerminationReason::ExplicitCancellation if containment.process_tree_terminated() => {\n            WingetExecutionStatus::Cancelled\n        }\n        _ => WingetExecutionStatus::UnknownRequiresRecovery,\n    };\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "        WingetExecutionStatus::TimedOut => (\n            \"The WinGet invocation exceeded the bounded execution window; final installer state is unknown.\"\n                .to_owned(),\n            \"Inspect WinGet logs and running installer processes before creating another plan.\"\n                .to_owned(),\n        ),\n",
    "        WingetExecutionStatus::TimedOut => (\n            \"The bounded execution window expired and ToolOS confirmed that the contained process tree terminated; partial installer state may remain.\"\n                .to_owned(),\n            \"Review the captured output and residual machine state before creating another plan.\"\n                .to_owned(),\n        ),\n        WingetExecutionStatus::Cancelled => (\n            \"The user requested cancellation and ToolOS confirmed that the contained process tree terminated; partial installer state may remain.\"\n                .to_owned(),\n            \"Review residual machine state before retrying or planning cleanup.\".to_owned(),\n        ),\n        WingetExecutionStatus::UnknownRequiresRecovery => (\n            \"ToolOS could not prove complete process-tree termination; installation state is unknown.\"\n                .to_owned(),\n            \"Do not start another mutable package action. Run the recovery inspection workflow.\"\n                .to_owned(),\n        ),\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "        process_evidence,\n        preflight_resolution,\n",
    "        process_evidence,\n        containment,\n        preflight_resolution,\n",
)
replace_once(
    "crates/toolos-winget/src/execution.rs",
    "            \"On timeout, ToolOS cannot prove that every installer child process terminated; inspect the machine before retrying.\"\n                .to_owned(),\n",
    "            \"The execution report records the containment method, root PID, termination reason, confirmation result, and remaining active-process count.\"\n                .to_owned(),\n",
)

replace_once(
    "crates/toolos-storage/src/lib.rs",
    "pub struct ActionExecutionFinish<'a> {\n    pub plan_id: Uuid,\n    pub final_status: &'a str,\n    pub updated_plan_json: &'a str,\n    pub resource_key: &'a str,\n}\n",
    "pub struct ActionExecutionFinish<'a> {\n    pub plan_id: Uuid,\n    pub final_status: &'a str,\n    pub updated_plan_json: &'a str,\n    pub resource_key: &'a str,\n    pub release_lock: bool,\n}\n",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    "        transaction.execute(\n            \"DELETE FROM resource_lock WHERE resource_key = ?1 AND holder_plan_id = ?2\",\n            params![execution.resource_key, execution.plan_id.to_string()],\n        )?;\n",
    "        if execution.release_lock {\n            transaction.execute(\n                \"DELETE FROM resource_lock WHERE resource_key = ?1 AND holder_plan_id = ?2\",\n                params![execution.resource_key, execution.plan_id.to_string()],\n            )?;\n        }\n",
)

replace_once(
    "apps/toolos-daemon/src/contained_adapter.rs",
    "        let response = transport_stdout\n            .lines()\n            .find(|line| !line.trim().is_empty())\n            .map(|line| {\n                serde_json::from_str::<RpcResponse>(line.trim())\n                    .map_err(|error| anyhow!(\"contained adapter returned invalid JSON-RPC: {error}\"))\n            })\n            .transpose()?;\n",
    "        let response = transport_stdout\n            .lines()\n            .find(|line| !line.trim().is_empty())\n            .and_then(|line| serde_json::from_str::<RpcResponse>(line.trim()).ok());\n",
)
replace_once(
    "apps/toolos-daemon/src/contained_adapter.rs",
    "use anyhow::{anyhow, Context};\n",
    "use anyhow::Context;\n",
)

replace_once(
    "apps/toolos-daemon/src/main.rs",
    "use std::path::{Path, PathBuf};\n",
    "use std::collections::HashMap;\nuse std::path::{Path, PathBuf};\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "use uuid::Uuid;\n\n#[derive(Clone)]\n",
    "use uuid::Uuid;\n\nmod contained_adapter;\n\n#[derive(Clone)]\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "    build_approval_receipt, build_execution_report, build_install_plan, validate_execution_request,\n    InstallPlanStatus, PackageScope, ProcessEvidence, WingetExecutionStatus,\n",
    "    build_approval_receipt, build_execution_report, build_install_plan, validate_execution_request,\n    CommandPreview, InstallPlanStatus, PackageScope, ProcessContainmentEvidence, ProcessEvidence,\n    ProcessTerminationReason, WingetExecutionStatus,\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "use tracing::{error, info, instrument};\n",
    "use toolos_windows_job::{CancellationReason, CancellationToken};\nuse tracing::{error, info, instrument};\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "    winget_adapter_path: PathBuf,\n}\n",
    "    winget_adapter_path: PathBuf,\n    active_install_executions: Arc<tokio::sync::Mutex<HashMap<Uuid, CancellationToken>>>,\n}\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        system_adapter_path,\n        winget_adapter_path,\n    });\n",
    "        system_adapter_path,\n        winget_adapter_path,\n        active_install_executions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),\n    });\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        \"winget.install.execute\" => winget_install_execute(state, trace_id, &request.params).await,\n        \"winget.install.lock\" => winget_install_lock(state),\n",
    "        \"winget.install.execute\" => winget_install_execute(state, trace_id, &request.params).await,\n        \"winget.install.cancel\" => winget_install_cancel(state, trace_id, &request.params).await,\n        \"winget.install.lock\" => winget_install_lock(state),\n",
)
old_execution = '''    let process_evidence = match invoke_adapter(
        &state.winget_adapter_path,
        "winget.install.execute",
        serde_json::to_value(&request)?,
        Duration::from_secs(31 * 60),
    )
    .await
    {
        Ok(payload) => serde_json::from_value::<ProcessEvidence>(payload)?,
        Err(error) => ProcessEvidence {
            executable: command.executable.clone(),
            args: command.args.clone(),
            exit_code: None,
            stdout: String::new(),
            stderr: format!("ToolOS adapter invocation failed: {error}"),
            timed_out: false,
            duration_ms: 0,
        },
    };

    let post_install_state = match winget_installed(state, trace_id, &selector_json).await {
        Ok(value) => value
            .get("snapshot")
            .cloned()
            .map(serde_json::from_value::<WingetInstalledStateReport>)
            .transpose()?,
        Err(error) => {
            state.storage.append_event(
                trace_id,
                "winget.install.post_state.failed",
                &json!({"execution_id": execution_id, "error": error.to_string()}),
            )?;
            None
        }
    };
'''
new_execution = '''    let cancellation = CancellationToken::default();
    {
        let mut active = state.active_install_executions.lock().await;
        if active.insert(plan_id, cancellation.clone()).is_some() {
            return Err(anyhow!("an execution is already active for plan {plan_id}"));
        }
    }
    let contained_result = contained_adapter::invoke(
        &state.winget_adapter_path,
        "winget.install.execute",
        serde_json::to_value(&request)?,
        Duration::from_secs(31 * 60),
        cancellation,
    )
    .await;
    state.active_install_executions.lock().await.remove(&plan_id);
    let (process_evidence, containment) =
        contained_process_evidence(&command, contained_result)?;

    let post_install_state = if containment.process_tree_terminated() {
        match winget_installed(state, trace_id, &selector_json).await {
            Ok(value) => value
                .get("snapshot")
                .cloned()
                .map(serde_json::from_value::<WingetInstalledStateReport>)
                .transpose()?,
            Err(error) => {
                state.storage.append_event(
                    trace_id,
                    "winget.install.post_state.failed",
                    &json!({"execution_id": execution_id, "error": error.to_string()}),
                )?;
                None
            }
        }
    } else {
        state.storage.append_event(
            trace_id,
            "winget.install.post_state.skipped",
            &json!({
                "execution_id": execution_id,
                "reason": "process-tree termination was not confirmed"
            }),
        )?;
        None
    };
'''
replace_once("apps/toolos-daemon/src/main.rs", old_execution, new_execution)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        process_evidence,\n        preflight_resolution,\n",
    "        process_evidence,\n        containment,\n        preflight_resolution,\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        WingetExecutionStatus::ProviderFailed | WingetExecutionStatus::TimedOut => {\n            InstallPlanStatus::ExecutionFailed\n        }\n",
    "        WingetExecutionStatus::ProviderFailed | WingetExecutionStatus::TimedOut => {\n            InstallPlanStatus::ExecutionFailed\n        }\n        WingetExecutionStatus::Cancelled => InstallPlanStatus::ExecutionCancelled,\n        WingetExecutionStatus::UnknownRequiresRecovery => {\n            InstallPlanStatus::UnknownRequiresRecovery\n        }\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "            resource_key: &plan.lock_key,\n        })?;\n",
    "            resource_key: &plan.lock_key,\n            release_lock: report.status != WingetExecutionStatus::UnknownRequiresRecovery,\n        })?;\n",
)
helper = '''
fn contained_process_evidence(
    command: &CommandPreview,
    outcome: anyhow::Result<contained_adapter::ContainedAdapterOutcome>,
) -> anyhow::Result<(ProcessEvidence, ProcessContainmentEvidence)> {
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let containment = ProcessContainmentEvidence::unavailable(error.to_string());
            return Ok((
                ProcessEvidence {
                    executable: command.executable.clone(),
                    args: command.args.clone(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("contained adapter setup failed: {error}"),
                    timed_out: false,
                    duration_ms: 0,
                },
                containment,
            ));
        }
    };

    let timed_out = outcome.containment.termination_reason == ProcessTerminationReason::TimedOut;
    if let Some(response) = outcome.response {
        if let Some(error) = response.error {
            return Ok((
                ProcessEvidence {
                    executable: command.executable.clone(),
                    args: command.args.clone(),
                    exit_code: None,
                    stdout: outcome.transport_stdout,
                    stderr: format!(
                        "adapter error {}: {}; {}",
                        error.code, error.message, outcome.transport_stderr
                    ),
                    timed_out,
                    duration_ms: outcome.duration_ms,
                },
                outcome.containment,
            ));
        }
        if let Some(payload) = response.result {
            if let Ok(evidence) = serde_json::from_value::<ProcessEvidence>(payload) {
                return Ok((evidence, outcome.containment));
            }
        }
    }

    Ok((
        ProcessEvidence {
            executable: command.executable.clone(),
            args: command.args.clone(),
            exit_code: None,
            stdout: outcome.transport_stdout,
            stderr: format!(
                "contained adapter produced no valid execution response; root_exit_code={:?}; {}",
                outcome.root_exit_code, outcome.transport_stderr
            ),
            timed_out,
            duration_ms: outcome.duration_ms,
        },
        outcome.containment,
    ))
}

async fn winget_install_cancel(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.cancel")?;
    let token = state
        .active_install_executions
        .lock()
        .await
        .get(&plan_id)
        .cloned();
    let active = token.is_some();
    let cancel_requested = token
        .as_ref()
        .is_some_and(|token| token.cancel(CancellationReason::ExplicitCancellation));
    state.storage.append_event(
        trace_id,
        "winget.install.execution.cancel_requested",
        &json!({
            "plan_id": plan_id,
            "active": active,
            "cancel_requested": cancel_requested
        }),
    )?;
    Ok(json!({
        "plan_id": plan_id,
        "active": active,
        "cancel_requested": cancel_requested
    }))
}

'''
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "fn validate_execution_authorization(\n",
    helper + "fn validate_execution_authorization(\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        InstallPlanStatus::ExecutionFailed => \"EXECUTION_FAILED\",\n        InstallPlanStatus::Expired => \"EXPIRED\",\n",
    "        InstallPlanStatus::ExecutionFailed => \"EXECUTION_FAILED\",\n        InstallPlanStatus::ExecutionCancelled => \"EXECUTION_CANCELLED\",\n        InstallPlanStatus::UnknownRequiresRecovery => \"UNKNOWN_REQUIRES_RECOVERY\",\n        InstallPlanStatus::Expired => \"EXPIRED\",\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        WingetExecutionStatus::TimedOut => \"TIMED_OUT\",\n",
    "        WingetExecutionStatus::TimedOut => \"TIMED_OUT\",\n        WingetExecutionStatus::Cancelled => \"CANCELLED\",\n        WingetExecutionStatus::UnknownRequiresRecovery => \"UNKNOWN_REQUIRES_RECOVERY\",\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        {\n            \"capability_id\": \"package.preview.install\",\n",
    "        {\n            \"capability_id\": \"package.install.cancel.winget\",\n            \"provider_id\": \"toolos.daemon.governance\",\n            \"blast_radius\": \"SAFETY_CONTROL\",\n            \"status\": \"IMPLEMENTED\"\n        },\n        {\n            \"capability_id\": \"package.preview.install\",\n",
)

replace_once(
    "apps/toolos-cli/src/main.rs",
    "    /// Show the current ToolOS WinGet package-manager lock.\n    WingetInstallLock,\n",
    "    /// Request cancellation of one active governed WinGet execution.\n    WingetInstallCancel {\n        #[arg(long)]\n        plan_id: String,\n    },\n    /// Show the current ToolOS WinGet package-manager lock.\n    WingetInstallLock,\n",
)
replace_once(
    "apps/toolos-cli/src/main.rs",
    "        Command::WingetInstallLock => (\"winget.install.lock\".to_owned(), json!({})),\n",
    "        Command::WingetInstallCancel { plan_id } => (\n            \"winget.install.cancel\".to_owned(),\n            json!({\"plan_id\": plan_id}),\n        ),\n        Command::WingetInstallLock => (\"winget.install.lock\".to_owned(), json!({})),\n",
)
