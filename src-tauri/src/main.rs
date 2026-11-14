#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Windows 7 compatibility shims.
///
/// Crates in the dependency tree statically import a few Windows 8+ entry
/// points without ever calling them — the `windows` crate's ETW module, for
/// example, is dragged in by linker granularity rather than by an actual call.
/// The Windows 7 loader resolves *every* static import before the process
/// starts, so the client aborts with "Entry Point Not Found" before any of our
/// own code runs.
///
/// Defining the symbol here satisfies the reference at link time, so no import
/// is emitted and nothing has to be patched after the build. The stub returns
/// `ERROR_SUCCESS`; it is never executed because nothing calls it.
#[cfg(all(windows, target_vendor = "win7"))]
#[no_mangle]
pub unsafe extern "system" fn EventSetInformation(
    _registration_handle: u64,
    _information_class: i32,
    _event_information: *const core::ffi::c_void,
    _information_length: u32,
) -> u32 {
    0
}

fn main() {
    pointer_app_lib::run();
}
