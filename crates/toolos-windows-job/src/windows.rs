use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::ptr::{null, null_mut};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
    CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};

use crate::{
    duration_ms, CancellationReason, CancellationToken, ContainedOutput, ContainmentReport, JobError,
    ProcessSpec, Result, TerminationReason,
};

const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
const WAIT_SLICE: Duration = Duration::from_millis(50);
const TERMINATE_EXIT_CODE: u32 = 0x544f_4f4c;
const CONTAINMENT_METHOD: &str = "WINDOWS_JOB_OBJECT_PROC_THREAD_ATTRIBUTE_JOB_LIST";

#[derive(Debug)]
pub struct ContainedChild {
    spec: ProcessSpec,
    job: OwnedHandle,
    process: OwnedHandle,
    root_process_id: u32,
    stdout_reader: Option<JoinHandle<io::Result<CapturedOutput>>>,
    stderr_reader: Option<JoinHandle<io::Result<CapturedOutput>>>,
    started: Instant,
    completed: bool,
}

impl ContainedChild {
    pub fn spawn(spec: ProcessSpec) -> Result<Self> {
        validate_spec(&spec)?;

        let job = create_kill_on_close_job()?;
        let stdin_pipe = PipePair::new(PipeDirection::ParentWrites)?;
        let stdout_pipe = PipePair::new(PipeDirection::ParentReads)?;
        let stderr_pipe = PipePair::new(PipeDirection::ParentReads)?;

        let child_handles = [
            stdin_pipe.child.raw(),
            stdout_pipe.child.raw(),
            stderr_pipe.child.raw(),
        ];
        let job_handles = [job.raw()];
        let mut attributes = AttributeList::new(2)?;
        attributes.add(
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            job_handles.as_ptr().cast(),
            size_of::<HANDLE>() * job_handles.len(),
        )?;
        attributes.add(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            child_handles.as_ptr().cast(),
            size_of::<HANDLE>() * child_handles.len(),
        )?;

        let mut application = wide_nul(spec.executable.as_os_str(), "executable path")?;
        let mut command_line = command_line(&spec.executable, &spec.args)?;
        let current_directory = spec
            .executable
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| wide_nul(path.as_os_str(), "working directory"))
            .transpose()?;

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = stdin_pipe.child.raw();
        startup.StartupInfo.hStdOutput = stdout_pipe.child.raw();
        startup.StartupInfo.hStdError = stderr_pipe.child.raw();
        startup.lpAttributeList = attributes.as_mut_ptr();

