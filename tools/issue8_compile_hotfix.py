from pathlib import Path

path = Path("apps/toolos-daemon/src/main.rs")
source = path.read_text(encoding="utf-8")
old = "use toolos_process::{ContainedProcessControl, ProcessContainmentEvidence, ProcessStopReason};\n"
new = "use toolos_process::{ContainedProcessControl, ProcessStopReason};\n"
count = source.count(old)
if count != 1:
    raise RuntimeError(f"expected one process import, found {count}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
