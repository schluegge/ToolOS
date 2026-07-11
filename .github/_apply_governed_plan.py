from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected text not found in {path}: {old[:120]!r}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")


# Winget domain and plan hashing.
replace_once(
    "crates/toolos-winget/Cargo.toml",
    "schemars.workspace = true\nserde.workspace = true\n",
    "schemars.workspace = true\nserde.workspace = true\nserde_json.workspace = true\nsha2.workspace = true\nuuid.workspace = true\n",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "use chrono::{DateTime, Utc};\nuse schemars::JsonSchema;\nuse serde::{Deserialize, Serialize};\n",
    "use chrono::{DateTime, Duration, Utc};\nuse schemars::JsonSchema;\nuse serde::{Deserialize, Serialize};\nuse sha2::{Digest, Sha256};\nuse uuid::Uuid;\n",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "pub struct WingetInstalledStateReport {\n    pub provider_id: String,\n    pub provider_version: Option<String>,\n    pub status: InstalledQueryStatus,\n    pub selector: PackageSelector,\n    pub installed_probe: CommandPreview,\n    pub installed_evidence: Option<ProcessEvidence>,\n    pub observed_at: DateTime<Utc>,\n    pub definitive_installed_match: Option<bool>,\n    pub limitations: Vec<String>,\n    pub single_safest_next_action: String,\n}\n\npub fn normalize_selector",
    """pub struct WingetInstalledStateReport {
    pub provider_id: String,
    pub provider_version: Option<String>,
    pub status: InstalledQueryStatus,
    pub selector: PackageSelector,
    pub installed_probe: CommandPreview,
    pub installed_evidence: Option<ProcessEvidence>,
    pub observed_at: DateTime<Utc>,
    pub definitive_installed_match: Option<bool>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallPlanStatus {
    AwaitingApproval,
    Blocked,
    ApprovedExecutionDisabled,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApprovalChallenge {
    pub required_phrase: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstallPlan {
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub status: InstallPlanStatus,
    pub selector: PackageSelector,
    pub resolution: WingetResolutionReport,
    pub installed_state: WingetInstalledStateReport,
    pub install_preview: CommandPreview,
    pub approval_allowed: bool,
    pub approval_challenge: Option<ApprovalChallenge>,
    pub lock_key: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub execution_enabled: bool,
    pub blockers: Vec<String>,
    pub pre_execution_requirements: Vec<String>,
    pub verification: Vec<String>,
    pub rollback: Vec<String>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstallApprovalReceipt {
    pub approval_id: Uuid,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub package_id: String,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub lock_key: String,
    pub lock_expires_at: DateTime<Utc>,
    pub status: InstallPlanStatus,
    pub execution_enabled: bool,
    pub limitations: Vec<String>,
}

pub fn build_install_plan(
    resolution: WingetResolutionReport,
    installed_state: WingetInstalledStateReport,
    now: DateTime<Utc>,
    ttl_seconds: u64,
) -> Result<WingetInstallPlan, String> {
    if resolution.selector != installed_state.selector {
        return Err("resolution and installed-state selectors differ".to_owned());
    }

    let ttl_seconds = ttl_seconds.clamp(60, 900);
    let expires_at = now + Duration::seconds(i64::try_from(ttl_seconds).unwrap_or(900));
    let plan_id = Uuid::new_v4();
    let lock_key = "package-manager:winget".to_owned();
    let install_preview = resolution.install_preview.clone();
    let mut blockers = Vec::new();

    if resolution.status != ResolutionStatus::ResolvedExact {
        blockers.push("Exact WinGet package resolution did not succeed.".to_owned());
    }
    if installed_state.status != InstalledQueryStatus::QueryCompleted {
        blockers.push("The installed-state query did not complete successfully.".to_owned());
    }
    if installed_state.definitive_installed_match == Some(true) {
        blockers.push("The exact package is already installed according to definitive evidence.".to_owned());
    }

    let fingerprint = serde_json::to_vec(&(
        plan_id,
        &resolution,
        &installed_state,
        &install_preview,
        now,
        expires_at,
        &lock_key,
    ))
    .map_err(|error| format!("cannot serialize install-plan fingerprint: {error}"))?;
    let plan_hash = format!("{:x}", Sha256::digest(fingerprint));
    let approval_allowed = blockers.is_empty();
    let approval_challenge = approval_allowed.then(|| ApprovalChallenge {
        required_phrase: format!(
            "APPROVE INSTALL {} {}",
            resolution.selector.package_id,
            &plan_hash[..12]
        ),
        expires_at,
    });
    let status = if approval_allowed {
        InstallPlanStatus::AwaitingApproval
    } else {
        InstallPlanStatus::Blocked
    };
    let single_safest_next_action = if approval_allowed {
        "Review the exact command, raw identity and installed-state evidence, then enter the approval phrase before the plan expires. Approval still cannot execute the installer."
            .to_owned()
    } else {
        "Resolve every blocker and create a new plan; blocked plans cannot be approved."
            .to_owned()
    };

    Ok(WingetInstallPlan {
        plan_id,
        plan_hash,
        status,
        selector: resolution.selector.clone(),
        resolution,
        installed_state,
        install_preview,
        approval_allowed,
        approval_challenge,
        lock_key,
        created_at: now,
        expires_at,
        execution_enabled: false,
        blockers,
        pre_execution_requirements: vec![
            "Re-run exact package resolution immediately before any future execution.".to_owned(),
            "Re-run installed-state evidence immediately before any future execution.".to_owned(),
            "Review package and source agreements separately; approval does not accept them.".to_owned(),
            "Acquire the package-manager lock and retain it through verification.".to_owned(),
            "Request elevation separately if machine scope or the installer requires it.".to_owned(),
        ],
        verification: vec![
            "Run the exact installed-state query after installation.".to_owned(),
            "Run a provider-appropriate executable or application healthcheck when one is defined."
                .to_owned(),
            "Persist command, exit code, bounded output, duration, and post-state evidence."
                .to_owned(),
        ],
        rollback: vec![
            "Use the exact uninstall preview only after a separate destructive-action approval."
                .to_owned(),
            "Do not claim full rollback: installer-created files, services, settings, and user data may remain."
                .to_owned(),
        ],
        limitations: vec![
            "WinGet has no documented true no-side-effect install dry-run in the command surface used here; ToolOS dry run means plan generation without invoking `winget install`."
                .to_owned(),
            "Localized WinGet list output is retained as evidence and is not parsed into a definitive installed verdict."
                .to_owned(),
            "The generated command contains no agreement acceptance, hash bypass, dependency skip, force, override, or custom installer arguments."
                .to_owned(),
            "Approval changes only ToolOS metadata and a local lock; machine execution remains disabled."
                .to_owned(),
        ],
        single_safest_next_action,
    })
}

pub fn build_approval_receipt(
    plan: &WingetInstallPlan,
    confirmation: &str,
    now: DateTime<Utc>,
    lock_ttl_seconds: u64,
) -> Result<WingetInstallApprovalReceipt, String> {
    if plan.status != InstallPlanStatus::AwaitingApproval || !plan.approval_allowed {
        return Err("install plan is not awaiting approval".to_owned());
    }
    let challenge = plan
        .approval_challenge
        .as_ref()
        .ok_or_else(|| "install plan has no approval challenge".to_owned())?;
    if now >= challenge.expires_at || now >= plan.expires_at {
        return Err("install plan approval window has expired".to_owned());
    }
    if confirmation.trim() != challenge.required_phrase {
        return Err("approval phrase does not match the immutable install plan".to_owned());
    }

    let lock_ttl_seconds = lock_ttl_seconds.clamp(30, 300);
    let requested_expiry =
        now + Duration::seconds(i64::try_from(lock_ttl_seconds).unwrap_or(300));
    let expires_at = std::cmp::min(plan.expires_at, requested_expiry);

    Ok(WingetInstallApprovalReceipt {
        approval_id: Uuid::new_v4(),
        plan_id: plan.plan_id,
        plan_hash: plan.plan_hash.clone(),
        package_id: plan.selector.package_id.clone(),
        approved_at: now,
        expires_at,
        lock_key: plan.lock_key.clone(),
        lock_expires_at: expires_at,
        status: InstallPlanStatus::ApprovedExecutionDisabled,
        execution_enabled: false,
        limitations: vec![
            "This receipt authorizes only the immutable plan hash during its short validity window."
                .to_owned(),
            "The package-manager lock is local to ToolOS and cannot prevent external WinGet processes."
                .to_owned(),
            "No package or source agreement was accepted and no installer was executed.".to_owned(),
        ],
    })
}

pub fn normalize_selector""",
)
replace_once(
    "crates/toolos-winget/src/lib.rs",
    "    #[test]\n    fn install_preview_does_not_accept_agreements_or_bypass_hashes() {",
    """    #[test]
    fn governed_plan_binds_hash_phrase_and_expiry() {
        let resolution = WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: ResolutionStatus::ResolvedExact,
            selector: selector(),
            identity_probe: identity_probe(&selector()),
            identity_evidence: None,
            install_preview: install_preview(&selector()),
            uninstall_preview: uninstall_preview(&selector()),
            observed_at: Utc::now(),
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let installed = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector(),
            installed_probe: installed_probe(&selector()),
            installed_evidence: None,
            observed_at: Utc::now(),
            definitive_installed_match: None,
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let now = Utc::now();
        let plan = build_install_plan(resolution, installed, now, 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::AwaitingApproval);
        assert_eq!(plan.plan_hash.len(), 64);
        assert!(!plan.execution_enabled);
        let phrase = plan
            .approval_challenge
            .as_ref()
            .expect("challenge")
            .required_phrase
            .clone();
        let receipt = build_approval_receipt(&plan, &phrase, now, 300).expect("receipt");
        assert_eq!(receipt.plan_hash, plan.plan_hash);
        assert!(!receipt.execution_enabled);
        assert!(build_approval_receipt(&plan, "wrong", now, 300).is_err());
    }

    #[test]
    fn governed_plan_blocks_unresolved_identity() {
        let resolution = WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: None,
            status: ResolutionStatus::Blocked,
            selector: selector(),
            identity_probe: identity_probe(&selector()),
            identity_evidence: None,
            install_preview: install_preview(&selector()),
            uninstall_preview: uninstall_preview(&selector()),
            observed_at: Utc::now(),
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let installed = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: None,
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector(),
            installed_probe: installed_probe(&selector()),
            installed_evidence: None,
            observed_at: Utc::now(),
            definitive_installed_match: None,
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let plan = build_install_plan(resolution, installed, Utc::now(), 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::Blocked);
        assert!(!plan.approval_allowed);
        assert!(plan.approval_challenge.is_none());
    }

    #[test]
    fn install_preview_does_not_accept_agreements_or_bypass_hashes() {""",
)

