//! py-spy: a sampling profiler for python programs
//!
//! This crate lets you use py-spy as a rust library, and gather stack traces from
//! your python process programmatically.
//!
//! # Example:
//!
//! ```rust,no_run
//! fn print_python_stacks(pid: py_spy::Pid) -> Result<(), failure::Error> {
//!     // Create a new PythonSpy object with the default config options
//!     let config = py_spy::Config::default();
//!     let mut process = py_spy::PythonSpy::new(pid, &config)?;
//!
//!     // get stack traces for each thread in the process
//!     let traces = process.get_stack_traces()?;
//!
//!     // Print out the python stack for each thread
//!     for trace in traces {
//!         println!("Thread {:#X} ({})", trace.thread_id, trace.status_str());
//!         for frame in &trace.frames {
//!             println!("\t {} ({}:{})", frame.name, frame.filename, frame.line);
//!         }
//!     }
//!     Ok(())
//! }
//! ```

#[macro_use]
extern crate clap;
#[macro_use]
extern crate failure;
extern crate goblin;
#[macro_use]
extern crate lazy_static;
extern crate libc;
#[macro_use]
extern crate log;
#[cfg(unwind)]
extern crate lru;
extern crate memmap;
extern crate proc_maps;
extern crate regex;
#[macro_use]
extern crate serde_derive;
#[cfg(windows)]
extern crate winapi;
extern crate cpp_demangle;
extern crate rand;
extern crate remoteprocess;

mod config;
mod binary_parser;
#[cfg(unwind)]
mod cython;
#[cfg(unwind)]
mod native_stack_trace;
mod python_bindings;
mod python_interpreters;
mod python_spy;
mod python_data_access;
pub mod sampler;
mod stack_trace;
pub mod timer;
mod utils;
mod version;

pub use python_spy::PythonSpy;
pub use config::Config;
pub use stack_trace::StackTrace;
pub use stack_trace::Frame;
pub use remoteprocess::Pid;
