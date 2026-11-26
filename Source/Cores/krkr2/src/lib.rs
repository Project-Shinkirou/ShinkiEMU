//! Android uses the native shared library; iOS links the private native archive
//! into this Framework producer.

pub use shinki_emu_krkr2::query;

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(
    version: u32,
    output: *mut *const shinki_emu_abi::CoreApi,
) -> shinki_emu_abi::Status {
    // SAFETY: the ABI caller supplies the output pointer for the native query.
    unsafe { query(version, output) }
}
