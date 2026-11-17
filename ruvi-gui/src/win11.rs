//! Windows 11 native window chrome integration (DWM).
//!
//! Applies an immersive dark/light title bar matching the active studio theme,
//! rounded window corners and the Mica system backdrop on the non-client area.
//! All calls are best-effort: on Windows 10 or older builds unsupported
//! attributes are silently ignored by DWM, and on non-Windows targets this is a no-op.

#[cfg(windows)]
pub fn apply_window_chrome(frame: &eframe::Frame, dark: bool) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    // DWMWINDOWATTRIBUTE values (stable since Windows 11 22H2; older builds ignore them).
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMWCP_ROUND: i32 = 2;
    const DWMSBT_MAINWINDOW: i32 = 2; // Mica

    let Ok(handle) = frame.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as windows_sys::Win32::Foundation::HWND;

    let set = |attr: u32, value: i32| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            &value as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        )
    };

    let dark_ok = set(DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(dark)) == 0;
    let _ = set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND);
    let _ = set(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW);
    dark_ok
}

#[cfg(not(windows))]
pub fn apply_window_chrome(_frame: &eframe::Frame, _dark: bool) -> bool {
    false
}
