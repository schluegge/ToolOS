use std::ffi::{c_void, OsStr};
use std::fs::File;
use std::io::{Read, Write};
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;
use uuid::Uuid;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_FAILED,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::{
    ContainedCommandSpec, ContainedProcessOutput, ContainmentError, ProcessContainmentEvidence,
    ProcessStopReason,
};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const TERMINATION_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_TRUNCATION_MARKER: &str = "\n[ToolOS truncated contained process output]";
const STOP_NONE: u8 = 0;
const STOP_TIMED_OUT: u8 = 1;
const STOP_CANCELLED: u8 = 2;
const STOP_DAEMON_SHUTDOWN: u8 = 3;
const STOP_CONTAINMENT_FAILED: u8 = 4;

#[derive(Debug)]
struct OwnedHandle(usize);

impl OwnedHandle {
    fn new(handle: HANDLE, operation: &str) -> Result<Self, ContainmentError> {
        if handle.is_null() {
            return Err(ContainmentError::ContainmentUnavailable(format!(
                "{operation} failed with Win32 error {}",
                last_error()
            )));
        }
        Ok(Self(handle as usize))
    }

    fn raw(&self) -> HANDLE {
        self.0 as HANDLE
    }

    fn into_raw(mut self) -> HANDLE {
        let handle = self.raw();
        self.0 = 0;
        handle
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: this object uniquely owns the valid HANDLE until it is consumed.
            unsafe {
                CloseHandle(self.raw());
            }
        }
    }
}

#[derive(Debug)]
struct AttributeList {
    _storage: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttributeList {
    fn new(attribute_count: u32) -> Result<Self, ContainmentError> {
        let mut bytes = 0usize;
        // SAFETY: a null first call is the documented size-query pattern.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), attribute_count, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(ContainmentError::ContainmentUnavailable(format!(
                "InitializeProcThreadAttributeList size query failed with Win32 error {}",
                last_error()
            )));
        }

        let words = bytes.div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; words];
        let pointer = storage.as_mut_ptr().cast::<c_void>();
        // SAFETY: storage is writable, pointer-aligned, and at least the requested size.
        let initialized =
            unsafe { InitializeProcThreadAttributeList(pointer, attribute_count, 0, &mut bytes) };
        if initialized == 0 {
            return Err(ContainmentError::ContainmentUnavailable(format!(
                "InitializeProcThreadAttributeList failed with Win32 error {}",
                last_error()
            )));
        }

        Ok(Self {
            _storage: storage,
            pointer,
        })
    }

    fn update<T>(&mut self, attribute: usize, values: &[T]) -> Result<(), ContainmentError> {
        if values.is_empty() {
            return Err(ContainmentError::ContainmentUnavailable(
                "process attribute value list cannot be empty".to_owned(),
            ));
        }
        // SAFETY: pointer references an initialized list and values lives through the call.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                attribute,
                values.as_ptr().cast::<c_void>(),
                size_of_val(values),
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            return Err(ContainmentError::ContainmentUnavailable(format!(
                "UpdateProcThreadAttribute({attribute}) failed with Win32 error {}",
                last_error()
            )));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: pointer was initialized successfully and remains backed by storage.
        unsafe {
            DeleteProcThreadAttributeList(self.pointer);
        }
    }
}

#[derive(Debug)]
struct PipeEnds {
    parent: OwnedHandle,
    child: OwnedHandle,
}

#[derive(Debug)]
struct ControlInner {
    job: OwnedHandle,
    requested_stop: AtomicU8,
}

#[derive(Debug, Clone)]
pub struct ContainedProcessControl {
    inner: Arc<ControlInner>,
}

