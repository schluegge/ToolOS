from __future__ import annotations

import sys
from pathlib import Path


def replace_exact(source: str, old: str, new: str, label: str) -> str:
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, found {count}")
    return source.replace(old, new, 1)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: patch-hotfix.py <decompressed-patch-script>")
    target = Path(sys.argv[1])
    source = target.read_text(encoding="utf-8")

    old = r'''replace_once(
    "adapters/toolos-winget-adapter/src/main.rs",
    '                "package.preview.install",\n                "package.preview.uninstall"\n',
    '                "package.preview.install",\n                "package.preview.uninstall",\n                "package.install.execute.winget.user"\n',
)
'''
    new = r'''replace_once(
    "adapters/toolos-winget-adapter/src/main.rs",
    '            "probe": evidence,\n            "capabilities": [\n                "package.resolve.winget",\n                "package.installed.query.winget",\n                "package.preview.install",\n                "package.preview.uninstall"\n            ]\n',
    '            "probe": evidence,\n            "capabilities": [\n                "package.resolve.winget",\n                "package.installed.query.winget",\n                "package.preview.install",\n                "package.preview.uninstall",\n                "package.install.execute.winget.user"\n            ]\n',
)
'''
    source = replace_exact(source, old, new, "healthy WinGet capability marker")
    target.write_text(source, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
