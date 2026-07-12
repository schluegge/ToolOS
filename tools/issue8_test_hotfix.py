from pathlib import Path

path = Path("apps/toolos-daemon/src/main.rs")
source = path.read_text(encoding="utf-8")
old = '''        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.execute.winget")
                && value.get("status").and_then(Value::as_str)
                    == Some("IMPLEMENTED_USER_SCOPE_PINNED")
        }));
'''
new = '''        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.execute.winget")
                && value.get("status").and_then(Value::as_str)
                    == Some("IMPLEMENTED_USER_SCOPE_PINNED_JOB_OBJECT")
        }));
        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.cancel.winget")
                && value.get("status").and_then(Value::as_str)
                    == Some("IMPLEMENTED_JOB_OBJECT")
        }));
'''
count = source.count(old)
if count != 1:
    raise RuntimeError(f"expected one capability assertion, found {count}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
