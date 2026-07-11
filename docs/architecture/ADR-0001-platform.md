# ADR-0001: ToolOS platform architecture

- Status: accepted for the first implementation slice
- Scope: Milestone A

## Decision

ToolOS uses a Rust control plane with four user-facing/runtime boundaries:

1. a user-scoped daemon owning state, evidence, policy, adapter supervision, and local IPC;
2. a CLI using the same daemon contracts;
3. a Tauri 2 desktop client with React/TypeScript;
4. out-of-process adapters using versioned JSON-RPC.

The daemon and CLI use cross-platform local sockets through `interprocess`. SQLite is accessed through `rusqlite`; migrations are applied atomically through `rusqlite_migration`. The Tauri frontend invokes registered Rust commands and does not receive arbitrary shell access.

## Why

A daemon survives UI restarts and provides one place for locks, workflow state, evidence, quotas, cancellation, and crash recovery. Out-of-process adapters allow reuse of existing ecosystems without importing their dependency or crash surface into the core. The provider boundary prevents ToolOS from becoming coupled to one package manager, model runner, agent harness, or operating environment.

## Current capability

`machine.inspect` and `project.inspect` are the first provider-independent, read-only capabilities. The `toolos-system-adapter` provider implements them without running discovered tools or repository scripts.

## Deferred

The following are intentionally not claimed complete:

- signed distribution and update verification;
- authenticated per-user IPC beyond operating-system local-socket isolation;
- write workflows, approvals, locks, rollback, and cancellation;
- installation/runtime/package-manager adapters;
- MCP, model, browser, desktop, and cloud providers;
- full Windows 11 clean-VM acceptance proof.

These remain roadmap work rather than placeholders represented as working controls.
