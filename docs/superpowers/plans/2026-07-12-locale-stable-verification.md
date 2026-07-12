# Locale-Stable Package Verification Implementation Plan

> Execute task-by-task. Parent PR #7 remains Draft. Backlog work remains out of scope.

## Goal

Complete Issue #10 with an official typed package-presence provider, immutable package recipes, independent package/application verdicts, and final Windows evidence.

## Task 1 — Verification domain

Create `crates/toolos-winget/src/verification.rs` with:

- `VerificationVerdict`: `Verified`, `NotVerified`, `Indeterminate`.
- `ApplicationHealthVerdict`: `Healthy`, `Unhealthy`, `Indeterminate`.
- dimension reports for ID, source, version, scope, architecture;
- official-provider contract/result types;
- recipe evidence/result types;
- final `WingetVerificationReport` with independent provider, package, and health claims;
- pure aggregation functions and fixture tests.

## Task 2 — Official PowerShell provider parser

Implement ToolOS JSON contract `toolos.microsoft-winget-client.installed/1`.

- Parse only `Id`, `Source`, `InstalledVersion`, `VersionComparison`, module version, and contract name.
- Exact zero matches → ID/source not verified.
- Exact one match with equal version → those dimensions verified.
- Multiple exact matches → indeterminate.
- Missing module, unsupported contract, missing copied property, or provider exception → indeterminate.
- Never parse localized tables or message substrings.

## Task 3 — Static recipe engine

Add reviewed built-in recipes and no user-supplied command surface.

First recipe: `Git.Git`.

- resolve `git.exe`/`git` from PATH;
- execute only `git --version` read-only;
- parse with built-in `GIT_VERSION_V1`;
- inspect PE architecture;
- classify scope only from explicit canonical user/system roots;
- return indeterminate when a dimension cannot be proven.

## Task 4 — Adapter capability

Add `winget.verify` to `toolos-winget-adapter`.

- Selector enters as typed JSON.
- Adapter invokes constant `powershell.exe -NoProfile -NonInteractive` provider script with selector JSON in a base64 environment variable.
- Adapter runs the static recipe independently.
- Provider module absence does not fall back to CLI table parsing.
- Persist bounded process evidence for provider and health probes.

## Task 5 — Daemon and execution integration

Add daemon RPC `winget.verify` and evidence kind.

After contained provider success and post-state capture:

- run verification against the immutable selector;
- embed verification report in execution report;
- map to `EXECUTION_VERIFIED_HEALTHY`, `EXECUTION_SUCCEEDED_UNVERIFIED`, or `EXECUTION_VERIFICATION_FAILED`;
- do not run verification for timeout, cancellation, provider failure, or unknown recovery.

## Task 6 — CLI/UI/schema

- Add CLI command `winget-verify`.
- Add verification controls/results to the WinGet panel.
- Display provider result, package identity, and application health separately.
- Add `winget-verification-report.schema.json` and validate in CI.
- Update execution schema for embedded verification.

## Task 7 — Windows tests

Permanent Windows CI must include:

- real `Git.Git` recipe against runner Git;
- wrong-version fixture;
- wrong-architecture PE fixture;
- wrong-scope path fixture;
- missing executable;
- provider identity verified + healthcheck unhealthy;
- official provider JSON fixture parser tests.

No test installs packages or modules.

## Task 8 — Documentation and final evidence

Update README/ADR/source ledger. Final gates:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
all JSON schemas
React typecheck/build
Tauri Windows compile
Windows process containment suite
Windows verification recipe suite
```

Close Issue #10 only on final-SHA evidence. Then parent PR #7 may leave Draft only if no new blocker is found during final forensic review.