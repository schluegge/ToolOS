from pathlib import Path

path = Path("apps/toolos-daemon/src/recovery_tests.rs")
source = path.read_text(encoding="utf-8")
replacements = [
    (
        "    let (_, state, report) = reconcile(ExecutionJournalPhase::SpawnIntent).await;\n",
        "    let (_seed, state, report) = reconcile(ExecutionJournalPhase::SpawnIntent).await;\n",
        "spawn-intent seed",
    ),
    (
        "    let (_, state, report) = reconcile(ExecutionJournalPhase::Spawned).await;\n",
        "    let (_seed, state, report) = reconcile(ExecutionJournalPhase::Spawned).await;\n",
        "spawned seed",
    ),
]
for old, new, label in replacements:
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    source = source.replace(old, new, 1)
path.write_text(source, encoding="utf-8", newline="\n")