impl ContainedProcessControl {
    pub fn cancel(&self, reason: ProcessStopReason) -> Result<(), ContainmentError> {
        let requested = stop_code(reason);
        if requested == STOP_NONE {
            return Ok(());
        }
        let previous = self.inner.requested_stop.compare_exchange(
            STOP_NONE,
            requested,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if previous.is_err() {
            return Ok(());
        }
        // SAFETY: the Arc keeps the Job Object HANDLE alive for this call.
        let terminated = unsafe { TerminateJobObject(self.inner.job.raw(), 1) };
        if terminated == 0 {
            self.inner
                .requested_stop
                .store(STOP_CONTAINMENT_FAILED, Ordering::Release);
            return Err(ContainmentError::TerminationFailed(format!(
                "TerminateJobObject failed with Win32 error {}",
                last_error()
            )));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ContainedProcess {
    execution_id: Uuid,
    root_pid: u32,
    control: ContainedProcessControl,
    completion: oneshot::Receiver<Result<ContainedProcessOutput, ContainmentError>>,
}

impl ContainedProcess {
    #[must_use]
    pub fn execution_id(&self) -> Uuid {
        self.execution_id
    }

    #[must_use]
    pub fn root_pid(&self) -> u32 {
        self.root_pid
    }

    #[must_use]
    pub fn control(&self) -> ContainedProcessControl {
        self.control.clone()
    }

    pub async fn wait(self) -> Result<ContainedProcessOutput, ContainmentError> {
        self.completion
            .await
            .map_err(|_| ContainmentError::CompletionChannelClosed)?
    }
}

pub fn spawn_contained(spec: ContainedCommandSpec) -> Result<ContainedProcess, ContainmentError> {
    let execution_id = Uuid::new_v4();
    let started = Instant::now();

    // SAFETY: null security attributes and name are accepted by CreateJobObjectW.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    let job = OwnedHandle::new(job, "CreateJobObjectW")?;

    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: limits points to the exact structure required by this information class.
    let configured = unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .expect("job limit structure fits u32"),
        )
    };
    if configured == 0 {
        return Err(ContainmentError::ContainmentUnavailable(format!(
            "SetInformationJobObject failed with Win32 error {}",
            last_error()
        )));
    }

    let stdin_pipe = create_pipe(true)?;
    let stdout_pipe = create_pipe(false)?;
    let stderr_pipe = create_pipe(false)?;

    let mut attributes = AttributeList::new(2)?;
    attributes.update(PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, &[job.raw()])?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
        &[
            stdin_pipe.child.raw(),
            stdout_pipe.child.raw(),
            stderr_pipe.child.raw(),
        ],
    )?;

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb =
        u32::try_from(size_of::<STARTUPINFOEXW>()).expect("startup structure fits u32");
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin_pipe.child.raw();
    startup.StartupInfo.hStdOutput = stdout_pipe.child.raw();
    startup.StartupInfo.hStdError = stderr_pipe.child.raw();
    startup.lpAttributeList = attributes.pointer;

    let application = wide_null(&spec.executable);
    let mut command_line = command_line(&spec.executable, &spec.args);
    let mut process_information = PROCESS_INFORMATION::default();
    // SAFETY: all pointers reference live, correctly initialized buffers and structures.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            null(),
            null(),
            &startup.StartupInfo,
            &mut process_information,
        )
    };
    if created == 0 {
        return Err(ContainmentError::SpawnFailed(format!(
            "CreateProcessW failed with Win32 error {}",
            last_error()
        )));
    }

    let process = OwnedHandle::new(
        process_information.hProcess,
        "CreateProcessW process handle",
    )?;
    let thread_handle = OwnedHandle::new(
        process_information.hThread,
        "CreateProcessW primary thread handle",
    )?;
    drop(thread_handle);
    drop(attributes);
    drop(stdin_pipe.child);
    drop(stdout_pipe.child);
    drop(stderr_pipe.child);

    let root_pid = process_information.dwProcessId;
    let stdin_file = file_from_owned_handle(stdin_pipe.parent);
    let stdout_file = file_from_owned_handle(stdout_pipe.parent);
    let stderr_file = file_from_owned_handle(stderr_pipe.parent);

    let control = ContainedProcessControl {
        inner: Arc::new(ControlInner {
            job,
            requested_stop: AtomicU8::new(STOP_NONE),
        }),
    };
    let waiter_control = control.clone();
    let (sender, receiver) = oneshot::channel();

    thread::Builder::new()
        .name(format!("toolos-contained-{execution_id}"))
        .spawn(move || {
            let result = wait_for_completion(
                execution_id,
                root_pid,
                process,
                waiter_control,
                stdin_file,
                stdout_file,
                stderr_file,
                spec,
                started,
            );
            let _ = sender.send(result);
        })
        .map_err(|error| ContainmentError::SpawnFailed(error.to_string()))?;

    Ok(ContainedProcess {
        execution_id,
        root_pid,
        control,
        completion: receiver,
    })
}

