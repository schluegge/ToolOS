# ToolOS

ToolOS is a Windows-first, ecosystem-wide local control plane for non-coders. It turns a goal into a transparent, evidence-backed sequence of actions across existing tools instead of rebuilding those tools.

## Implemented vertical slice

This repository currently implements the platform skeleton and a useful read-only discovery path:

- `toolos-daemon`: user-scoped Rust daemon with local socket IPC and SQLite state.
- `toolos`: CLI for health, system scan, project inspection, evidence, and event replay.
- `toolos-launcher`: starts the daemon when necessary and launches the desktop client.
- `toolos-system-adapter`: out-of-process JSON-RPC adapter that inspects the machine and project without running repository scripts.
- `toolos-ui`: Tauri 2 + React guided dashboard using the same daemon API.
- Typed capability, evidence, adapter, action, approval, quota, lesson, and policy schemas.
- Correlated event and evidence persistence.

This is not the complete twelve-phase product. The implemented scope is Milestone A plus the first read-only part of Milestone B.

## What the scan does

The system scan detects the host OS/architecture and whether representative executables are resolvable on `PATH`, including Git, Rust, Node, Python, PowerShell, WinGet, WSL, Docker, Podman, Ollama, and several coding-agent CLIs. It does not execute those tools and does not read browser profiles, secrets, AppData content, or repository install hooks.

The project inspector checks the selected directory identity and known stack marker files such as `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, solution/project files, and common agent instruction files.

## Development

Prerequisites are Rust stable, Node.js, pnpm, and platform requirements for Tauri 2.

```powershell
corepack enable
pnpm install
cargo build --workspace
cargo test --workspace
pnpm --dir apps/toolos-ui build
cargo run -p toolos-daemon
```

In another terminal:

```powershell
cargo run -p toolos-cli -- doctor
cargo run -p toolos-cli -- scan
cargo run -p toolos-cli -- inspect .
cargo run -p toolos-cli -- evidence
```

Run the Tauri desktop shell:

```powershell
pnpm --dir apps/toolos-ui tauri dev
```

Detailed Windows setup and verification are in [`docs/operations/development.md`](docs/operations/development.md).

## Architecture

The UI and CLI never perform unrestricted shell actions. They send typed JSON-RPC requests over a local socket to the daemon. The daemon owns policy, evidence, persistence, and adapter supervision. The system adapter is a separate process using line-delimited JSON-RPC over stdio.

See [`docs/architecture/ADR-0001-platform.md`](docs/architecture/ADR-0001-platform.md).

## Safety boundary

The current release is read-only. No installation, deletion, billing, credential extraction, browser stealth, CAPTCHA bypass, or arbitrary repository execution is implemented.
