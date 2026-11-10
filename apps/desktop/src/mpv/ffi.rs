//! The subset of libmpv's C API (`client.h`, `render.h`) the player uses,
//! resolved at run time so that building needs neither mpv headers nor an
//! import library. Values match libmpv's client API 2.x (mpv 0.35 and later),
//! which kept them unchanged from 1.x.
#![allow(non_camel_case_types)]

use libloading::Library;
use std::ffi::{c_char, c_double, c_int, c_ulong, c_void};

#[repr(C)]
pub struct mpv_handle {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct mpv_render_context {
    _opaque: [u8; 0],
}

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

/// Only the leading fields are read; the struct grew over releases.
#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
}

#[repr(C)]
pub struct mpv_render_param {
    pub kind: c_int,
    pub data: *mut c_void,
}

pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

pub const FORMAT_NONE: c_int = 0;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;

pub const END_FILE_REASON_EOF: c_int = 0;
pub const END_FILE_REASON_ERROR: c_int = 4;

pub const RENDER_PARAM_INVALID: c_int = 0;
pub const RENDER_PARAM_API_TYPE: c_int = 1;
pub const RENDER_PARAM_SW_SIZE: c_int = 17;
pub const RENDER_PARAM_SW_FORMAT: c_int = 18;
pub const RENDER_PARAM_SW_STRIDE: c_int = 19;
pub const RENDER_PARAM_SW_POINTER: c_int = 20;
pub const RENDER_UPDATE_FRAME: u64 = 1;

/// The software renderer appeared in client API 1.107 (mpv 0.33).
const MIN_API_VERSION: c_ulong = (1 << 16) | 107;

/// Library file names tried in order, after `AETHER_LIBMPV` if it is set.
pub const LIBRARY_NAMES: &[&str] = if cfg!(windows) {
    &["libmpv-2.dll", "mpv-2.dll", "mpv-1.dll"]
} else if cfg!(target_os = "macos") {
    &[
        "libmpv.2.dylib",
        "libmpv.dylib",
        "/opt/homebrew/lib/libmpv.dylib",
        "/usr/local/lib/libmpv.dylib",
    ]
} else {
    &["libmpv.so.2", "libmpv.so.1", "libmpv.so"]
};

/// # Safety
/// `T` must be the exact function pointer type of the symbol `name`.
unsafe fn symbol<T: Copy>(library: &Library, name: &str) -> Result<T, String> {
    let bytes = format!("{name}\0");
    // SAFETY: forwarded to the caller.
    unsafe { library.get::<T>(bytes.as_bytes()) }
        .map(|found| *found)
        .map_err(|error| format!("libmpv sans {name}: {error}"))
}

type UpdateCallback = unsafe extern "C" fn(*mut c_void);

pub struct Api {
    pub client_api_version: unsafe extern "C" fn() -> c_ulong,
    pub create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub set_option_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub set_property_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub get_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> *mut c_char,
    pub command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    pub observe_property: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    pub wait_event: unsafe extern "C" fn(*mut mpv_handle, c_double) -> *mut mpv_event,
    pub wakeup: unsafe extern "C" fn(*mut mpv_handle),
    pub render_context_create: unsafe extern "C" fn(
        *mut *mut mpv_render_context,
        *mut mpv_handle,
        *mut mpv_render_param,
    ) -> c_int,
    pub render_context_set_update_callback:
        unsafe extern "C" fn(*mut mpv_render_context, Option<UpdateCallback>, *mut c_void),
    pub render_context_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    pub render_context_render:
        unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    pub render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
    /// Keeps the function pointers above valid; declared last so it drops last.
    _library: Library,
}

impl Api {
    /// Opens libmpv, or explains how to install it.
    pub fn load() -> Result<Self, String> {
        let custom = std::env::var("AETHER_LIBMPV").ok();
        let names = custom
            .iter()
            .map(String::as_str)
            .chain(LIBRARY_NAMES.iter().copied());
        let mut last_error = String::new();
        for name in names {
            // SAFETY: loading libmpv runs its library constructors, which have
            // no preconditions; the symbols are checked below.
            match unsafe { Library::new(name) } {
                Ok(library) => return Self::resolve(library),
                Err(error) => last_error = error.to_string(),
            }
        }
        Err(format!(
            "libmpv est introuvable ({last_error}). Installez mpv/libmpv (voir apps/desktop/README.md) \
             ou indiquez son chemin dans AETHER_LIBMPV."
        ))
    }

    fn resolve(library: Library) -> Result<Self, String> {
        macro_rules! symbol {
            ($name:literal) => {
                // SAFETY: each field is declared with the signature of the
                // same-named function in libmpv's client.h / render.h.
                unsafe { symbol(&library, $name) }?
            };
        }
        let api = Self {
            client_api_version: symbol!("mpv_client_api_version"),
            create: symbol!("mpv_create"),
            initialize: symbol!("mpv_initialize"),
            terminate_destroy: symbol!("mpv_terminate_destroy"),
            error_string: symbol!("mpv_error_string"),
            free: symbol!("mpv_free"),
            set_option_string: symbol!("mpv_set_option_string"),
            set_property_string: symbol!("mpv_set_property_string"),
            get_property_string: symbol!("mpv_get_property_string"),
            command: symbol!("mpv_command"),
            observe_property: symbol!("mpv_observe_property"),
            wait_event: symbol!("mpv_wait_event"),
            wakeup: symbol!("mpv_wakeup"),
            render_context_create: symbol!("mpv_render_context_create"),
            render_context_set_update_callback: symbol!("mpv_render_context_set_update_callback"),
            render_context_update: symbol!("mpv_render_context_update"),
            render_context_render: symbol!("mpv_render_context_render"),
            render_context_free: symbol!("mpv_render_context_free"),
            _library: library,
        };
        // SAFETY: takes no argument and only returns a constant.
        let version = unsafe { (api.client_api_version)() };
        if version < MIN_API_VERSION {
            return Err(format!(
                "libmpv trop ancienne (API {}.{}), mpv 0.35 ou plus récent est nécessaire",
                version >> 16,
                version & 0xffff
            ));
        }
        Ok(api)
    }
}
