use std;

use failure::{Error, ResultExt};

use remoteprocess::{ProcessMemory, Pid, Process};

use crate::python_interpreters::{InterpreterState, ThreadState, FrameObject, CodeObject, TupleObject};
use crate::python_data_access::{copy_string, copy_bytes};

/// Call stack for a single python thread
#[derive(Debug, Clone, Serialize)]
pub struct StackTrace {
    /// The process id than generated this stack trace
    pub pid: Pid,
    /// The python thread id for this stack trace
    pub thread_id: u64,
    /// The OS thread id for this stack tracee
    pub os_thread_id: Option<u64>,
    /// Whether or not the thread was active
    pub active: bool,
    /// Whether or not the thread held the GIL
    pub owns_gil: bool,
    /// The frames
    pub frames: Vec<Frame>
}

/// Information about a single function call in a stack trace
#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize)]
pub struct Frame {
    /// The function name
    pub name: String,
    /// The full filename of the file
    pub filename: String,
    /// The module/shared library the
    pub module: Option<String>,
    /// A short, more readable, representation of the filename
    pub short_filename: Option<String>,
    /// The line number inside the file (or 0 for native frames without line information)
    pub line: i32,
    /// Local Variables associated with the frame
    pub locals: Option<Vec<LocalVariable>>,
}

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize)]
pub struct LocalVariable {
    pub name: String,
    pub addr: usize,
    pub arg: bool,
    pub repr: Option<String>,
}

/// Given an InterpreterState, this function returns a vector of stack traces for each thread
pub fn get_stack_traces<I>(interpreter: &I, process: &Process) -> Result<(Vec<StackTrace>), Error>
        where I: InterpreterState {
    // TODO: deprecate this method
    let mut ret = Vec::new();
    let mut threads = interpreter.head();
    while !threads.is_null() {
        let thread = process.copy_pointer(threads).context("Failed to copy PyThreadState")?;
        ret.push(get_stack_trace(&thread, process, false)?);
        // This seems to happen occasionally when scanning BSS addresses for valid interpeters
        if ret.len() > 4096 {
            return Err(format_err!("Max thread recursion depth reached"));
        }
        threads = thread.next();
    }
    Ok(ret)
}

/// Gets a stack trace for an individual thread
pub fn get_stack_trace<T>(thread: &T, process: &Process, copy_locals: bool) -> Result<StackTrace, Error>
        where T: ThreadState {
    // TODO: just return frames here? everything else probably should be returned out of scope
    let mut frames = Vec::new();
    let mut frame_ptr = thread.frame();
    while !frame_ptr.is_null() {
        let frame = process.copy_pointer(frame_ptr).context("Failed to copy PyFrameObject")?;
        let code = process.copy_pointer(frame.code()).context("Failed to copy PyCodeObject")?;

        let filename = copy_string(code.filename(), process).context("Failed to copy filename")?;
        let name = copy_string(code.name(), process).context("Failed to copy function name")?;

        let line = match get_line_number(&code, frame.lasti(), process) {
            Ok(line) => line,
            Err(e) => {
                // Failling to get the line number really shouldn't be fatal here, but
                // can happen in extreme cases (https://github.com/benfred/py-spy/issues/164)
                // Rather than fail set the linenumber to 0. This is used by the native extensions
                // to indicate that we can't load a line number and it should be handled gracefully
                warn!("Failed to get line number from {}.{}: {}", filename, name, e);
                0
            }
        };

        let locals = if copy_locals {
            Some(get_locals(&code, frame_ptr, &frame, process)?)
        } else {
            None
        };

        frames.push(Frame{name, filename, line, short_filename: None, module: None, locals});
        if frames.len() > 4096 {
            return Err(format_err!("Max frame recursion depth reached"));
        }

        frame_ptr = frame.back();
    }

    Ok(StackTrace{pid: process.pid, frames, thread_id: thread.thread_id(), owns_gil: false, active: true, os_thread_id: None})
}

impl StackTrace {
    pub fn status_str(&self) -> &str {
        match (self.owns_gil, self.active) {
            (_, false) => "idle",
            (true, true) => "active+gil",
            (false, true) => "active",
        }
    }

    pub fn format_threadid(&self) -> String {
        // native threadids in osx are kinda useless, use the pthread id instead
        #[cfg(target_os="macos")]
        return format!("{:#X}", self.thread_id);

        // otherwise use the native threadid if given
        #[cfg(not(target_os="macos"))]
        match self.os_thread_id {
            Some(tid) => format!("{}", tid),
            None => format!("{:#X}", self.thread_id)
        }
    }
}

/// Returns the line number from a PyCodeObject (given the lasti index from a PyFrameObject)
fn get_line_number<C: CodeObject, P: ProcessMemory>(code: &C, lasti: i32, process: &P) -> Result<i32, Error> {
    let table = copy_bytes(code.lnotab(), process).context("Failed to copy line number table")?;

    // unpack the line table. format is specified here:
    // https://github.com/python/cpython/blob/master/Objects/lnotab_notes.txt
    let size = table.len();
    let mut i = 0;
    let mut line_number: i32 = code.first_lineno();
    let mut bytecode_address: i32 = 0;
    while (i + 1) < size {
        bytecode_address += i32::from(table[i]);
        if bytecode_address > lasti {
            break;
        }

        line_number += i32::from(table[i + 1]);
        i += 2;
    }

    Ok(line_number)
}

fn get_locals<C: CodeObject, F: FrameObject, P: ProcessMemory>(code: &C, frameptr: *const F, frame: &F, process: &P)
        -> Result<Vec<LocalVariable>, Error> {
    let local_count = code.nlocals() as usize;
    let argcount = code.argcount() as usize;
    let varnames = process.copy_pointer(code.varnames())?;

    let ptr_size = std::mem::size_of::<*const i32>();
    let locals_addr = frameptr as usize + std::mem::size_of_val(frame) - ptr_size;

    let mut ret = Vec::new();

    for i in 0..local_count {
        let nameptr: *const C::StringObject = process.copy_struct(varnames.address(code.varnames() as usize, i))?;
        let name = copy_string(nameptr, process)?;
        let addr: usize = process.copy_struct(locals_addr + i * ptr_size)?;
        if addr == 0 {
            continue;
        }
        ret.push(LocalVariable{name, addr, arg: i < argcount, repr: None});
    }
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use remoteprocess::LocalProcess;
    use python_bindings::v3_7_0::{PyCodeObject};
    use python_data_access::tests::to_byteobject;

    #[test]
    fn test_get_line_number() {
        let mut lnotab = to_byteobject(&[0u8, 1, 10, 1, 8, 1, 4, 1]);
        let code = PyCodeObject{co_firstlineno: 3,
                                co_lnotab: &mut lnotab.base.ob_base.ob_base,
                                ..Default::default()};
        let lineno = get_line_number(&code, 30, &LocalProcess).unwrap();
        assert_eq!(lineno, 7);
    }
}
