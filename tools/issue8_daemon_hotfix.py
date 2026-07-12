from pathlib import Path

path = Path("tools/issue8_daemon_integration.py")
source = path.read_text(encoding="utf-8")
old = '    "fn bounded_limit(\\n",\n'
new = '    "fn bounded_limit(",\n'
count = source.count(old)
if count != 1:
    raise RuntimeError(f"expected one bounded_limit marker, found {count}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
