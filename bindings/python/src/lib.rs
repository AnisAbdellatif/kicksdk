//! kicksdk for Python: `kicksdk._native`, two entry points over the core's
//! dispatcher, and the core's reconnect `Backoff`. The Python package
//! (`python/kicksdk`) is the interface; this only converts between Python
//! objects and the dispatcher's JSON.

use kicksdk::dispatch::{self, DispatchError, SessionHandle};
use kicksdk::realtime::Backoff;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pythonize::{depythonize, pythonize};
use serde_json::Value;

create_exception!(kicksdk._native, NativeError, PyException, "A failed call: (kind, status, message).");

fn args(obj: Option<&Bound<'_, PyDict>>) -> PyResult<Value> {
    match obj {
        None => Ok(Value::Null),
        Some(d) => depythonize(d.as_any()).map_err(|e| NativeError::new_err(("call", None::<u16>, e.to_string()))),
    }
}

fn result(py: Python<'_>, r: Result<Value, DispatchError>) -> PyResult<Py<PyAny>> {
    match r {
        Ok(v) => {
            Ok(pythonize(py, &v).map_err(|e| NativeError::new_err(("call", None::<u16>, e.to_string())))?.unbind())
        }
        Err(e) => Err(NativeError::new_err((e.kind, e.status, e.message))),
    }
}

/// Calls one core function by name with keyword-style arguments.
#[pyfunction]
#[pyo3(signature = (name, args=None))]
fn call(py: Python<'_>, name: &str, args: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    let a = self::args(args)?;
    result(py, py.detach(|| dispatch::call(name, &a)))
}

/// A realtime session; `call(method, args)` runs one of its methods.
#[pyclass(module = "kicksdk._native")]
struct NativeSession(SessionHandle);

#[pymethods]
impl NativeSession {
    #[new]
    fn new(channels: Vec<String>) -> Self {
        Self(SessionHandle::new(channels))
    }

    #[pyo3(signature = (method, args=None))]
    fn call(&mut self, py: Python<'_>, method: &str, args: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        let a = self::args(args)?;
        result(py, self.0.call(method, &a))
    }
}

/// The core's reconnect delays, in milliseconds. Not in `dispatch`, which is
/// stateless functions and the session: the arithmetic lives in the core all
/// the same, so every language waits the same way.
#[pyclass(module = "kicksdk._native")]
struct NativeBackoff(Backoff);

#[pymethods]
impl NativeBackoff {
    #[new]
    fn new(initial_ms: u64, max_ms: u64) -> Self {
        Self(Backoff::new(initial_ms, max_ms))
    }

    #[getter]
    fn initial_ms(&self) -> u64 {
        self.0.initial_ms
    }

    #[getter]
    fn max_ms(&self) -> u64 {
        self.0.max_ms
    }

    fn next_delay(&mut self) -> u64 {
        self.0.next_delay()
    }

    fn next_delay_jittered(&mut self, random: f64) -> u64 {
        self.0.next_delay_jittered(random)
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(call, m)?)?;
    m.add_class::<NativeSession>()?;
    m.add_class::<NativeBackoff>()?;
    m.add("NativeError", m.py().get_type::<NativeError>())?;
    m.add("FUNCTIONS", dispatch::FUNCTIONS.to_vec())?;
    Ok(())
}
