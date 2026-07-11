# ToolOS agent instructions

ToolOS is built in roadmap order. The current implemented scope is Milestone A plus the first read-only Phase 1 capability.

Rules:

1. Reuse a maintained operating-system facility, CLI, API, protocol, or library before writing a replacement.
2. Do not execute repository-provided scripts during discovery.
3. Every factual machine/project claim must be represented by structured evidence.
4. Write-capable actions require a simulation, declared blast radius, approval, and recovery story before implementation.
5. Keep UI, CLI, and daemon on the same contracts; do not create UI-only behavior.
6. Do not add fake controls. Every visible control must call an implemented daemon method.
7. Do not claim a test passed without its command output or CI run.
8. Prefer one recommended path. Preserve raw commands, logs, and evidence for expert inspection.

Normal verification:

```text
mise run verify
```

When mise is unavailable, run the commands listed in `docs/operations/development.md`.
