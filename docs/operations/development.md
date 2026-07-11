# Development and verification

## Windows prerequisites

Install or make available:

- Git;
- Rust stable with Cargo, rustfmt, and Clippy;
- Node.js 24 or another currently supported LTS release;
- Corepack and pnpm;
- Microsoft C++ Build Tools and WebView2 requirements documented by Tauri 2.

ToolOS does not silently elevate or install prerequisites in this implementation slice.

## Verify the repository

From the repository root in PowerShell:

```powershell
corepack enable
pnpm install
cargo build --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/toolos-ui check
pnpm --dir apps/toolos-ui build
cargo check --manifest-path apps/toolos-ui/src-tauri/Cargo.toml
```

Expected result: every command exits with code `0`.

## Run the daemon and CLI

Terminal 1:

```powershell
$env:RUST_LOG = "toolos=info"
cargo run -p toolos-daemon
```

Terminal 2:

```powershell
cargo run -p toolos-cli -- doctor
cargo run -p toolos-cli -- scan
cargo run -p toolos-cli -- inspect .
cargo run -p toolos-cli -- evidence --limit 20
cargo run -p toolos-cli -- events --limit 20
```

The daemon database defaults to:

```text
%LOCALAPPDATA%\ToolOS\toolos.db
```

Set `TOOLOS_DATA_DIR` to use another directory.

## Run the desktop UI

Keep the daemon running, then:

```powershell
pnpm --dir apps/toolos-ui tauri dev
```

The dashboard should show daemon health. Pressing **Scan this machine** must add a new evidence record. Selecting a project path and pressing **Inspect project** must return marker files without executing project code.

## Verification boundary

A successful frontend build does not prove the Windows desktop bundle launches. A successful Rust unit test does not prove local socket behavior on Windows. Release readiness therefore requires the separate clean Windows 11 VM test listed in the product roadmap.
