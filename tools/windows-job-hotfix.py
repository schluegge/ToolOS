from pathlib import Path

path = Path("crates/toolos-windows-job/src/windows.rs")
source = path.read_text(encoding="utf-8")
old = '''        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut read_handle = null_mut();
        let mut write_handle = null_mut();
        let created =
            unsafe { CreatePipe(&mut read_handle, &mut write_handle, &mut attributes, 0) };
'''
new = '''        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut read_handle = null_mut();
        let mut write_handle = null_mut();
        let created = unsafe { CreatePipe(&mut read_handle, &mut write_handle, &attributes, 0) };
'''
if source.count(old) != 1:
    raise SystemExit(f"expected exactly one CreatePipe marker, found {source.count(old)}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
