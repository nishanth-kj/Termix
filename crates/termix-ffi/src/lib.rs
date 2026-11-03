//! Stable C exports. The strings returned here are JSON and must be freed
//! with `termix_string_free`. No secrets are returned.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use termix_core::{Capability, Support};

fn leak_json(value: &str) -> *mut c_char {
    CString::new(value)
        .unwrap_or_else(|_| CString::new("").expect("empty"))
        .into_raw()
}

#[no_mangle]
pub extern "C" fn termix_version() -> *const c_char {
    static VERSION: &CStr = c"0.1.0";
    VERSION.as_ptr()
}

/// # Safety
/// `ptr` must be null or a pointer returned by `termix_support_json`.
#[no_mangle]
pub unsafe extern "C" fn termix_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe { drop(CString::from_raw(ptr)) }
}

#[no_mangle]
pub extern "C" fn termix_support_json() -> *mut c_char {
    let items: Vec<_> = [
        Capability::Ssh,
        Capability::Sftp,
        Capability::LocalTerminal,
        Capability::HostKeyVerification,
        Capability::EncryptedSecrets,
        Capability::SystemMonitorLocal,
        Capability::SystemMonitorSsh,
        Capability::FolderSyncPlan,
        Capability::Docker,
        Capability::Kubernetes,
        Capability::Rdp,
        Capability::Vnc,
    ]
    .into_iter()
    .map(|capability| {
        serde_json::json!({
            "name": capability.label(),
            "implemented": capability.support() == Support::Implemented,
        })
    })
    .collect();
    leak_json(&serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn version_and_support() {
        let version = unsafe { CStr::from_ptr(termix_version()) };
        assert_eq!(version.to_str().unwrap(), "0.1.0");
        let ptr = termix_support_json();
        let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { termix_string_free(ptr) };
        assert!(text.contains("SSH"));
        assert!(text.contains("\"implemented\":false"));
    }
}
