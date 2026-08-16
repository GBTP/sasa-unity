use std::{
    cell::RefCell,
    ffi::CString,
    fmt::Display,
    os::raw::{c_char, c_uchar},
    ptr::null,
};

thread_local! {
    static ERROR_INFO: RefCell<Option<CString>> = const { RefCell::new(None) };
}

pub fn set_error(error: impl Display) {
    // 错误信息里出现 NUL 时退化成固定串，避免 unwrap 在 FFI 边界上 panic
    let msg = CString::new(format!("{error}"))
        .unwrap_or_else(|_| CString::new("error message contains NUL").unwrap());
    ERROR_INFO.with(|it| *it.borrow_mut() = Some(msg));
}

pub fn clear_error() {
    ERROR_INFO.with(|it| *it.borrow_mut() = None);
}

/// 执行可能失败的操作，失败时记录错误并返回 `default`。
pub fn fallible<T>(op: impl FnOnce() -> anyhow::Result<T>, default: T) -> T {
    match op() {
        Ok(v) => v,
        Err(e) => {
            set_error(e);
            default
        }
    }
}

#[no_mangle]
pub extern "C" fn SasaUnity_HasError() -> c_uchar {
    ERROR_INFO.with(|it| u8::from(it.borrow().is_some()))
}

/// 返回值指向线程局部缓冲，下一次同线程的 sasa 调用即失效，调用方需立刻拷贝。
#[no_mangle]
pub extern "C" fn SasaUnity_GetError() -> *const c_char {
    ERROR_INFO.with(|it| match it.borrow().as_ref() {
        Some(msg) => msg.as_ptr(),
        None => null(),
    })
}
