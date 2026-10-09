use crate::{
    config::Config,
    engine::{Engine, Fault},
    web,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    ffi::{c_char, c_void, CStr},
    ptr,
    sync::{Mutex, OnceLock},
};

#[repr(C)]
pub struct Buffer {
    ptr: *mut u8,
    len: usize,
}
#[repr(C)]
pub struct HostApi {
    abi_version: u32,
    host_ctx: *mut c_void,
    call: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize, *mut Buffer) -> i32,
    >,
    free_buffer: Option<unsafe extern "C" fn(*mut c_void, usize)>,
}
#[repr(C)]
pub struct PluginApi {
    abi_version: u32,
    call: Option<unsafe extern "C" fn(*const c_char, *const u8, usize, *mut Buffer) -> i32>,
    free_buffer: Option<unsafe extern "C" fn(*mut c_void, usize)>,
    shutdown: Option<unsafe extern "C" fn()>,
}
#[derive(Default)]
struct State {
    engine: Option<Engine>,
    error: Option<String>,
    enforcement_enabled: bool,
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();

#[no_mangle]
pub unsafe extern "C" fn cliproxy_plugin_init(host: *const HostApi, plugin: *mut PluginApi) -> i32 {
    if plugin.is_null() || host.is_null() || (*host).abi_version != 1 {
        return 1;
    }
    *plugin = PluginApi {
        abi_version: 1,
        call: Some(call),
        free_buffer: Some(free),
        shutdown: Some(shutdown),
    };
    0
}
unsafe extern "C" fn call(
    method: *const c_char,
    request: *const u8,
    len: usize,
    response: *mut Buffer,
) -> i32 {
    if response.is_null() {
        return 1;
    }
    *response = Buffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    if method.is_null() || (request.is_null() && len > 0) {
        return 1;
    }
    let method = match CStr::from_ptr(method).to_str() {
        Ok(method) => method,
        Err(_) => return 1,
    };
    let bytes = if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(request, len)
    };
    let result = std::panic::catch_unwind(|| {
        let data: Value = if bytes.is_empty() {
            json!({})
        } else {
            match serde_json::from_slice(bytes) {
                Ok(data) => data,
                Err(_) => return failure(method, "宿主 RPC JSON 无效"),
            }
        };
        dispatch(method, &data)
    })
    .unwrap_or_else(|_| failure(method, "插件内部异常，已拒绝当前请求"));
    let bytes = result.to_string().into_bytes().into_boxed_slice();
    (*response).len = bytes.len();
    (*response).ptr = Box::into_raw(bytes) as *mut u8;
    0
}
unsafe extern "C" fn free(pointer: *mut c_void, len: usize) {
    if !pointer.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer as *mut u8,
            len,
        )));
    }
}
unsafe extern "C" fn shutdown() {
    if let Some(lock) = STATE.get() {
        if let Ok(mut state) = lock.lock() {
            state.engine = None;
            state.error = None;
            state.enforcement_enabled = false;
        }
    }
}
fn ok(result: Value) -> Value {
    json!({"ok":true,"result":result})
}
fn terminate(error: &Fault) -> Value {
    web::rejection(error.status, &error.json())
}
fn failure(method: &str, message: &str) -> Value {
    let error = Fault::new(
        503,
        "plugin_unavailable",
        format!(
            "插件暂时不可用：{}。请联系管理员检查插件配置和运行状态。",
            message.trim_end_matches('。')
        ),
    );
    if method.starts_with("request.intercept") {
        return ok(terminate(&error));
    }
    if method == "management.handle" {
        return ok(web::response(
            503,
            "application/json; charset=utf-8",
            &error.json().to_string(),
        ));
    }
    json!({"ok":false,"error":{"code":error.code,"message":error.message}})
}
fn registration() -> Value {
    json!({"schema_version":6,
        "metadata":{"Name":"API 密钥权限与额度","Version":env!("CARGO_PKG_VERSION"),
        "Author":"cpa-apikey-manager","GitHubRepository":option_env!("CPA_PLUGIN_REPOSITORY").unwrap_or(env!("CARGO_PKG_REPOSITORY")),"ConfigFields":[
            {"Name":"cpa-config-path","Type":"string","Description":"可选；默认自动识别宿主 -config 参数或工作目录 config.yaml"},
            {"Name":"cpa-base-url","Type":"string","Description":"宿主地址，用于读取模型目录"},
            {"Name":"data-dir","Type":"string","Description":"SQLite 数据目录"}]},
        "capabilities":{"request_interceptor":true,"request_lifecycle_plugin":true,"usage_plugin":true,"management_api":true}})
}
fn dispatch(method: &str, data: &Value) -> Value {
    if method == "management.register" {
        return ok(web::registration(data));
    }
    if method == "management.handle" {
        if let Some(resource) = web::resource(data["Path"].as_str().unwrap_or("")) {
            return ok(resource);
        }
    }
    let lock = STATE.get_or_init(|| Mutex::new(State::default()));
    let mut state = match lock.lock() {
        Ok(state) => state,
        Err(_) => return failure(method, "插件状态已损坏，请重启插件"),
    };
    if method == "plugin.register" || method == "plugin.reconfigure" {
        let config = STANDARD
            .decode(data["config_yaml"].as_str().unwrap_or(""))
            .map_err(|_| "插件配置编码无效".to_string())
            .and_then(|bytes| {
                serde_yaml::from_slice::<Config>(&bytes)
                    .map_err(|_| "插件 YAML 配置无效".to_string())
            });
        let result = config.and_then(|config| {
            let config = config.resolve()?;
            if let Some(engine) = state.engine.as_mut() {
                if engine.config.data_dir != config.data_dir {
                    return Err("修改数据目录需要重启宿主".into());
                }
                engine.config = config;
                if let Some(worker) = &engine.price_sync {
                    worker.reconfigure(engine.config.clone());
                }
                let _ = engine.sync();
            } else {
                state.engine = Some(Engine::open(config).map_err(|e| e.message)?);
            }
            Ok(())
        });
        state.error = result.err();
        state.enforcement_enabled = state
            .engine
            .as_ref()
            .and_then(|engine| engine.enforcement_enabled().ok())
            .unwrap_or(false);
        // Keep the plugin registered so the management page remains available.
        // Runtime callbacks bypass until enforcement is explicitly enabled.
        return ok(registration());
    }
    if let Some(response) = bypass_when_disabled(method, state.enforcement_enabled) {
        return response;
    }
    if let Some(error) = &state.error {
        return failure(method, error);
    }
    let Some(engine) = state.engine.as_mut() else {
        return failure(method, "插件尚未完成配置");
    };
    match method {
        "request.intercept_before" | "request.intercept_after" => {
            match engine.intercept(data, method.ends_with("_after"), chrono::Utc::now()) {
                Ok(result) => ok(result),
                Err(error) => ok(terminate(&error)),
            }
        }
        "request.complete" => match engine.complete(data, chrono::Utc::now()) {
            Ok(()) => ok(json!({})),
            Err(error) => {
                engine.health_error = Some(error.message.clone());
                failure(method, &error.message)
            }
        },
        "usage.handle" => match engine.usage(data, chrono::Utc::now()) {
            Ok(()) => ok(json!({})),
            Err(error) => {
                engine.health_error = Some(error.message.clone());
                failure(method, &error.message)
            }
        },
        "management.handle" => {
            let response = web::handle(engine, data);
            if let Ok(enabled) = engine.enforcement_enabled() {
                state.enforcement_enabled = enabled;
            }
            ok(response)
        }
        _ => json!({"ok":false,"error":{"code":"unknown_method","message":"不支持的宿主调用方法"}}),
    }
}

fn bypass_when_disabled(method: &str, enforcement_enabled: bool) -> Option<Value> {
    if enforcement_enabled {
        return None;
    }
    match method {
        "request.intercept_before"
        | "request.intercept_after"
        | "request.complete"
        | "usage.handle" => Some(ok(json!({}))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{bypass_when_disabled, registration};

    #[test]
    fn disabled_enforcement_bypasses_runtime_callbacks() {
        for method in [
            "request.intercept_before",
            "request.intercept_after",
            "request.complete",
            "usage.handle",
        ] {
            let response = bypass_when_disabled(method, false).expect("callback should bypass");
            assert_eq!(response["ok"], true);
            assert_eq!(response["result"], serde_json::json!({}));
        }
    }

    #[test]
    fn enabled_enforcement_uses_normal_runtime_path() {
        assert!(bypass_when_disabled("request.intercept_before", true).is_none());
        assert!(bypass_when_disabled("management.handle", false).is_none());
    }

    #[test]
    fn plugin_config_no_longer_exposes_enforcement_switch() {
        let value = registration();
        let fields = value["metadata"]["ConfigFields"].as_array().unwrap();
        assert!(!fields
            .iter()
            .any(|field| field["Name"] == "enforcement-enabled"));
    }
}
