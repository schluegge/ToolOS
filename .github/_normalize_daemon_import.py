from pathlib import Path

path = Path("apps/toolos-daemon/src/main.rs")
text = path.read_text(encoding="utf-8")
old = "use toolos_storage::{Storage, StoredActionPlan, StoredApprovalReceipt, StoredResourceLock};"
new = "use toolos_storage::{\n    Storage, StoredActionPlan, StoredApprovalReceipt, StoredResourceLock,\n};"
if text.count(old) != 1:
    raise SystemExit(f"expected one compact toolos_storage import, found {text.count(old)}")
path.write_text(text.replace(old, new, 1), encoding="utf-8")
print("daemon storage import normalized")