# Generic persisted plans, receipts, and expiring resource locks.
replace_once(
    "crates/toolos-storage/Cargo.toml",
    "rusqlite_migration.workspace = true\nserde_json.workspace = true\n",
    "rusqlite_migration.workspace = true\nserde.workspace = true\nserde_json.workspace = true\n",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    "use rusqlite::{params, Connection, OptionalExtension};\n",
    "use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};\nuse serde::{Deserialize, Serialize};\n",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    "    M::up(\n        \"CREATE TABLE daemon_metadata (\n            key TEXT PRIMARY KEY,\n            value TEXT NOT NULL,\n            updated_at TEXT NOT NULL\n        );\",\n    ),\n];",
    """    M::up(
        "CREATE TABLE daemon_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    ),
    M::up(
        "CREATE TABLE action_plan (
            id TEXT PRIMARY KEY,
            capability TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            status TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            approval_phrase TEXT NOT NULL,
            record_json TEXT NOT NULL
        );
        CREATE INDEX action_plan_status_idx ON action_plan(status, expires_at);
        CREATE TABLE approval_receipt (
            id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL UNIQUE,
            plan_hash TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            record_json TEXT NOT NULL,
            FOREIGN KEY(plan_id) REFERENCES action_plan(id)
        );
        CREATE TABLE resource_lock (
            resource_key TEXT PRIMARY KEY,
            holder_plan_id TEXT NOT NULL,
            acquired_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            FOREIGN KEY(holder_plan_id) REFERENCES action_plan(id)
        );",
    ),
];""",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    "    #[error(\"invalid timestamp in database: {0}\")]\n    Timestamp(#[from] chrono::ParseError),\n}\n\n#[derive(Debug, Clone)]\npub struct Storage",
    """    #[error("invalid timestamp in database: {0}")]
    Timestamp(#[from] chrono::ParseError),
    #[error("action plan not found: {0}")]
    PlanNotFound(String),
    #[error("action plan expired: {0}")]
    PlanExpired(String),
    #[error("action plan hash mismatch")]
    PlanHashMismatch,
    #[error("approval phrase mismatch")]
    ApprovalPhraseMismatch,
    #[error("action plan is not awaiting approval: {0}")]
    PlanNotApprovable(String),
    #[error("resource lock {resource_key} is held by plan {holder_plan_id} until {expires_at}")]
    ResourceLocked {
        resource_key: String,
        holder_plan_id: String,
        expires_at: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredActionPlan {
    pub id: Uuid,
    pub capability: String,
    pub resource_key: String,
    pub status: String,
    pub plan_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub approval_phrase: String,
    pub record_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredApprovalReceipt {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub resource_key: String,
    pub record_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredResourceLock {
    pub resource_key: String,
    pub holder_plan_id: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Storage""",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    "    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), StorageError> {",
    """    pub fn store_action_plan(&self, plan: &StoredActionPlan) -> Result<(), StorageError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO action_plan (
                id, capability, resource_key, status, plan_hash, created_at, expires_at,
                approval_phrase, record_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                plan.id.to_string(),
                plan.capability,
                plan.resource_key,
                plan.status,
                plan.plan_hash,
                plan.created_at.to_rfc3339(),
                plan.expires_at.to_rfc3339(),
                plan.approval_phrase,
                plan.record_json,
            ],
        )?;
        Ok(())
    }

    pub fn get_action_plan(&self, id: Uuid) -> Result<Option<StoredActionPlan>, StorageError> {
        let connection = self.open_connection()?;
        let row = connection
            .query_row(
                "SELECT capability, resource_key, status, plan_hash, created_at, expires_at,
                        approval_phrase, record_json
                 FROM action_plan WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(
                capability,
                resource_key,
                status,
                plan_hash,
                created_at,
                expires_at,
                approval_phrase,
                record_json,
            )| {
                Ok(StoredActionPlan {
                    id,
                    capability,
                    resource_key,
                    status,
                    plan_hash,
                    created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
                    expires_at: DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc),
                    approval_phrase,
                    record_json,
                })
            },
        )
        .transpose()
    }

    pub fn approve_action_plan(
        &self,
        plan_id: Uuid,
        expected_hash: &str,
        confirmation: &str,
        updated_plan_json: &str,
        receipt: &StoredApprovalReceipt,
        lock: &StoredResourceLock,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let mut connection = self.open_connection()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let transaction = connection.transaction()?;
        let plan = transaction
            .query_row(
                "SELECT resource_key, status, plan_hash, expires_at, approval_phrase
                 FROM action_plan WHERE id = ?1",
                [plan_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StorageError::PlanNotFound(plan_id.to_string()))?;
        let (resource_key, status, plan_hash, expires_at, approval_phrase) = plan;
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        if now >= expires_at {
            return Err(StorageError::PlanExpired(plan_id.to_string()));
        }
        if status != "AWAITING_APPROVAL" {
            return Err(StorageError::PlanNotApprovable(status));
        }
        if plan_hash != expected_hash || receipt.plan_hash != expected_hash {
            return Err(StorageError::PlanHashMismatch);
        }
        if confirmation.trim() != approval_phrase {
            return Err(StorageError::ApprovalPhraseMismatch);
        }
        if resource_key != lock.resource_key || resource_key != receipt.resource_key {
            return Err(StorageError::PlanNotApprovable(
                "approval resource does not match plan".to_owned(),
            ));
        }

        let existing = transaction
            .query_row(
                "SELECT holder_plan_id, acquired_at, expires_at FROM resource_lock
                 WHERE resource_key = ?1",
                [&resource_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((holder_plan_id, _acquired_at, lock_expires_at)) = existing {
            let parsed = DateTime::parse_from_rfc3339(&lock_expires_at)?.with_timezone(&Utc);
            if parsed > now && holder_plan_id != plan_id.to_string() {
                return Err(StorageError::ResourceLocked {
                    resource_key,
                    holder_plan_id,
                    expires_at: lock_expires_at,
                });
            }
            transaction.execute(
                "DELETE FROM resource_lock WHERE resource_key = ?1",
                [&lock.resource_key],
            )?;
        }

        transaction.execute(
            "INSERT INTO resource_lock (resource_key, holder_plan_id, acquired_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                lock.resource_key,
                lock.holder_plan_id.to_string(),
                lock.acquired_at.to_rfc3339(),
                lock.expires_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "UPDATE action_plan SET status = ?1, record_json = ?2 WHERE id = ?3",
            params![
                "APPROVED_EXECUTION_DISABLED",
                updated_plan_json,
                plan_id.to_string()
            ],
        )?;
        transaction.execute(
            "INSERT INTO approval_receipt (
                id, plan_id, plan_hash, approved_at, expires_at, resource_key, record_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                receipt.id.to_string(),
                receipt.plan_id.to_string(),
                receipt.plan_hash,
                receipt.approved_at.to_rfc3339(),
                receipt.expires_at.to_rfc3339(),
                receipt.resource_key,
                receipt.record_json,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_resource_lock(
        &self,
        resource_key: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<StoredResourceLock>, StorageError> {
        let connection = self.open_connection()?;
        let row = connection
            .query_row(
                "SELECT holder_plan_id, acquired_at, expires_at FROM resource_lock
                 WHERE resource_key = ?1",
                [resource_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((holder_plan_id, acquired_at, expires_at)) = row else {
            return Ok(None);
        };
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        if expires_at <= now {
            connection.execute(
                "DELETE FROM resource_lock WHERE resource_key = ?1",
                [resource_key],
            )?;
            return Ok(None);
        }
        Ok(Some(StoredResourceLock {
            resource_key: resource_key.to_owned(),
            holder_plan_id: Uuid::parse_str(&holder_plan_id)?,
            acquired_at: DateTime::parse_from_rfc3339(&acquired_at)?.with_timezone(&Utc),
            expires_at,
        }))
    }

    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), StorageError> {""",
)
replace_once(
    "crates/toolos-storage/src/lib.rs",
    "    #[test]\n    fn evidence_and_events_round_trip() {",
    """    #[test]
    fn plan_approval_is_atomic_and_locks_resource() {
        let directory = tempdir().expect("temp directory");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let now = Utc::now();
        let plan_id = Uuid::new_v4();
        let plan = StoredActionPlan {
            id: plan_id,
            capability: "package.install.plan.winget".to_owned(),
            resource_key: "package-manager:winget".to_owned(),
            status: "AWAITING_APPROVAL".to_owned(),
            plan_hash: "abc".to_owned(),
            created_at: now,
            expires_at: now + chrono::Duration::minutes(10),
            approval_phrase: "APPROVE".to_owned(),
            record_json: "{}".to_owned(),
        };
        storage.store_action_plan(&plan).expect("store plan");
        let receipt = StoredApprovalReceipt {
            id: Uuid::new_v4(),
            plan_id,
            plan_hash: "abc".to_owned(),
            approved_at: now,
            expires_at: now + chrono::Duration::minutes(5),
            resource_key: plan.resource_key.clone(),
            record_json: "{}".to_owned(),
        };
        let lock = StoredResourceLock {
            resource_key: plan.resource_key.clone(),
            holder_plan_id: plan_id,
            acquired_at: now,
            expires_at: receipt.expires_at,
        };
        storage
            .approve_action_plan(plan_id, "abc", "APPROVE", "{}", &receipt, &lock, now)
            .expect("approve plan");
        let saved = storage.get_action_plan(plan_id).expect("get plan").expect("plan");
        assert_eq!(saved.status, "APPROVED_EXECUTION_DISABLED");
        assert_eq!(
            storage
                .get_resource_lock("package-manager:winget", now)
                .expect("lock")
                .expect("active lock")
                .holder_plan_id,
            plan_id
        );
    }

    #[test]
    fn plan_approval_rejects_wrong_phrase() {
        let directory = tempdir().expect("temp directory");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let now = Utc::now();
        let plan_id = Uuid::new_v4();
        let plan = StoredActionPlan {
            id: plan_id,
            capability: "package.install.plan.winget".to_owned(),
            resource_key: "package-manager:winget".to_owned(),
            status: "AWAITING_APPROVAL".to_owned(),
            plan_hash: "abc".to_owned(),
            created_at: now,
            expires_at: now + chrono::Duration::minutes(10),
            approval_phrase: "APPROVE".to_owned(),
            record_json: "{}".to_owned(),
        };
        storage.store_action_plan(&plan).expect("store plan");
        let receipt = StoredApprovalReceipt {
            id: Uuid::new_v4(),
            plan_id,
            plan_hash: "abc".to_owned(),
            approved_at: now,
            expires_at: now + chrono::Duration::minutes(5),
            resource_key: plan.resource_key.clone(),
            record_json: "{}".to_owned(),
        };
        let lock = StoredResourceLock {
            resource_key: plan.resource_key,
            holder_plan_id: plan_id,
            acquired_at: now,
            expires_at: receipt.expires_at,
        };
        assert!(matches!(
            storage.approve_action_plan(plan_id, "abc", "WRONG", "{}", &receipt, &lock, now),
            Err(StorageError::ApprovalPhraseMismatch)
        ));
    }

    #[test]
    fn evidence_and_events_round_trip() {""",
)

