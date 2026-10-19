//! macOS conventional SHIORI ABI: UTF-8, signed 32-bit lengths, malloc/free ownership.
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

static FFI_LOCK: Mutex<()> = Mutex::new(());
const MAX_BYTES: i32 = 8 * 1024 * 1024;

unsafe fn take_input(pointer: *mut c_void, length: i32) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let value = if (0..=MAX_BYTES).contains(&length) {
        let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), length as usize);
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    } else {
        None
    };
    libc::free(pointer);
    value
}

#[no_mangle]
pub extern "C" fn loadu(pointer: *mut c_void, length: i32) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(path) = (unsafe { take_input(pointer, length) }) else {
            return 0;
        };
        let Ok(_guard) = FFI_LOCK.try_lock() else {
            return 0;
        };
        // The conventional ABI owns one ghost per loaded image. Never overwrite another session.
        if super::is_already_initialized() || path.contains('\0') {
            return 0;
        }
        match super::init(std::path::Path::new(&path)) {
            Ok(state) => {
                *super::lock_state() = Some(state);
                1
            }
            Err(_) => 0,
        }
    }))
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn load(pointer: *mut c_void, length: i32) -> i32 {
    loadu(pointer, length)
}

#[no_mangle]
pub extern "C" fn request(pointer: *mut c_void, length: *mut i32) -> *mut c_void {
    catch_unwind(AssertUnwindSafe(|| {
        if length.is_null() {
            unsafe {
                libc::free(pointer);
            }
            return std::ptr::null_mut();
        }
        let input = unsafe { take_input(pointer, *length) };
        unsafe {
            *length = 0;
        }
        let Some(input) = input else {
            return std::ptr::null_mut();
        };
        let Ok(_guard) = FFI_LOCK.try_lock() else {
            return std::ptr::null_mut();
        };
        if !super::is_already_initialized() {
            return std::ptr::null_mut();
        }
        let response = catch_unwind(AssertUnwindSafe(|| super::handle_request(&input)))
            .unwrap_or_else(|_| {
                super::PANICKED.store(true, std::sync::atomic::Ordering::Relaxed);
                super::error_response("panic")
            });
        let response = super::encode_response(&response, true);
        if response.len() > MAX_BYTES as usize {
            return std::ptr::null_mut();
        }
        unsafe {
            let result = libc::malloc(response.len().max(1));
            if result.is_null() {
                return result;
            }
            std::ptr::copy_nonoverlapping(response.as_ptr(), result.cast(), response.len());
            *length = response.len() as i32;
            result
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn unload() -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Ok(_guard) = FFI_LOCK.try_lock() else {
            return 0;
        };
        let mut state = super::lock_state();
        if let Some(current) = state.as_ref() {
            if !current.load_failed
                && super::save_globals(&current.codegen.env.globals, &current.ghost_dir).is_err()
            {
                return 0;
            }
        }
        *state = None;
        1
    }))
    .unwrap_or(0)
}