#[allow(clippy::too_many_arguments)]
fn wait_for_completion(
    execution_id: Uuid,
    root_pid: u32,
    process: OwnedHandle,
    control: ContainedProcessControl,
    mut stdin_file: File,
    stdout_file: File,
    stderr_file: File,
    spec: ContainedCommandSpec,
    started: Instant,
) -> Result<ContainedProcessOutput, ContainmentError> {
    let stdin = spec.stdin;
    let stdin_writer = thread::spawn(move || {
        if !stdin.is_empty() {
            let _ = stdin_file.write_all(&stdin);
            let _ = stdin_file.flush();
        }
    });
    let stdout_limit = spec.max_stdout_bytes;
    let stderr_limit = spec.max_stderr_bytes;
    let stdout_reader = thread::spawn(move || read_bounded(stdout_file, stdout_limit));
    let stderr_reader = thread::spawn(move || read_bounded(stderr_file, stderr_limit));

    let mut root_exited = false;
    let mut exit_code = None;
    let mut termination_requested_at = None;
    let mut containment_confirmed = false;
    let active_after_cleanup: Option<u32>;

    loop {
        // SAFETY: process HANDLE remains owned for the duration of this loop.
        let wait = unsafe { WaitForSingleObject(process.raw(), 50) };
        if wait == WAIT_OBJECT_0 {
            root_exited = true;
            exit_code = process_exit_code(process.raw())?;
        } else if wait == WAIT_FAILED {
            control
                .inner
                .requested_stop
                .store(STOP_CONTAINMENT_FAILED, Ordering::Release);
            return Err(ContainmentError::WaitFailed(format!(
                "WaitForSingleObject failed with Win32 error {}",
                last_error()
            )));
        } else if wait != WAIT_TIMEOUT {
            control
                .inner
                .requested_stop
                .store(STOP_CONTAINMENT_FAILED, Ordering::Release);
            return Err(ContainmentError::WaitFailed(format!(
                "WaitForSingleObject returned unexpected value {wait}"
            )));
        }

        let active = query_active_processes(control.inner.job.raw())?;
        if root_exited && active == 0 {
            containment_confirmed = true;
            active_after_cleanup = Some(0);
            break;
        }

        if control.inner.requested_stop.load(Ordering::Acquire) == STOP_NONE
            && started.elapsed() >= spec.timeout
        {
            control.cancel(ProcessStopReason::TimedOut)?;
            termination_requested_at = Some(Instant::now());
        } else if control.inner.requested_stop.load(Ordering::Acquire) != STOP_NONE
            && termination_requested_at.is_none()
        {
            termination_requested_at = Some(Instant::now());
        }

        if let Some(requested_at) = termination_requested_at {
            if active == 0 {
                containment_confirmed = true;
                active_after_cleanup = Some(0);
                break;
            }
            if requested_at.elapsed() >= TERMINATION_CONFIRMATION_TIMEOUT {
                active_after_cleanup = Some(active);
                control
                    .inner
                    .requested_stop
                    .store(STOP_CONTAINMENT_FAILED, Ordering::Release);
                break;
            }
        }

        thread::sleep(POLL_INTERVAL);
    }

    drop(process);
    let _ = stdin_writer.join();
    let stdout = stdout_reader
        .join()
        .map_err(|_| ContainmentError::WaitFailed("stdout reader thread panicked".to_owned()))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| ContainmentError::WaitFailed("stderr reader thread panicked".to_owned()))?;
    let stop_reason = stop_reason(control.inner.requested_stop.load(Ordering::Acquire));
    let detail = if containment_confirmed {
        format!(
            "Windows Job Object accounting confirmed zero active processes after {stop_reason:?}"
        )
    } else {
        format!(
            "Windows Job Object accounting did not confirm zero active processes; observed {:?}",
            active_after_cleanup
        )
    };

    Ok(ContainedProcessOutput {
        execution_id,
        root_pid,
        exit_code,
        stdout,
        stderr,
        duration_ms: duration_ms(started.elapsed()),
        containment: ProcessContainmentEvidence {
            method: "WINDOWS_JOB_OBJECT_STARTUP_ATTRIBUTE".to_owned(),
            root_pid: Some(root_pid),
            stop_reason,
            active_processes_after_cleanup: active_after_cleanup,
            descendants_terminated: containment_confirmed.then_some(true),
            containment_confirmed,
            detail,
        },
    })
}

fn create_pipe(parent_writes: bool) -> Result<PipeEnds, ContainmentError> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
            .expect("security attributes fit u32"),
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read_handle: HANDLE = null_mut();
    let mut write_handle: HANDLE = null_mut();
    // SAFETY: output pointers and security attributes are valid for this call.
    let created = unsafe {
        CreatePipe(
            &mut read_handle,
            &mut write_handle,
            &raw const attributes,
            0,
        )
    };
    if created == 0 {
        return Err(ContainmentError::ContainmentUnavailable(format!(
            "CreatePipe failed with Win32 error {}",
            last_error()
        )));
    }
    let read = OwnedHandle::new(read_handle, "CreatePipe read handle")?;
    let write = OwnedHandle::new(write_handle, "CreatePipe write handle")?;
    let parent_handle = if parent_writes {
        write.raw()
    } else {
        read.raw()
    };
    // SAFETY: parent_handle is a valid pipe HANDLE; clearing inheritance is documented.
    let cleared = unsafe { SetHandleInformation(parent_handle, HANDLE_FLAG_INHERIT, 0) };
    if cleared == 0 {
        return Err(ContainmentError::ContainmentUnavailable(format!(
            "SetHandleInformation failed with Win32 error {}",
            last_error()
        )));
    }

    if parent_writes {
        Ok(PipeEnds {
            parent: write,
            child: read,
        })
    } else {
        Ok(PipeEnds {
            parent: read,
            child: write,
        })
    }
}

