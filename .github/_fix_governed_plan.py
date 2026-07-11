from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected text not found in {path}: {old[:160]!r}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


replace_once(
    "crates/toolos-storage/src/lib.rs",
    """#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredResourceLock {
    pub resource_key: String,
    pub holder_plan_id: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Storage""",
    """#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredResourceLock {
    pub resource_key: String,
    pub holder_plan_id: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ActionPlanApproval<'a> {
    pub plan_id: Uuid,
    pub expected_hash: &'a str,
    pub confirmation: &'a str,
    pub updated_plan_json: &'a str,
    pub receipt: &'a StoredApprovalReceipt,
    pub lock: &'a StoredResourceLock,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Storage""",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    """    pub fn approve_action_plan(
        &self,
        plan_id: Uuid,
        expected_hash: &str,
        confirmation: &str,
        updated_plan_json: &str,
        receipt: &StoredApprovalReceipt,
        lock: &StoredResourceLock,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let mut connection = self.open_connection()?;""",
    """    pub fn approve_action_plan(
        &self,
        approval: &ActionPlanApproval<'_>,
    ) -> Result<(), StorageError> {
        let plan_id = approval.plan_id;
        let expected_hash = approval.expected_hash;
        let confirmation = approval.confirmation;
        let updated_plan_json = approval.updated_plan_json;
        let receipt = approval.receipt;
        let lock = approval.lock;
        let now = approval.now;
        let mut connection = self.open_connection()?;""",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    """        storage
            .approve_action_plan(plan_id, "abc", "APPROVE", "{}", &receipt, &lock, now)
            .expect("approve plan");""",
    """        storage
            .approve_action_plan(&ActionPlanApproval {
                plan_id,
                expected_hash: "abc",
                confirmation: "APPROVE",
                updated_plan_json: "{}",
                receipt: &receipt,
                lock: &lock,
                now,
            })
            .expect("approve plan");""",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    """        assert!(matches!(
            storage.approve_action_plan(plan_id, "abc", "WRONG", "{}", &receipt, &lock, now),
            Err(StorageError::ApprovalPhraseMismatch)
        ));""",
    """        assert!(matches!(
            storage.approve_action_plan(&ActionPlanApproval {
                plan_id,
                expected_hash: "abc",
                confirmation: "WRONG",
                updated_plan_json: "{}",
                receipt: &receipt,
                lock: &lock,
                now,
            }),
            Err(StorageError::ApprovalPhraseMismatch)
        ));""",
)

replace_once(
    "apps/toolos-daemon/src/main.rs",
    """use toolos_storage::{
    Storage, StoredActionPlan, StoredApprovalReceipt, StoredResourceLock,
};""",
    """use toolos_storage::{
    ActionPlanApproval, Storage, StoredActionPlan, StoredApprovalReceipt, StoredResourceLock,
};""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    """fn winget_install_plan_get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.plan.get")?;
    let stored = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    Ok(serde_json::from_str(&stored.record_json)?)
}""",
    """fn winget_install_plan_get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.plan.get")?;
    let stored = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    let mut plan: WingetInstallPlan = serde_json::from_str(&stored.record_json)?;
    if plan.status == InstallPlanStatus::AwaitingApproval && Utc::now() >= plan.expires_at {
        plan.status = InstallPlanStatus::Expired;
        plan.approval_allowed = false;
        plan.approval_challenge = None;
        plan.single_safest_next_action =
            "This plan expired. Create a new plan from fresh identity and installed-state evidence."
                .to_owned();
    }
    Ok(serde_json::to_value(plan)?)
}""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    """    state.storage.approve_action_plan(
        plan_id,
        &expected_hash,
        &confirmation,
        &updated_plan_json,
        &stored_receipt,
        &lock,
        now,
    )?;""",
    """    state.storage.approve_action_plan(&ActionPlanApproval {
        plan_id,
        expected_hash: &expected_hash,
        confirmation: &confirmation,
        updated_plan_json: &updated_plan_json,
        receipt: &stored_receipt,
        lock: &lock,
        now,
    })?;""",
)

verify = '''name: verify

on:
  push:
    branches: [main, "build/**"]
  pull_request:

permissions:
  contents: read

jobs:
  rust:
    name: Rust workspace
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace

  frontend:
    name: React frontend
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
      - run: corepack enable
      - run: pnpm install --no-frozen-lockfile
      - run: pnpm --dir apps/toolos-ui check
      - run: pnpm --dir apps/toolos-ui build

  tauri-windows:
    name: Tauri Windows compile
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: apps/toolos-ui/src-tauri
      - run: cargo check --manifest-path apps/toolos-ui/src-tauri/Cargo.toml
'''
(ROOT / ".github/workflows/verify.yml").write_text(verify, encoding="utf-8")

print("governed plan Clippy and expiry fixes applied")