        let mut process_information = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_mut_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                null(),
                current_directory
                    .as_ref()
                    .map_or(null(), |directory| directory.as_ptr()),
                &startup.StartupInfo,
                &mut process_information,
            )
        };
        if created == 0 {
            return Err(last_error("CreateProcessW"));
        }

        let process = OwnedHandle::new(process_information.hProcess);
        let thread_handle = OwnedHandle::new(process_information.hThread);
        let root_process_id = process_information.dwProcessId;
        drop(thread_handle);
        drop(attributes);
        drop(stdin_pipe.child);
        drop(stdout_pipe.child);
        drop(stderr_pipe.child);

        let stdout_reader = spawn_reader(stdout_pipe.parent.into_file(), spec.max_capture_bytes);
        let stderr_reader = spawn_reader(stderr_pipe.parent.into_file(), spec.max_capture_bytes);

        let mut stdin = stdin_pipe.parent.into_file();
        if let Err(error) = stdin.write_all(&spec.stdin).and_then(|()| stdin.flush()) {
            let _ = unsafe { TerminateJobObject(job.raw(), TERMINATE_EXIT_CODE) };
            return Err(JobError::Io(error));
        }
        drop(stdin);

        Ok(Self {
            spec,
            job,
            process,
            root_process_id,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            started: Instant::now(),
            completed: false,
        })
    }

    pub fn wait(mut self, cancellation: &CancellationToken) -> Result<ContainedOutput> {
        let outcome = loop {
            if let Some(reason) = cancellation.reason() {
                let termination_reason = match reason {
                    CancellationReason::ExplicitCancellation => {
                        TerminationReason::ExplicitCancellation
                    }
                    CancellationReason::DaemonShutdown => TerminationReason::DaemonShutdown,
                };
                break self.terminate_and_report(termination_reason, None);
            }

            if self.started.elapsed() >= self.spec.timeout {
                break self.terminate_and_report(TerminationReason::TimedOut, None);
            }

            let wait = unsafe {
                WaitForSingleObject(self.process.raw(), duration_to_wait_millis(WAIT_SLICE))
            };
            if wait == WAIT_OBJECT_0 {
                let exit_code = process_exit_code(self.process.raw()).ok();
                match wait_for_job_empty(self.job.raw(), self.spec.descendant_grace) {
                    Ok(Some(0)) => {
                        break WaitOutcome {
                            exit_code,
                            termination_reason: TerminationReason::ProcessExited,
                            termination_requested: false,
                            termination_confirmed: true,
                            active_processes_after: Some(0),
                            descendants_outlived_root: false,
                            detail: None,
                        };
                    }
                    Ok(active) => {
                        break self.terminate_and_report(
                            TerminationReason::DescendantsOutlivedRoot,
                            Some(format!(
                                "root process exited while {active:?} job processes remained"
                            )),
                        );
                    }
                    Err(error) => {
                        break self.terminate_and_report(
                            TerminationReason::ContainmentFailure,
                            Some(error.to_string()),
                        );
                    }
                }
            }
            if wait == WAIT_TIMEOUT {
                continue;
            }
            if wait == WAIT_FAILED {
                break self.terminate_and_report(
                    TerminationReason::ContainmentFailure,
                    Some(last_error("WaitForSingleObject").to_string()),
                );
            }
            break self.terminate_and_report(
                TerminationReason::ContainmentFailure,
                Some(format!("unexpected WaitForSingleObject result {wait}")),
            );
        };

        let (stdout, stderr) = if outcome.termination_confirmed {
            (
                join_reader(self.stdout_reader.take())?,
                join_reader(self.stderr_reader.take())?,
            )
        } else {
            self.stdout_reader.take();
            self.stderr_reader.take();
            (CapturedOutput::default(), CapturedOutput::default())
        };

        self.completed = true;
        Ok(ContainedOutput {
            exit_code: outcome.exit_code,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
            duration_ms: duration_ms(self.started.elapsed()),
            containment: ContainmentReport {
                method: CONTAINMENT_METHOD.to_owned(),
                root_process_id: self.root_process_id,
                kill_on_job_close: true,
                assigned_at_creation: true,
                inherited_handle_list_restricted: true,
                termination_reason: outcome.termination_reason,
                termination_requested: outcome.termination_requested,
                termination_confirmed: outcome.termination_confirmed,
                active_processes_after: outcome.active_processes_after,
                descendants_outlived_root: outcome.descendants_outlived_root,
                detail: outcome.detail,
            },
        })
    }

    fn terminate_and_report(
        &self,
        reason: TerminationReason,
        prior_detail: Option<String>,
    ) -> WaitOutcome {
        let mut detail = prior_detail;
        let terminated = unsafe { TerminateJobObject(self.job.raw(), TERMINATE_EXIT_CODE) };
        if terminated == 0 {
            detail = append_detail(detail, last_error("TerminateJobObject").to_string());
        }
        let confirmation = wait_for_job_empty(self.job.raw(), self.spec.termination_grace);
        match confirmation {
            Ok(active) => WaitOutcome {
                exit_code: None,
                termination_reason: reason,
                termination_requested: true,
                termination_confirmed: terminated != 0 && active == Some(0),
                active_processes_after: active,
                descendants_outlived_root: reason == TerminationReason::DescendantsOutlivedRoot,
                detail,
            },
            Err(error) => WaitOutcome {
                exit_code: None,
                termination_reason: reason,
                termination_requested: true,
                termination_confirmed: false,
                active_processes_after: None,
                descendants_outlived_root: reason == TerminationReason::DescendantsOutlivedRoot,
                detail: append_detail(detail, error.to_string()),
            },
        }
    }
}

impl Drop for ContainedChild {
    fn drop(&mut self) {
        if !self.completed {
            let _ = unsafe { TerminateJobObject(self.job.raw(), TERMINATE_EXIT_CODE) };
        }
    }
}

#[derive(Debug)]
struct WaitOutcome {
    exit_code: Option<u32>,
    termination_reason: TerminationReason,
    termination_requested: bool,
    termination_confirmed: bool,
    active_processes_after: Option<u32>,
    descendants_outlived_root: bool,
    detail: Option<String>,
}