fn query_active_processes(job: HANDLE) -> Result<u32, ContainmentError> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: accounting is the exact structure required by this information class.
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                .expect("accounting structure fits u32"),
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(ContainmentError::WaitFailed(format!(
            "QueryInformationJobObject failed with Win32 error {}",
            last_error()
        )));
    }
    Ok(accounting.ActiveProcesses)
}

fn process_exit_code(process: HANDLE) -> Result<Option<i32>, ContainmentError> {
    let mut code = 0u32;
    // SAFETY: process is a valid process HANDLE and code is writable.
    let read = unsafe { GetExitCodeProcess(process, &mut code) };
    if read == 0 {
        return Err(ContainmentError::WaitFailed(format!(
            "GetExitCodeProcess failed with Win32 error {}",
            last_error()
        )));
    }
    Ok(Some(i32::from_ne_bytes(code.to_ne_bytes())))
}

fn file_from_owned_handle(handle: OwnedHandle) -> File {
    let raw = handle.into_raw();
    // SAFETY: ownership is transferred exactly once from OwnedHandle to File.
    unsafe { File::from_raw_handle(raw as RawHandle) }
}

fn read_bounded(mut file: File, limit: usize) -> String {
    let mut retained = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let remaining = limit.saturating_sub(retained.len());
                let kept = read.min(remaining);
                retained.extend_from_slice(&buffer[..kept]);
                truncated |= kept < read;
            }
            Err(_) => break,
        }
    }
    let mut text = String::from_utf8_lossy(&retained).into_owned();
    if truncated {
        text.push_str(OUTPUT_TRUNCATION_MARKER);
    }
    text
}

fn command_line(executable: &OsStr, args: &[std::ffi::OsString]) -> Vec<u16> {
    let mut command = quote_windows_arg(executable);
    for argument in args {
        command.push(' ');
        command.push_str(&quote_windows_arg(argument));
    }
    command.encode_utf16().chain([0]).collect()
}

fn quote_windows_arg(argument: &OsStr) -> String {
    let value = argument.to_string_lossy();
    if !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return value.into_owned();
    }

    let mut output = String::from("\"");
    let mut backslashes = 0usize;
    for character in value.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                output.push_str(&"\\".repeat(backslashes * 2 + 1));
                output.push('"');
                backslashes = 0;
            }
            _ => {
                output.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                output.push(character);
            }
        }
    }
    output.push_str(&"\\".repeat(backslashes * 2));
    output.push('"');
    output
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

fn stop_code(reason: ProcessStopReason) -> u8 {
    match reason {
        ProcessStopReason::Exited => STOP_NONE,
        ProcessStopReason::TimedOut => STOP_TIMED_OUT,
        ProcessStopReason::Cancelled => STOP_CANCELLED,
        ProcessStopReason::DaemonShutdown => STOP_DAEMON_SHUTDOWN,
        ProcessStopReason::ContainmentFailed => STOP_CONTAINMENT_FAILED,
    }
}

fn stop_reason(code: u8) -> ProcessStopReason {
    match code {
        STOP_TIMED_OUT => ProcessStopReason::TimedOut,
        STOP_CANCELLED => ProcessStopReason::Cancelled,
        STOP_DAEMON_SHUTDOWN => ProcessStopReason::DaemonShutdown,
        STOP_CONTAINMENT_FAILED => ProcessStopReason::ContainmentFailed,
        _ => ProcessStopReason::Exited,
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn last_error() -> u32 {
    // SAFETY: GetLastError has no preconditions.
    unsafe { GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_argument_quoting_matches_create_process_rules() {
        assert_eq!(quote_windows_arg(OsStr::new("")), "\"\"");
        assert_eq!(quote_windows_arg(OsStr::new("plain")), "plain");
        assert_eq!(quote_windows_arg(OsStr::new("a b")), "\"a b\"");
        assert_eq!(quote_windows_arg(OsStr::new("a\"b")), "\"a\\\"b\"");
        assert_eq!(quote_windows_arg(OsStr::new("a b\\")), "\"a b\\\\\"");
    }

    #[test]
    fn stop_reason_round_trips() {
        for reason in [
            ProcessStopReason::Exited,
            ProcessStopReason::TimedOut,
            ProcessStopReason::Cancelled,
            ProcessStopReason::DaemonShutdown,
            ProcessStopReason::ContainmentFailed,
        ] {
            assert_eq!(stop_reason(stop_code(reason)), reason);
        }
    }
}