# Daemon integration.
replace_once(
    "apps/toolos-daemon/Cargo.toml",
    "toolos-storage = { path = \"../../crates/toolos-storage\" }\n",
    "toolos-storage = { path = \"../../crates/toolos-storage\" }\ntoolos-winget = { path = \"../../crates/toolos-winget\" }\n",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "use toolos_storage::Storage;\n",
    """use toolos_storage::{
    Storage, StoredActionPlan, StoredApprovalReceipt, StoredResourceLock,
};
use toolos_winget::{
    build_approval_receipt, build_install_plan, InstallPlanStatus, WingetInstallPlan,
    WingetInstalledStateReport, WingetResolutionReport,
};
""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        \"winget.installed\" => winget_installed(state, trace_id, &request.params).await,\n",
    """        "winget.installed" => winget_installed(state, trace_id, &request.params).await,
        "winget.install.plan" => winget_install_plan(state, trace_id, &request.params).await,
        "winget.install.plan.get" => winget_install_plan_get(state, &request.params),
        "winget.install.approve" => winget_install_approve(state, trace_id, &request.params),
        "winget.install.lock" => winget_install_lock(state),
""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "fn winget_evidence(\n",
    """async fn winget_install_plan(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let resolution_result = winget_resolve(state, trace_id, params).await?;
    let installed_result = winget_installed(state, trace_id, params).await?;
    let resolution: WingetResolutionReport = serde_json::from_value(
        resolution_result
            .get("snapshot")
            .cloned()
            .context("winget resolution result had no snapshot")?,
    )?;
    let installed_state: WingetInstalledStateReport = serde_json::from_value(
        installed_result
            .get("snapshot")
            .cloned()
            .context("winget installed-state result had no snapshot")?,
    )?;
    let plan = build_install_plan(resolution, installed_state, Utc::now(), 600)
        .map_err(anyhow::Error::msg)?;
    let approval_phrase = plan
        .approval_challenge
        .as_ref()
        .map(|challenge| challenge.required_phrase.clone())
        .unwrap_or_default();
    let stored = StoredActionPlan {
        id: plan.plan_id,
        capability: "package.install.plan.winget".to_owned(),
        resource_key: plan.lock_key.clone(),
        status: install_plan_status(&plan.status).to_owned(),
        plan_hash: plan.plan_hash.clone(),
        created_at: plan.created_at,
        expires_at: plan.expires_at,
        approval_phrase,
        record_json: serde_json::to_string(&plan)?,
    };
    state.storage.store_action_plan(&stored)?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-plan:{}", plan.plan_id),
        format!(
            "Governed WinGet install plan created with status {}",
            install_plan_status(&plan.status)
        ),
        "toolos.daemon.governance",
        serde_json::to_value(&plan)?,
        plan.limitations.clone(),
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_INSTALL_PLAN")?;
    state.storage.append_event(
        trace_id,
        "winget.install.plan.created",
        &json!({
            "plan_id": plan.plan_id,
            "plan_hash": plan.plan_hash,
            "status": install_plan_status(&plan.status),
            "expires_at": plan.expires_at
        }),
    )?;
    Ok(json!({"plan": plan, "evidence": evidence}))
}

fn winget_install_plan_get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.plan.get")?;
    let stored = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    Ok(serde_json::from_str(&stored.record_json)?)
}

fn winget_install_approve(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.approve")?;
    let expected_hash = required_string(params, "plan_hash", "winget.install.approve")?;
    let confirmation = required_string(params, "confirmation", "winget.install.approve")?;
    let stored = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    let plan: WingetInstallPlan = serde_json::from_str(&stored.record_json)?;
    let now = Utc::now();
    let receipt = build_approval_receipt(&plan, &confirmation, now, 300)
        .map_err(anyhow::Error::msg)?;
    let mut approved_plan = plan.clone();
    approved_plan.status = InstallPlanStatus::ApprovedExecutionDisabled;
    approved_plan.approval_challenge = None;
    approved_plan.single_safest_next_action =
        "Execution remains disabled. A future execution slice must revalidate identity, installed state, approval expiry, lock ownership, elevation, and agreements."
            .to_owned();
    let updated_plan_json = serde_json::to_string(&approved_plan)?;
    let stored_receipt = StoredApprovalReceipt {
        id: receipt.approval_id,
        plan_id: receipt.plan_id,
        plan_hash: receipt.plan_hash.clone(),
        approved_at: receipt.approved_at,
        expires_at: receipt.expires_at,
        resource_key: receipt.lock_key.clone(),
        record_json: serde_json::to_string(&receipt)?,
    };
    let lock = StoredResourceLock {
        resource_key: receipt.lock_key.clone(),
        holder_plan_id: receipt.plan_id,
        acquired_at: receipt.approved_at,
        expires_at: receipt.lock_expires_at,
    };
    state.storage.approve_action_plan(
        plan_id,
        &expected_hash,
        &confirmation,
        &updated_plan_json,
        &stored_receipt,
        &lock,
        now,
    )?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-approval:{}", receipt.approval_id),
        "Governed WinGet install plan approved while execution remained disabled",
        "toolos.daemon.governance",
        json!({"plan": approved_plan, "receipt": receipt, "lock": lock}),
        vec![
            "Approval is short-lived and bound to one immutable plan hash".to_owned(),
            "The local lock cannot block WinGet processes started outside ToolOS".to_owned(),
            "No installer, agreement acceptance, elevation, or machine mutation occurred"
                .to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_INSTALL_APPROVAL")?;
    state.storage.append_event(
        trace_id,
        "winget.install.plan.approved",
        &json!({
            "plan_id": plan_id,
            "approval_id": stored_receipt.id,
            "lock_key": lock.resource_key,
            "lock_expires_at": lock.expires_at,
            "execution_enabled": false
        }),
    )?;
    Ok(json!({
        "plan": approved_plan,
        "receipt": receipt,
        "lock": lock,
        "evidence": evidence
    }))
}

fn winget_install_lock(state: &AppState) -> anyhow::Result<Value> {
    let lock = state
        .storage
        .get_resource_lock("package-manager:winget", Utc::now())?;
    Ok(json!({
        "resource_key": "package-manager:winget",
        "active": lock.is_some(),
        "lock": lock
    }))
}

fn winget_evidence(
""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "        {\n            \"capability_id\": \"package.preview.install\",",
    """        {
            "capability_id": "package.install.plan.winget",
            "provider_id": "toolos.daemon.governance",
            "blast_radius": "LOCAL_METADATA_WRITE",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "package.install.approve.winget",
            "provider_id": "toolos.daemon.governance",
            "blast_radius": "LOCAL_METADATA_WRITE",
            "status": "IMPLEMENTED_EXECUTION_DISABLED"
        },
        {
            "capability_id": "package.install.execute.winget",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",
            "status": "DISABLED"
        },
        {
            "capability_id": "package.preview.install",""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "fn bounded_limit(params: &Value, default: usize) -> usize {",
    """fn required_string(params: &Value, key: &str, method: &str) -> anyhow::Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .with_context(|| format!("{method} requires a non-empty string '{key}' parameter"))
}

fn required_uuid(params: &Value, key: &str, method: &str) -> anyhow::Result<Uuid> {
    let value = required_string(params, key, method)?;
    Uuid::parse_str(&value).with_context(|| format!("{method} requires a UUID '{key}' parameter"))
}

fn install_plan_status(status: &InstallPlanStatus) -> &'static str {
    match status {
        InstallPlanStatus::AwaitingApproval => "AWAITING_APPROVAL",
        InstallPlanStatus::Blocked => "BLOCKED",
        InstallPlanStatus::ApprovedExecutionDisabled => "APPROVED_EXECUTION_DISABLED",
        InstallPlanStatus::Expired => "EXPIRED",
    }
}

fn bounded_limit(params: &Value, default: usize) -> usize {""",
)
replace_once(
    "apps/toolos-daemon/src/main.rs",
    "    #[test]\n    fn explicit_data_directory_wins() {",
    """    #[test]
    fn capabilities_include_governed_install_plan_but_disable_execution() {
        let values = capabilities()
            .as_array()
            .expect("capabilities array")
            .clone();
        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.plan.winget")
                && value.get("status").and_then(Value::as_str) == Some("IMPLEMENTED")
        }));
        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.execute.winget")
                && value.get("status").and_then(Value::as_str) == Some("DISABLED")
        }));
    }

    #[test]
    fn required_uuid_rejects_invalid_values() {
        assert!(required_uuid(
            &json!({"plan_id": "not-a-uuid"}),
            "plan_id",
            "winget.install.plan.get"
        )
        .is_err());
    }

    #[test]
    fn explicit_data_directory_wins() {""",
)

# CLI surface.
replace_once(
    "apps/toolos-cli/src/main.rs",
    "    /// List persisted evidence records.\n    Evidence {",
    """    /// Create a governed WinGet install plan without executing the installer.
    WingetInstallPlan {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "winget")]
        source: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, value_enum)]
        scope: Option<WingetScope>,
        #[arg(long)]
        architecture: Option<String>,
    },
    /// Approve one immutable install-plan hash for a short local window.
    WingetInstallApprove {
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        plan_hash: String,
        #[arg(long)]
        confirmation: String,
    },
    /// Show the current ToolOS WinGet package-manager lock.
    WingetInstallLock,
    /// Get one persisted install plan by UUID.
    WingetInstallPlanGet {
        #[arg(long)]
        plan_id: String,
    },
    /// List persisted evidence records.
    Evidence {""",
)
replace_once(
    "apps/toolos-cli/src/main.rs",
    "        Command::Evidence { limit } => (\"evidence.list\".to_owned(), json!({\"limit\": limit})),",
    """        Command::WingetInstallPlan {
            id,
            source,
            version,
            scope,
            architecture,
        } => (
            "winget.install.plan".to_owned(),
            json!({
                "package_id": id,
                "source": source,
                "version": version,
                "scope": scope.map(|value| value.as_str()),
                "architecture": architecture
            }),
        ),
        Command::WingetInstallApprove {
            plan_id,
            plan_hash,
            confirmation,
        } => (
            "winget.install.approve".to_owned(),
            json!({
                "plan_id": plan_id,
                "plan_hash": plan_hash,
                "confirmation": confirmation
            }),
        ),
        Command::WingetInstallLock => ("winget.install.lock".to_owned(), json!({})),
        Command::WingetInstallPlanGet { plan_id } => (
            "winget.install.plan.get".to_owned(),
            json!({"plan_id": plan_id}),
        ),
        Command::Evidence { limit } => ("evidence.list".to_owned(), json!({"limit": limit})),""",
)
replace_once(
    "apps/toolos-cli/src/main.rs",
    "    #[test]\n    fn clap_parses_winget_installed_query() {",
    """    #[test]
    fn clap_parses_governed_install_plan_and_approval() {
        let cli = Cli::try_parse_from([
            "toolos",
            "winget-install-plan",
            "--id",
            "Git.Git",
            "--scope",
            "user",
        ])
        .expect("parse install plan");
        assert!(matches!(cli.command, Command::WingetInstallPlan { .. }));

        let cli = Cli::try_parse_from([
            "toolos",
            "winget-install-approve",
            "--plan-id",
            "00000000-0000-0000-0000-000000000001",
            "--plan-hash",
            "abc",
            "--confirmation",
            "APPROVE INSTALL Git.Git abc",
        ])
        .expect("parse approval");
        assert!(matches!(cli.command, Command::WingetInstallApprove { .. }));
    }

    #[test]
    fn clap_parses_winget_installed_query() {""",
)

# React API and visible plan/approval workflow.
replace_once(
    "apps/toolos-ui/src/api.ts",
    "export type EvidenceRecord = {",
    """export type WingetInstalledStateReport = {
  provider_id: string;
  provider_version: string | null;
  status: "QUERY_COMPLETED" | "BLOCKED" | "UNAVAILABLE";
  selector: WingetPackageSelector;
  installed_probe: CommandPreview;
  installed_evidence: ProcessEvidence | null;
  observed_at: string;
  definitive_installed_match: boolean | null;
  limitations: string[];
  single_safest_next_action: string;
};

export type ApprovalChallenge = {
  required_phrase: string;
  expires_at: string;
};

export type WingetInstallPlan = {
  plan_id: string;
  plan_hash: string;
  status: "AWAITING_APPROVAL" | "BLOCKED" | "APPROVED_EXECUTION_DISABLED" | "EXPIRED";
  selector: WingetPackageSelector;
  resolution: WingetResolutionReport;
  installed_state: WingetInstalledStateReport;
  install_preview: CommandPreview;
  approval_allowed: boolean;
  approval_challenge: ApprovalChallenge | null;
  lock_key: string;
  created_at: string;
  expires_at: string;
  execution_enabled: boolean;
  blockers: string[];
  pre_execution_requirements: string[];
  verification: string[];
  rollback: string[];
  limitations: string[];
  single_safest_next_action: string;
};

export type WingetInstallApprovalReceipt = {
  approval_id: string;
  plan_id: string;
  plan_hash: string;
  package_id: string;
  approved_at: string;
  expires_at: string;
  lock_key: string;
  lock_expires_at: string;
  status: "APPROVED_EXECUTION_DISABLED";
  execution_enabled: false;
  limitations: string[];
};

export type ResourceLock = {
  resource_key: string;
  holder_plan_id: string;
  acquired_at: string;
  expires_at: string;
};

export type WingetApprovalResult = {
  plan: WingetInstallPlan;
  receipt: WingetInstallApprovalReceipt;
  lock: ResourceLock;
  evidence: EvidenceRecord;
};

export type EvidenceRecord = {""",
)
replace_once(
    "apps/toolos-ui/src/api.ts",
    "  listEvidence: (limit = 20) =>\n    daemonRequest<EvidenceRecord[]>(\"evidence.list\", { limit }),",
    """  createWingetInstallPlan: (selector: WingetPackageSelector) =>
    daemonRequest<{ plan: WingetInstallPlan; evidence: EvidenceRecord }>(
      "winget.install.plan",
      selector,
    ),
  approveWingetInstallPlan: (
    planId: string,
    planHash: string,
    confirmation: string,
  ) =>
    daemonRequest<WingetApprovalResult>("winget.install.approve", {
      plan_id: planId,
      plan_hash: planHash,
      confirmation,
    }),
  getWingetInstallPlan: (planId: string) =>
    daemonRequest<WingetInstallPlan>("winget.install.plan.get", {
      plan_id: planId,
    }),
  wingetInstallLock: () =>
    daemonRequest<{
      resource_key: string;
      active: boolean;
      lock: ResourceLock | null;
    }>("winget.install.lock"),
  listEvidence: (limit = 20) =>
    daemonRequest<EvidenceRecord[]>("evidence.list", { limit }),""",
)
replace_once(
    "apps/toolos-ui/src/WingetPanel.tsx",
    "import \"./winget.css\";\n",
    "import { InstallPlanPanel } from \"./InstallPlanPanel\";\nimport \"./winget.css\";\n",
)
replace_once(
    "apps/toolos-ui/src/WingetPanel.tsx",
    "          <ul className=\"limitation-list\">\n            {result.limitations.map((limitation) => (\n              <li key={limitation}>{limitation}</li>\n            ))}\n          </ul>\n",
    """          <ul className="limitation-list">
            {result.limitations.map((limitation) => (
              <li key={limitation}>{limitation}</li>
            ))}
          </ul>

          <InstallPlanPanel
            selector={result.selector}
            disabled={disabled}
            onEvidence={onEvidence}
          />
""",
)
write(
    "apps/toolos-ui/src/InstallPlanPanel.tsx",
    r'''import { useState } from "react";
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
''',
)
write(
    "apps/toolos-ui/src/install-plan.css",
    r'''.install-plan {
  margin-top: 24px;
  padding-top: 24px;
  border-top: 1px solid var(--line);
}

.install-plan-heading {
  display: flex;
  justify-content: space-between;
  gap: 24px;
  align-items: flex-start;
}

.install-plan-heading h3 {
  margin: 4px 0 8px;
  font-size: 1.1rem;
}

.install-plan-heading p {
  margin: 0;
  max-width: 720px;
  color: var(--muted);
}

.install-plan-message {
  margin-top: 16px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.025);
}

.install-plan-message.error {
  border-color: rgba(232, 101, 101, 0.55);
}

.install-plan-body {
  display: grid;
  gap: 14px;
  margin-top: 16px;
}

.plan-facts {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 10px;
}

.plan-facts > div,
.plan-command,
.plan-list,
.approval-box,
.approval-receipt {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 14px;
  background: rgba(0, 0, 0, 0.12);
}

.plan-facts span,
.approval-box span,
.approval-receipt span {
  display: block;
  color: var(--muted);
  font-size: 0.82rem;
}

.plan-facts code,
.plan-command code,
.approval-box code,
.approval-receipt code {
  display: block;
  margin-top: 7px;
  overflow-wrap: anywhere;
}

.plan-command span {
  display: inline-block;
  margin-top: 10px;
  color: var(--muted);
  font-size: 0.8rem;
}

.plan-list ul {
  margin: 10px 0 0;
  padding-left: 20px;
}

.plan-list li + li {
  margin-top: 7px;
}

.plan-list.blocked {
  border-color: rgba(232, 101, 101, 0.55);
}

.plan-list.muted {
  color: var(--muted);
}

.approval-box {
  border-color: rgba(223, 178, 74, 0.55);
}

.approval-controls {
  display: grid;
  grid-template-columns: auto minmax(240px, 1fr) auto;
  gap: 10px;
  margin-top: 12px;
}

.approval-controls input {
  min-width: 0;
}

.approval-receipt {
  border-color: rgba(89, 190, 139, 0.55);
}

@media (max-width: 840px) {
  .install-plan-heading {
    flex-direction: column;
  }

  .approval-controls {
    grid-template-columns: 1fr;
  }
}
''',
)
replace_once(
    "apps/toolos-ui/src/App.tsx",
    "          <strong>Read-only</strong>\n          <p>No installs, extraction, deletes, credentials, billing, or repository scripts.</p>",
    """          <strong>Planning and local approval only</strong>
          <p>No installs, extraction, deletes, agreement acceptance, elevation, credentials, billing, or repository scripts.</p>""",
)

# Documentation and schema.
write(
    "docs/provider-decisions/ADR-0004-winget-governed-install-plan.md",
    r'''# ADR-0004: Governed WinGet install plan and short-lived approval

## Status

Accepted for the planning-only milestone.

## Decision

ToolOS may create and approve an immutable WinGet installation plan, but it may not execute that plan in this milestone.

Plan creation performs the existing exact `winget show` resolution and exact installed-state query, then hashes the selector, evidence, disabled command preview, timestamps, and lock key. Plans expire after ten minutes.

Approval requires the exact generated phrase and plan hash. The SQLite transaction validates the unexpired plan, checks the phrase, reserves `package-manager:winget`, updates the plan state, and stores the approval receipt atomically. The approval and lock expire after at most five minutes.

## Safety boundary

- `winget install` is never invoked.
- `--accept-package-agreements` and `--accept-source-agreements` are never added.
- Hash bypasses, dependency skipping, force, overrides, custom installer arguments, and silent elevation are absent.
- The ToolOS lock is local coordination only; it cannot block external WinGet processes.
- Approval authorizes one immutable hash, not a package name or future regenerated command.

## Source evidence

The official Windows Package Manager documentation confirms exact ID/source selection with `winget install --id ... --exact --source ...` and documents version, scope, architecture, interactivity, agreement, hash-bypass, dependency, force, silent, and logging options. It does not expose a true no-side-effect install dry-run in the CLI surface used here. ToolOS therefore defines dry run as generating and persisting the plan without invoking the install command.

## Deferred

A future execution milestone must revalidate identity, installed state, approval expiry, lock ownership, agreement state, elevation requirements, and post-install healthchecks immediately before machine mutation.
''',
)
write(
    "schemas/winget-install-plan.schema.json",
    r'''{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://toolos.local/schemas/winget-install-plan.schema.json",
  "title": "ToolOS governed WinGet install plan",
  "type": "object",
  "required": [
    "plan_id",
    "plan_hash",
    "status",
    "selector",
    "resolution",
    "installed_state",
    "install_preview",
    "approval_allowed",
    "lock_key",
    "created_at",
    "expires_at",
    "execution_enabled",
    "blockers",
    "pre_execution_requirements",
    "verification",
    "rollback",
    "limitations",
    "single_safest_next_action"
  ],
  "properties": {
    "plan_id": { "type": "string", "format": "uuid" },
    "plan_hash": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
    "status": {
      "enum": ["AWAITING_APPROVAL", "BLOCKED", "APPROVED_EXECUTION_DISABLED", "EXPIRED"]
    },
    "selector": { "type": "object" },
    "resolution": { "type": "object" },
    "installed_state": { "type": "object" },
    "install_preview": { "type": "object" },
    "approval_allowed": { "type": "boolean" },
    "approval_challenge": {
      "type": ["object", "null"],
      "properties": {
        "required_phrase": { "type": "string", "minLength": 1 },
        "expires_at": { "type": "string", "format": "date-time" }
      }
    },
    "lock_key": { "const": "package-manager:winget" },
    "created_at": { "type": "string", "format": "date-time" },
    "expires_at": { "type": "string", "format": "date-time" },
    "execution_enabled": { "const": false },
    "blockers": { "type": "array", "items": { "type": "string" } },
    "pre_execution_requirements": { "type": "array", "items": { "type": "string" } },
    "verification": { "type": "array", "items": { "type": "string" } },
    "rollback": { "type": "array", "items": { "type": "string" } },
    "limitations": { "type": "array", "items": { "type": "string" } },
    "single_safest_next_action": { "type": "string", "minLength": 1 }
  },
  "additionalProperties": false
}
''',
)
replace_once(
    "README.md",
    "cargo run -p toolos-cli -- winget-resolve --id Git.Git --source winget --scope user --architecture x64\n",
    """cargo run -p toolos-cli -- winget-resolve --id Git.Git --source winget --scope user --architecture x64
cargo run -p toolos-cli -- winget-installed --id Git.Git --source winget --scope user
cargo run -p toolos-cli -- winget-install-plan --id Git.Git --source winget --scope user --architecture x64
# Then approve only the exact plan/hash/phrase returned by the previous command:
cargo run -p toolos-cli -- winget-install-approve --plan-id <UUID> --plan-hash <SHA256> --confirmation "<EXACT PHRASE>"
""",
)
replace_once(
    "README.md",
    "The current release remains read-only. No installation, extraction, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, or arbitrary repository execution is implemented.",
    "The current release permits read-only observations plus local plan, approval-receipt, and lock metadata. No installation, extraction, deletion, agreement acceptance, elevation, billing, credential extraction, browser stealth, CAPTCHA bypass, or arbitrary repository execution is implemented.",
)

print("governed WinGet install-plan feature applied")
