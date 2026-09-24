//! Native macOS SAORI libraries use UTF-8 and transfer malloc-owned buffers.
use std::{
    collections::HashMap,
    ffi::{c_void, CString},
    path::Path,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
type Load = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type Request = unsafe extern "C" fn(*mut c_void, *mut i32) -> *mut c_void;
type Unload = unsafe extern "C" fn() -> i32;
pub struct SaoriDll {
    handle: *mut c_void,
    request_fn: Request,
    unload_fn: Unload,
    lock: Mutex<()>,
}
unsafe impl Send for SaoriDll {}
unsafe impl Sync for SaoriDll {}
impl Drop for SaoriDll {
    fn drop(&mut self) {
        unsafe {
            (self.unload_fn)();
            libc::dlclose(self.handle);
        }
    }
}
unsafe fn buffer(bytes: &[u8]) -> Result<*mut c_void, String> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("SAORI input too large".into());
    }
    let pointer = libc::malloc(bytes.len().max(1));
    if pointer.is_null() {
        return Err("SAORI allocation failed".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
    Ok(pointer)
}
impl SaoriDll {
    pub fn load(path: &Path) -> Result<Self, String> {
        let path = if path
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("dll"))
        {
            path.with_extension("dylib")
        } else {
            path.to_owned()
        };
        let filename =
            CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        unsafe {
            let handle = libc::dlopen(filename.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                return Err(format!("SAORI library unavailable: {}", path.display()));
            }
            let loadu = libc::dlsym(handle, c"loadu".as_ptr());
            let loader = if loadu.is_null() {
                libc::dlsym(handle, c"load".as_ptr())
            } else {
                loadu
            };
            let requester = libc::dlsym(handle, c"request".as_ptr());
            let unloader = libc::dlsym(handle, c"unload".as_ptr());
            let result = (|| {
                if loader.is_null() || requester.is_null() || unloader.is_null() {
                    return Err("SAORI entry points missing".into());
                }
                let load: Load = std::mem::transmute(loader);
                let directory = format!("{}/", path.parent().unwrap_or(Path::new(".")).display());
                let input = buffer(directory.as_bytes())?;
                if load(input, directory.len() as i32) == 0 {
                    return Err("SAORI load failed".into());
                }
                Ok(Self {
                    handle,
                    request_fn: std::mem::transmute(requester),
                    unload_fn: std::mem::transmute(unloader),
                    lock: Mutex::new(()),
                })
            })();
            if result.is_err() {
                libc::dlclose(handle);
            }
            result
        }
    }
    pub fn request(&self, args: &[String]) -> HashMap<String, String> {
        let Ok(_guard) = self.lock.lock() else {
            return HashMap::new();
        };
        let mut request = "EXECUTE SAORI/1.0\r\nCharset: UTF-8\r\n".to_owned();
        for (i, value) in args.iter().enumerate() {
            request.push_str(&format!("Argument{i}: {value}\r\n"));
        }
        request.push_str("\r\n");
        unsafe {
            let Ok(input) = buffer(request.as_bytes()) else {
                return HashMap::new();
            };
            let mut length = request.len() as i32;
            let response = (self.request_fn)(input, &mut length);
            if response.is_null() {
                return HashMap::new();
            }
            let mut values = HashMap::new();
            if (0..=8 * 1024 * 1024).contains(&length)
                && length as usize <= libc::malloc_size(response)
            {
                let bytes = std::slice::from_raw_parts(response.cast::<u8>(), length as usize);
                if let Ok(text) = std::str::from_utf8(bytes) {
                    for line in text.lines() {
                        if let Some((key, value)) = line.split_once(": ") {
                            if key == "Result" {
                                values
                                    .entry("0".into())
                                    .or_insert_with(|| value.trim().into());
                            } else if let Some(n) = key.strip_prefix("Value") {
                                values.insert(n.into(), value.trim().into());
                            }
                        }
                    }
                }
            }
            libc::free(response);
            values
        }
    }
}
pub fn request_with_timeout(
    dll: Arc<SaoriDll>,
    args: Vec<String>,
    timeout: Duration,
) -> Option<HashMap<String, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("saori-call".into())
        .spawn(move || {
            let _ = sender.send(dll.request(&args));
        })
        .ok()?;
    receiver.recv_timeout(timeout).ok()
}
