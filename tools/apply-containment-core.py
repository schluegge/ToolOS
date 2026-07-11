from pathlib import Path

path = Path("apps/toolos-daemon/src/contained_adapter.rs")
source = path.read_text(encoding="utf-8")
replacements = [
    (
        "    pub stdout_truncated: bool,\n    pub stderr_truncated: bool,\n",
        "",
    ),
    (
        "            stdout_truncated: output.stdout_truncated,\n            stderr_truncated: output.stderr_truncated,\n",
        "",
    ),
]
for old, new in replacements:
    if source.count(old) != 1:
        raise SystemExit(f"expected exactly one marker, found {source.count(old)}: {old!r}")
    source = source.replace(old, new, 1)
path.write_text(source, encoding="utf-8", newline="\n")
