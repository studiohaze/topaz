use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use topaz_rt::{Host, ResourceId};

mod emitted;

#[derive(Default)]
struct WebHost {
    prints: RefCell<Vec<String>>,
    deferred_errors: RefCell<Vec<String>>,
    input: String,
}

impl WebHost {
    fn new(input: String) -> Self {
        Self {
            prints: RefCell::new(Vec::new()),
            deferred_errors: RefCell::new(Vec::new()),
            input,
        }
    }

    fn stdout(&self) -> Vec<String> {
        self.prints.borrow().clone()
    }

    fn defer_errors(&self) -> Vec<String> {
        self.deferred_errors.borrow().clone()
    }
}

impl Host for WebHost {
    fn print(&self, line: &str) {
        self.prints.borrow_mut().push(line.to_string());
    }

    fn open(&self, _path: &str) -> Result<ResourceId, String> {
        Err("web target PW1 has no filesystem host".to_string())
    }

    fn read(&self, _handle: ResourceId) -> Result<String, String> {
        Err("web target PW1 has no filesystem host".to_string())
    }

    fn write(&self, _handle: ResourceId, _s: &str) -> Result<(), String> {
        Err("web target PW1 has no filesystem host".to_string())
    }

    fn close(&self, _handle: ResourceId) {}

    fn now_millis(&self) -> u64 {
        0
    }

    fn defer_error(&self, rendered: &str) {
        self.deferred_errors.borrow_mut().push(rendered.to_string());
    }

    fn input(&self) -> String {
        self.input.clone()
    }

    fn lispex_application(
        &self,
        _request: topaz_rt::LispexApplicationRequest,
    ) -> topaz_rt::LispexApplicationResponse {
        topaz_rt::LispexApplicationResponse::OperationalFault {
            code: "target-unavailable".into(),
            detail: None,
        }
    }
}

thread_local! {
    static WEB_ALLOCATIONS: RefCell<BTreeMap<usize, Box<[u8]>>> =
        RefCell::new(BTreeMap::new());
}

fn store_bytes(mut bytes: Box<[u8]>) -> *mut u8 {
    if bytes.is_empty() {
        return std::ptr::null_mut();
    }
    let ptr = bytes.as_mut_ptr();
    WEB_ALLOCATIONS.with(|allocations| {
        let previous = allocations.borrow_mut().insert(ptr as usize, bytes);
        debug_assert!(previous.is_none(), "live Web allocation pointer collision");
    });
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    store_bytes(vec![0_u8; len].into_boxed_slice())
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_free(ptr: *mut u8, len: usize) {
    let _ = topaz_free_checked(ptr, len);
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_free_checked(ptr: *mut u8, len: usize) -> u32 {
    if ptr.is_null() || len == 0 {
        return u32::from(!(ptr.is_null() && len == 0));
    }
    WEB_ALLOCATIONS.with(|allocations| {
        let mut allocations = allocations.borrow_mut();
        let Some(bytes) = allocations.get(&(ptr as usize)) else {
            return 1;
        };
        if bytes.len() != len {
            return 2;
        }
        allocations.remove(&(ptr as usize));
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_live_allocations() -> usize {
    WEB_ALLOCATIONS.with(|allocations| allocations.borrow().len())
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_export_names_json() -> u64 {
    let mut out = String::from("[");
    for (i, name) in emitted::topaz_export_names().iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(&mut out, name);
    }
    out.push(']');
    store_string(out)
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_call_export_json(
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const u8,
    args_len: usize,
) -> u64 {
    let name = match read_utf8(name_ptr, name_len, "export name") {
        Ok(name) => name,
        Err(error) => return store_string(topaz_rt::canonical_abi_error(&error)),
    };
    let args_json = match read_utf8(args_ptr, args_len, "argument JSON") {
        Ok(args_json) => args_json,
        Err(error) => return store_string(topaz_rt::canonical_abi_error(&error)),
    };
    let host_impl = Rc::new(WebHost::default());
    let host: Rc<dyn Host> = host_impl.clone();
    store_string(emitted::call_export_json_with_host(host, &name, &args_json))
}

#[unsafe(no_mangle)]
pub extern "C" fn topaz_call_export_trace_json(
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const u8,
    args_len: usize,
    input_ptr: *const u8,
    input_len: usize,
) -> u64 {
    let empty_host = WebHost::default();
    let name = match read_utf8(name_ptr, name_len, "export name") {
        Ok(name) => name,
        Err(error) => return store_string(trace_json(topaz_rt::canonical_abi_error(&error), &empty_host)),
    };
    let args_json = match read_utf8(args_ptr, args_len, "argument JSON") {
        Ok(args_json) => args_json,
        Err(error) => return store_string(trace_json(topaz_rt::canonical_abi_error(&error), &empty_host)),
    };
    let input = match read_utf8(input_ptr, input_len, "input") {
        Ok(input) => input,
        Err(error) => return store_string(trace_json(topaz_rt::canonical_abi_error(&error), &empty_host)),
    };
    let host_impl = Rc::new(WebHost::new(input));
    let host: Rc<dyn Host> = host_impl.clone();
    let outcome = emitted::call_export_json_with_host(host, &name, &args_json);
    store_string(trace_json(outcome, &host_impl))
}

fn read_utf8(ptr: *const u8, len: usize, label: &str) -> Result<String, String> {
    if len == 0 {
        return if ptr.is_null() {
            Ok(String::new())
        } else {
            Err(format!("{label} has a non-null pointer for an empty value"))
        };
    }
    if ptr.is_null() {
        return Err(format!("{label} pointer is null"));
    }
    WEB_ALLOCATIONS.with(|allocations| {
        let allocations = allocations.borrow();
        let bytes = allocations
            .get(&(ptr as usize))
            .ok_or_else(|| format!("{label} pointer is not a live Topaz Web allocation"))?;
        if bytes.len() != len {
            return Err(format!(
                "{label} allocation length mismatch: expected {}, received {len}",
                bytes.len()
            ));
        }
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| format!("{label} is not valid UTF-8"))
    })
}

fn store_string(text: String) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let bytes = text.into_bytes().into_boxed_slice();
    let len = bytes.len();
    let ptr = store_bytes(bytes);
    ((ptr as u64) << 32) | (len as u64)
}

fn push_json_string(out: &mut String, raw: &str) {
    out.push('"');
    for c in raw.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn trace_json(outcome: String, host: &WebHost) -> String {
    let mut out = String::from("{\"outcome\":");
    out.push_str(&outcome);
    out.push_str(",\"stdout\":");
    push_json_string_array(&mut out, &host.stdout());
    out.push_str(",\"deferErrors\":");
    push_json_string_array(&mut out, &host.defer_errors());
    out.push('}');
    out
}

fn push_json_string_array(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(out, item);
    }
    out.push(']');
}