#[derive(Debug, Default)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Debug)]
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    fn raw(&self) -> HANDLE {
        self.0
    }

    fn into_file(mut self) -> File {
        let handle = self.0;
        self.0 = null_mut();
        unsafe { File::from_raw_handle(handle as RawHandle) }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { CloseHandle(self.0) };
            self.0 = null_mut();
        }
    }
}

#[derive(Debug)]
struct PipePair {
    parent: OwnedHandle,
    child: OwnedHandle,
}

#[derive(Debug, Clone, Copy)]
enum PipeDirection {
    ParentReads,
    ParentWrites,
}

impl PipePair {
    fn new(direction: PipeDirection) -> Result<Self> {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut read_handle = null_mut();
        let mut write_handle = null_mut();
        let created = unsafe {
            CreatePipe(
                &mut read_handle,
                &mut write_handle,
                &mut attributes,
                0,
            )
        };
        if created == 0 {
            return Err(last_error("CreatePipe"));
        }
        let read_handle = OwnedHandle::new(read_handle);
        let write_handle = OwnedHandle::new(write_handle);
        let (parent, child) = match direction {
            PipeDirection::ParentReads => (read_handle, write_handle),
            PipeDirection::ParentWrites => (write_handle, read_handle),
        };
        let changed = unsafe { SetHandleInformation(parent.raw(), HANDLE_FLAG_INHERIT, 0) };
        if changed == 0 {
            return Err(last_error("SetHandleInformation"));
        }
        Ok(Self { parent, child })
    }
}

#[derive(Debug)]
struct AttributeList {
    _buffer: Vec<usize>,
    pointer: *mut core::ffi::c_void,
}

impl AttributeList {
    fn new(attribute_count: u32) -> Result<Self> {
        let mut bytes = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), attribute_count, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(last_error("InitializeProcThreadAttributeList(size)"));
        }
        let word_count = bytes.div_ceil(size_of::<usize>());
        let mut buffer = vec![0usize; word_count];
        let pointer = buffer.as_mut_ptr().cast();
        let initialized = unsafe {
            InitializeProcThreadAttributeList(pointer, attribute_count, 0, &mut bytes)
        };
        if initialized == 0 {
            return Err(last_error("InitializeProcThreadAttributeList"));
        }
        Ok(Self {
            _buffer: buffer,
            pointer,
        })
    }

    fn add(&mut self, attribute: usize, value: *const core::ffi::c_void, bytes: usize) -> Result<()> {
        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                attribute,
                value,
                bytes,
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            return Err(last_error("UpdateProcThreadAttribute"));
        }
        Ok(())
    }

    fn as_mut_ptr(&mut self) -> *mut core::ffi::c_void {
        self.pointer
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe { DeleteProcThreadAttributeList(self.pointer) };
            self.pointer = null_mut();
        }
    }
}

fn create_kill_on_close_job() -> Result<OwnedHandle> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(last_error("CreateJobObjectW"));
    }
    let job = OwnedHandle::new(job);
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(last_error("SetInformationJobObject"));
    }
    Ok(job)
}

fn query_active_processes(job: HANDLE) -> Result<u32> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(last_error("QueryInformationJobObject"));
    }
    Ok(accounting.ActiveProcesses)
}

fn wait_for_job_empty(job: HANDLE, timeout: Duration) -> Result<Option<u32>> {
    let started = Instant::now();
    loop {
        let active = query_active_processes(job)?;
        if active == 0 {
            return Ok(Some(0));
        }
        if started.elapsed() >= timeout {
            return Ok(Some(active));
        }
        thread::sleep(WAIT_SLICE);
    }
}

fn process_exit_code(process: HANDLE) -> Result<u32> {
    let mut exit_code = 0u32;
    let read = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    if read == 0 {
        return Err(last_error("GetExitCodeProcess"));
    }
    Ok(exit_code)
}

fn spawn_reader(file: File, max_capture_bytes: usize) -> JoinHandle<io::Result<CapturedOutput>> {
    thread::spawn(move || read_bounded(file, max_capture_bytes))
}

