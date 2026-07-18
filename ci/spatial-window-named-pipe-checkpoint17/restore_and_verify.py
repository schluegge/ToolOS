from __future__ import annotations

import base64
import hashlib
import json
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ARCHIVE_SHA256 = "308d333549cb9b3311fee4b70e872b8dc786e167f46c588a2ee2301c7ecb4da0"


def safe_extract(archive: tarfile.TarFile, destination: Path) -> None:
    root = destination.resolve()
    for member in archive.getmembers():
        target = (destination / member.name).resolve()
        if root != target and root not in target.parents:
            raise SystemExit(f"unsafe archive path: {member.name}")
    archive.extractall(destination)


def main() -> int:
    encoded = "".join(
        path.read_text(encoding="ascii").strip()
        for path in sorted(ROOT.glob("source.tar.gz.b64.part*"))
    )
    payload = base64.b64decode(encoded, validate=True)
    if hashlib.sha256(payload).hexdigest() != ARCHIVE_SHA256:
        raise SystemExit("archive sha256 mismatch")
    archive_path = ROOT / "source.tar.gz"
    archive_path.write_bytes(payload)
    with tarfile.open(archive_path, mode="r:gz") as archive:
        safe_extract(archive, ROOT)
    archive_path.unlink()

    data = json.loads((ROOT / "source-manifest.json").read_text(encoding="utf-8"))
    expected_paths = set()
    for record in data["files"]:
        relative = Path(record["path"])
        expected_paths.add(relative.as_posix())
        path = ROOT / relative
        content = path.read_bytes()
        if len(content) != record["size"]:
            raise SystemExit(f"size mismatch: {relative}")
        if hashlib.sha256(content).hexdigest() != record["sha256"]:
            raise SystemExit(f"sha256 mismatch: {relative}")
    actual_paths = {
        path.relative_to(ROOT).as_posix()
        for path in (ROOT / "source").rglob("*")
        if path.is_file()
    }
    if actual_paths != expected_paths:
        raise SystemExit(
            f"manifest coverage mismatch; missing={sorted(expected_paths-actual_paths)}; "
            f"extra={sorted(actual_paths-expected_paths)}"
        )
    print(f"restored and verified {len(expected_paths)} exact source files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
