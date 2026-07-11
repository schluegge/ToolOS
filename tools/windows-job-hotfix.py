from pathlib import Path

path = Path("crates/toolos-windows-job/tests/process_tree.rs")
source = path.read_text(encoding="utf-8")
old = '    let child = spawn_fixture(&case, Duration::from_secs(2));\n'
new = '    let child = spawn_fixture(&case, Duration::from_millis(500));\n'
if source.count(old) != 1:
    raise SystemExit(f"expected exactly one timeout marker, found {source.count(old)}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
