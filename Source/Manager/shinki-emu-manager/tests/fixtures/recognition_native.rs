//! Compiled as a real cdylib by the focused recognition gate, not a Cargo test.
#[path = "../../../shinki-emu-manager-core/src/test_native.rs"]
mod native;
use parking_lot::Mutex;
use shinki_emu_abi as abi;
use std::sync::{Arc, LazyLock, atomic::{AtomicU64, Ordering}};
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static CALLS: LazyLock<Arc<Mutex<Vec<native::Call>>>> = LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    QUERY_COUNT.fetch_add(1, Ordering::SeqCst);
    let (plugin, family) = if cfg!(fixture_alternative) { ("siglus_rs", "siglus") }
        else if cfg!(fixture_failure) { ("ethornell-rs", "ethornell") }
        else { ("rfvp", "fvp") };
    let mut fixture = native::Fixture::new(plugin, family, "fixture.native");
    fixture.config = true;
    fixture.init_error = cfg!(fixture_failure);
    fixture.marker = Some("native-marker");
    fixture.calls = CALLS.clone();
    let query = native::prepare_query(0, fixture);
    unsafe { query(version, out) }
}

/// 0=query, 1=initialize, 2=describe, 3=probe, 4=open, 5=all recorded calls.
#[unsafe(no_mangle)]
pub extern "C" fn recognition_fixture_count(kind: u32) -> u64 {
    if kind == 0 { return QUERY_COUNT.load(Ordering::SeqCst); }
    let calls = CALLS.lock();
    if kind == 5 { return calls.len() as u64; }
    let name = match kind { 1 => "initialize", 2 => "describe", 3 => "probe", 4 => "open", _ => return 0 };
    calls.iter().filter(|call| call.name == name).count() as u64
}
