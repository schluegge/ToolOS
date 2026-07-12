from pathlib import Path

path = Path("apps/toolos-daemon/src/main.rs")
source = path.read_text(encoding="utf-8")
old = "mod contained_adapter;\nmod recovery;\n"
new = "mod contained_adapter;\nmod recovery;\n#[cfg(test)]\nmod recovery_tests;\n"
if source.count(old) != 1:
    raise RuntimeError(f"recovery test module marker: expected one match, found {source.count(old)}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
