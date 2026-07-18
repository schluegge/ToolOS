from __future__ import annotations

import base64
import hashlib
import json
import shutil
import tarfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent
DESTINATION = ROOT / "source"
MANIFEST = ROOT / "source-manifest.json"
ARCHIVE_SHA256 = "e7e5d7385f29539943e37cd910114ea5a693621df9cb34eeecfdaad3ddcf03c9"


def validate_member_name(name: str) -> None:
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts:
        raise SystemExit(f"unsafe archive path: {name}")


def main() -> int:
    parts = sorted(ROOT.glob("source.tar.gz.b64.part*"))
    if not parts:
        raise SystemExit("source archive parts are missing")
    encoded = "".join(path.read_text(encoding="ascii").strip() for path in parts)
    archive_bytes = base64.b64decode(encoded, validate=True)
    actual_archive_sha = hashlib.sha256(archive_bytes).hexdigest()
    if actual_archive_sha != ARCHIVE_SHA256:
        raise SystemExit(
            f"archive sha256 mismatch: expected={ARCHIVE_SHA256} actual={actual_archive_sha}"
        )

    if DESTINATION.exists():
        shutil.rmtree(DESTINATION)
    DESTINATION.mkdir(parents=True)

    archive_path = ROOT / "source.tar.gz"
    archive_path.write_bytes(archive_bytes)
    try:
        with tarfile.open(archive_path, mode="r:gz") as archive:
            for member in archive.getmembers():
                validate_member_name(member.name)
            archive.extractall(DESTINATION, filter="data")
    finally:
        archive_path.unlink(missing_ok=True)

    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    expected_paths: set[str] = set()
    for record in data["files"]:
        relative = PurePosixPath(record["path"])
        validate_member_name(relative.as_posix())
        expected_paths.add(relative.as_posix())
        path = DESTINATION.joinpath(*relative.parts)
        content = path.read_bytes()
        if len(content) != record["size"]:
            raise SystemExit(f"size mismatch: {relative}")
        actual_sha = hashlib.sha256(content).hexdigest()
        if actual_sha != record["sha256"]:
            raise SystemExit(f"sha256 mismatch: {relative}")

    actual_paths = {
        path.relative_to(DESTINATION).as_posix()
        for path in DESTINATION.rglob("*")
        if path.is_file()
    }
    if actual_paths != expected_paths:
        raise SystemExit(
            "manifest coverage mismatch; "
            f"missing={sorted(expected_paths - actual_paths)}; "
            f"extra={sorted(actual_paths - expected_paths)}"
        )

    print(
        f"restored and verified {len(expected_paths)} exact source files; "
        f"source_commit={data['source_commit']}; "
        f"checkpoint_zip_sha256={data['checkpoint_zip_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
