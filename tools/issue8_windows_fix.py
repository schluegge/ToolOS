from pathlib import Path

path = Path("crates/toolos-process/src/windows.rs")
source = path.read_text(encoding="utf-8")

replacements = [
    (
        "use std::mem::{size_of, zeroed};",
        "use std::mem::{size_of, size_of_val};",
    ),
    (
        "    PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW, STILL_ACTIVE,\n",
        "    PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,\n",
    ),
    (
        "struct OwnedHandle(HANDLE);\n\nimpl OwnedHandle {\n    fn new(handle: HANDLE, operation: &str) -> Result<Self, ContainmentError> {\n        if handle.is_null() {\n            return Err(ContainmentError::ContainmentUnavailable(format!(\n                \"{operation} failed with Win32 error {}\",\n                last_error()\n            )));\n        }\n        Ok(Self(handle))\n    }\n\n    fn raw(&self) -> HANDLE {\n        self.0\n    }\n\n    fn into_raw(mut self) -> HANDLE {\n        let handle = self.0;\n        self.0 = null_mut();\n        handle\n    }\n}\n\nimpl Drop for OwnedHandle {\n    fn drop(&mut self) {\n        if !self.0.is_null() {\n            // SAFETY: this object uniquely owns the valid HANDLE until it is consumed.\n            unsafe {\n                CloseHandle(self.0);\n            }\n        }\n    }\n}",
        "struct OwnedHandle(usize);\n\nimpl OwnedHandle {\n    fn new(handle: HANDLE, operation: &str) -> Result<Self, ContainmentError> {\n        if handle.is_null() {\n            return Err(ContainmentError::ContainmentUnavailable(format!(\n                \"{operation} failed with Win32 error {}\",\n                last_error()\n            )));\n        }\n        Ok(Self(handle as usize))\n    }\n\n    fn raw(&self) -> HANDLE {\n        self.0 as HANDLE\n    }\n\n    fn into_raw(mut self) -> HANDLE {\n        let handle = self.raw();\n        self.0 = 0;\n        handle\n    }\n}\n\nimpl Drop for OwnedHandle {\n    fn drop(&mut self) {\n        if self.0 != 0 {\n            // SAFETY: this object uniquely owns the valid HANDLE until it is consumed.\n            unsafe {\n                CloseHandle(self.raw());\n            }\n        }\n    }\n}",
    ),
    (
        "    let mut active_after_cleanup = None;",
        "    let active_after_cleanup: Option<u32>;",
    ),
    (
        "    let mut attributes = SECURITY_ATTRIBUTES {",
        "    let attributes = SECURITY_ATTRIBUTES {",
    ),
    (
        "    // The API does not retain this structure.\n    attributes.lpSecurityDescriptor = null_mut();\n\n",
        "",
    ),
    (
        "fn process_exit_code(process: HANDLE) -> Result<Option<i32>, ContainmentError> {\n    let mut code = STILL_ACTIVE;",
        "fn process_exit_code(process: HANDLE) -> Result<Option<i32>, ContainmentError> {\n    let mut code = 0u32;",
    ),
    (
        "    if code == STILL_ACTIVE {\n        Ok(None)\n    } else {\n        Ok(Some(i32::from_ne_bytes(code.to_ne_bytes())))\n    }",
        "    Ok(Some(i32::from_ne_bytes(code.to_ne_bytes())))",
    ),
    (
        "    unsafe { File::from_raw_handle(raw.cast::<c_void>() as RawHandle) }",
        "    unsafe { File::from_raw_handle(raw as RawHandle) }",
    ),
]

for old, new in replacements:
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"expected exactly one replacement match, found {count}: {old[:100]!r}")
    source = source.replace(old, new, 1)

path.write_text(source, encoding="utf-8", newline="\n")
