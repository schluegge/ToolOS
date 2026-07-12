from pathlib import Path

path = Path("crates/toolos-winget/src/lib.rs")
source = path.read_text(encoding="utf-8")
old = "mod execution;\nmod recovery;\npub use execution::*;\npub use recovery::*;\n"
new = "mod execution;\nmod recovery;\nmod verification;\npub use execution::*;\npub use recovery::*;\npub use verification::*;\n"
if source.count(old) != 1:
    raise RuntimeError(f"verification export marker: expected one match, found {source.count(old)}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