fn read_bounded(mut reader: File, max_capture_bytes: usize) -> io::Result<CapturedOutput> {
    let mut captured = Vec::with_capacity(max_capture_bytes.min(8192));
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = max_capture_bytes.saturating_sub(captured.len());
        let keep = remaining.min(read);
        captured.extend_from_slice(&buffer[..keep]);
        if keep < read {
            truncated = true;
        }
    }
    Ok(CapturedOutput {
        bytes: captured,
        truncated,
    })
}

fn join_reader(reader: Option<JoinHandle<io::Result<CapturedOutput>>>) -> Result<CapturedOutput> {
    reader
        .ok_or(JobError::ReaderThreadFailed)?
        .join()
        .map_err(|_| JobError::ReaderThreadFailed)?
        .map_err(JobError::Io)
}

fn validate_spec(spec: &ProcessSpec) -> Result<()> {
    if spec.executable.as_os_str().is_empty() {
        return Err(JobError::InvalidSpecification(
            "executable path is empty".to_owned(),
        ));
    }
    if spec.timeout.is_zero() {
        return Err(JobError::InvalidSpecification(
            "timeout must be greater than zero".to_owned(),
        ));
    }
    if spec.descendant_grace.is_zero() || spec.termination_grace.is_zero() {
        return Err(JobError::InvalidSpecification(
            "containment grace periods must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn command_line(executable: &std::path::Path, args: &[OsString]) -> Result<Vec<u16>> {
    let mut output = Vec::new();
    append_quoted_argument(&mut output, executable.as_os_str())?;
    for argument in args {
        output.push(u16::from(b' '));
        append_quoted_argument(&mut output, argument)?;
    }
    output.push(0);
    Ok(output)
}

fn append_quoted_argument(output: &mut Vec<u16>, argument: &OsStr) -> Result<()> {
    let units: Vec<u16> = argument.encode_wide().collect();
    if units.contains(&0) {
        return Err(JobError::InvalidSpecification(
            "process argument contains an interior NUL".to_owned(),
        ));
    }
    let quote = u16::from(b'"');
    let backslash = u16::from(b'\\');
    let needs_quotes = units.is_empty()
        || units
            .iter()
            .any(|unit| matches!(*unit, 0x20 | 0x09) || *unit == quote);
    if !needs_quotes {
        output.extend_from_slice(&units);
        return Ok(());
    }

    output.push(quote);
    let mut backslashes = 0usize;
    for unit in units {
        if unit == backslash {
            backslashes += 1;
            continue;
        }
        if unit == quote {
            output.extend(std::iter::repeat_n(backslash, backslashes * 2 + 1));
            output.push(quote);
        } else {
            output.extend(std::iter::repeat_n(backslash, backslashes));
            output.push(unit);
        }
        backslashes = 0;
    }
    output.extend(std::iter::repeat_n(backslash, backslashes * 2));
    output.push(quote);
    Ok(())
}

fn wide_nul(value: &OsStr, field: &str) -> Result<Vec<u16>> {
    let mut encoded: Vec<u16> = value.encode_wide().collect();
    if encoded.contains(&0) {
        return Err(JobError::InvalidSpecification(format!(
            "{field} contains an interior NUL"
        )));
    }
    encoded.push(0);
    Ok(encoded)
}

fn duration_to_wait_millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn last_error(operation: &'static str) -> JobError {
    let code = unsafe { GetLastError() };
    JobError::WindowsApi {
        operation,
        source: io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX)),
    }
}

fn append_detail(existing: Option<String>, new: String) -> Option<String> {
    Some(match existing {
        Some(existing) => format!("{existing}; {new}"),
        None => new,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(argument: &str) -> String {
        let mut output = Vec::new();
        append_quoted_argument(&mut output, OsStr::new(argument)).expect("quote argument");
        String::from_utf16(&output).expect("valid UTF-16")
    }

    #[test]
    fn windows_command_line_quotes_spaces_and_embedded_quotes() {
        assert_eq!(rendered("plain"), "plain");
        assert_eq!(rendered("two words"), "\"two words\"");
        assert_eq!(rendered("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn cancellation_is_first_writer_wins() {
        let token = CancellationToken::default();
        assert!(token.cancel(CancellationReason::ExplicitCancellation));
        assert!(!token.cancel(CancellationReason::DaemonShutdown));
        assert_eq!(token.reason(), Some(CancellationReason::ExplicitCancellation));
    }
}
