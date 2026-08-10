use std::{
    collections::HashMap,
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use ai::model::ApiRegistry;
use libloading::Library;
use serde::Deserialize;
use serde_json::Value;
use tool::{SearchRegistry, ToolRegistry};
use xstudio_plugin_api::{
    HostApiV1, JsonBytes, PluginDescriptorV1, PluginEntryV1, PluginJsonBytes, PluginStatus,
    PLUGIN_ABI_VERSION, PLUGIN_ENTRY_SYMBOL,
};

use crate::{
    adapters::{PluginAgentTool, PluginApiProvider, PluginContribution, PluginSearchEngine},
    env::{
        call_default_env, ExecutionEnvFactory, LocalExecutionEnvFactory, PluginExecutionEnvFactory,
    },
    error::{PluginError, PluginResult},
};

/// 插件运行时初始化选项。
#[derive(Clone, Debug)]
pub struct PluginRuntimeOptions {
    /// 上层提供的应用数据目录。
    pub app_dir: PathBuf,
}

/// 已加载插件清单。
#[derive(Debug, Deserialize)]
struct PluginManifest {
    /// 插件稳定标识。
    id: String,
    /// 声明能力列表。
    #[serde(default)]
    capabilities: Vec<String>,
    /// Provider 能力描述。
    #[serde(default)]
    providers: Vec<PluginContribution>,
    /// AgentTool 能力描述。
    #[serde(default)]
    tools: Vec<PluginContribution>,
    /// 搜索能力描述。
    #[serde(default)]
    searches: Vec<PluginContribution>,
}

/// 已加载的原生插件；持有动态库句柄直到进程退出。
pub struct PluginHandle {
    /// 动态库句柄，必须早于所有函数指针释放。
    _library: Library,
    /// 供插件反向调用宿主能力的进程级上下文。
    _host_context: Box<HostEnvContext>,
    /// 插件原始上下文。
    context: *mut c_void,
    /// 统一 JSON 调用入口。
    call_fn: xstudio_plugin_api::PluginCallV1,
    /// 插件 id。
    id: String,
    /// 插件声明能力。
    capabilities: Vec<String>,
    /// Provider 注册描述。
    providers: Vec<PluginContribution>,
    /// 工具注册描述。
    tools: Vec<PluginContribution>,
    /// 搜索注册描述。
    searches: Vec<PluginContribution>,
}

/// 宿主 Env 回调上下文。
struct HostEnvContext {
    /// 用于同步等待默认 Env 异步操作的 Tokio 运行时句柄。
    runtime: tokio::runtime::Handle,
}

unsafe impl Send for PluginHandle {}
unsafe impl Sync for PluginHandle {}

impl PluginHandle {
    /// 调用插件的版本化 JSON 能力。
    /// @param request 请求 JSON。
    pub fn call(&self, request: Value) -> PluginResult<Value> {
        let request = serde_json::to_vec(&request).map_err(|error| PluginError::Protocol {
            path: PathBuf::from(&self.id),
            message: error.to_string(),
        })?;
        let mut output = PluginJsonBytes {
            data: std::ptr::null(),
            len: 0,
            free: None,
        };
        let status = unsafe {
            (self.call_fn)(
                self.context,
                JsonBytes {
                    data: request.as_ptr(),
                    len: request.len(),
                },
                &mut output,
            )
        };
        if status != PluginStatus::Ok {
            free_json_bytes(output);
            return Err(PluginError::Protocol {
                path: PathBuf::from(&self.id),
                message: format!("插件调用失败: {status:?}"),
            });
        }
        let result = json_bytes(output.data, output.len, &self.id).and_then(|bytes| {
            serde_json::from_slice(bytes).map_err(|error| PluginError::Protocol {
                path: PathBuf::from(&self.id),
                message: format!("插件响应不是有效 JSON: {error}"),
            })
        });
        free_json_bytes(output);
        result
    }

    /// 判断插件是否声明某项能力。
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|item| item == capability)
    }
}

/// 进程级插件运行时。
pub struct PluginRuntime {
    /// 上层传入的应用目录。
    app_dir: PathBuf,
    /// 加载完成的原生插件。
    plugins: Vec<Arc<PluginHandle>>,
    /// 最终选定的执行环境工厂。
    env_factory: Arc<dyn ExecutionEnvFactory>,
}

static GLOBAL_RUNTIME: OnceLock<PluginRuntime> = OnceLock::new();

impl PluginRuntime {
    /// 从应用目录发现插件并初始化所有注册表。
    /// @param options 上层提供的运行时路径选项。
    pub fn global_with_options(options: PluginRuntimeOptions) -> PluginResult<&'static Self> {
        let runtime = Self::load(options)?;
        GLOBAL_RUNTIME
            .set(runtime)
            .map_err(|_| PluginError::EnvCall("插件运行时已初始化".to_string()))?;
        Ok(GLOBAL_RUNTIME
            .get()
            .expect("PluginRuntime must be available after initialization"))
    }

    /// 返回已初始化的插件运行时。
    pub fn global() -> PluginResult<&'static Self> {
        GLOBAL_RUNTIME
            .get()
            .ok_or_else(|| PluginError::EnvCall("插件运行时尚未初始化".to_string()))
    }

    /// 通过选定 factory 创建项目执行环境。
    /// @param cwd 项目工作目录。
    pub fn create_env(&self, cwd: &Path) -> PluginResult<Arc<dyn ai::agent::env::ExecutionEnv>> {
        self.env_factory.create(cwd)
    }

    /// 向所有声明 `harness` 能力的插件发送 Harness 事件快照。
    /// @param event 可序列化的 Harness 事件 JSON。
    pub async fn notify_harness_event(&self, event: Value) -> PluginResult<()> {
        for plugin in self
            .plugins
            .iter()
            .filter(|plugin| plugin.supports("harness"))
        {
            let plugin = Arc::clone(plugin);
            let event = event.clone();
            tokio::task::spawn_blocking(move || {
                plugin.call(
                    serde_json::json!({"kind":"harness","operation":"event","arguments":event}),
                )
            })
            .await
            .map_err(|error| PluginError::EnvCall(error.to_string()))??;
        }
        Ok(())
    }

    /// 顺序调用所有声明 `harness` 能力的插件 hook。
    /// @param hook Harness hook 名称。
    /// @param event 已序列化的事件快照。
    pub async fn call_harness_hook(&self, hook: &str, event: Value) -> PluginResult<Vec<Value>> {
        let mut results = Vec::new();
        for plugin in self
            .plugins
            .iter()
            .filter(|plugin| plugin.supports("harness"))
        {
            let plugin = Arc::clone(plugin);
            let hook = hook.to_string();
            let event = event.clone();
            let result = tokio::task::spawn_blocking(move || {
                plugin.call(serde_json::json!({
                    "kind":"harness",
                    "operation":"hook",
                    "name":hook,
                    "arguments":{"event":event},
                }))
            })
            .await
            .map_err(|error| PluginError::EnvCall(error.to_string()))??;
            results.push(result);
        }
        Ok(results)
    }

    /// 返回插件目录。
    pub fn plugin_dir(&self) -> PathBuf {
        self.app_dir.join("plugins")
    }

    /// 加载插件并在首次访问前初始化四个全局注册表。
    fn load(options: PluginRuntimeOptions) -> PluginResult<Self> {
        let plugin_dir = options.app_dir.join("plugins");
        fs::create_dir_all(&plugin_dir)?;
        let mut plugins = Vec::new();
        for entry in fs::read_dir(&plugin_dir)? {
            let path = entry?.path();
            if is_dynamic_library(&path) {
                plugins.push(Arc::new(load_plugin(&path)?));
            }
        }
        let env_factory: Arc<dyn ExecutionEnvFactory> = plugins
            .iter()
            .find(|plugin| plugin.supports("env"))
            .map(|plugin| {
                Arc::new(PluginExecutionEnvFactory::new(Arc::clone(plugin)))
                    as Arc<dyn ExecutionEnvFactory>
            })
            .unwrap_or_else(|| Arc::new(LocalExecutionEnvFactory));

        let mut providers = HashMap::new();
        let mut tools = HashMap::new();
        let mut searches = HashMap::new();
        for plugin in &plugins {
            for contribution in &plugin.providers {
                providers.insert(
                    contribution.name.clone(),
                    Arc::new(PluginApiProvider::new(
                        Arc::clone(plugin),
                        contribution.name.clone(),
                    )) as Arc<dyn ai::model::ApiProvider>,
                );
            }
            for contribution in &plugin.tools {
                let name = contribution.name.clone();
                let tool = PluginAgentTool::new(Arc::clone(plugin), contribution.clone())
                    .map_err(PluginError::EnvCall)?;
                tools.insert(name, Arc::new(tool) as Arc<dyn ai::agent::AgentTool>);
            }
            for contribution in &plugin.searches {
                let name = contribution.name.clone();
                let search = PluginSearchEngine::new(Arc::clone(plugin), contribution.clone())
                    .map_err(PluginError::EnvCall)?;
                searches.insert(
                    name,
                    Arc::new(search) as Arc<dyn tool::search::SearchEngine>,
                );
            }
        }

        ApiRegistry::global_with_extensions(providers).map_err(PluginError::EnvCall)?;
        SearchRegistry::global_with_extensions(searches).map_err(PluginError::EnvCall)?;
        ToolRegistry::global_with_extensions(tools).map_err(PluginError::EnvCall)?;

        Ok(Self {
            app_dir: options.app_dir,
            plugins,
            env_factory,
        })
    }
}

/// 判断路径是否是当前平台动态库。
fn is_dynamic_library(path: &Path) -> bool {
    let extension = path.extension().and_then(|value| value.to_str());
    #[cfg(target_os = "macos")]
    {
        extension == Some("dylib")
    }
    #[cfg(target_os = "windows")]
    {
        extension == Some("dll")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        extension == Some("so")
    }
}

/// 加载单个动态库并读取其 ABI 描述符。
fn load_plugin(path: &Path) -> PluginResult<PluginHandle> {
    let library = unsafe { Library::new(path) }.map_err(|error| PluginError::LoadLibrary {
        path: path.to_path_buf(),
        error,
    })?;
    let entry = unsafe { library.get::<PluginEntryV1>(PLUGIN_ENTRY_SYMBOL) }.map_err(|error| {
        PluginError::Entry {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let host_context = Box::new(HostEnvContext {
        runtime: tokio::runtime::Handle::current(),
    });
    let host = HostApiV1 {
        abi_version: PLUGIN_ABI_VERSION,
        context: (&*host_context as *const HostEnvContext).cast_mut().cast(),
        log: host_log,
        env_call: host_env_call,
    };
    let mut descriptor = PluginDescriptorV1 {
        abi_version: 0,
        plugin_context: std::ptr::null_mut(),
        manifest: PluginJsonBytes {
            data: std::ptr::null(),
            len: 0,
            free: None,
        },
        call: missing_call,
    };
    let status = unsafe { entry(&host, &mut descriptor) };
    if status != PluginStatus::Ok {
        return Err(PluginError::Entry {
            path: path.to_path_buf(),
            message: format!("状态: {status:?}"),
        });
    }
    if descriptor.abi_version != PLUGIN_ABI_VERSION {
        return Err(PluginError::AbiVersion {
            path: path.to_path_buf(),
            expected: PLUGIN_ABI_VERSION,
            actual: descriptor.abi_version,
        });
    }
    let manifest_result = json_bytes(
        descriptor.manifest.data,
        descriptor.manifest.len,
        &path.display().to_string(),
    )
    .and_then(|bytes| {
        serde_json::from_slice(bytes).map_err(|error| PluginError::Protocol {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
    });
    free_json_bytes(descriptor.manifest);
    let manifest: PluginManifest = manifest_result?;
    Ok(PluginHandle {
        _library: library,
        _host_context: host_context,
        context: descriptor.plugin_context,
        call_fn: descriptor.call,
        id: manifest.id,
        capabilities: manifest.capabilities,
        providers: manifest.providers,
        tools: manifest.tools,
        searches: manifest.searches,
    })
}

/// 将 ABI 指针范围校验为字节切片。
/// @param data 插件返回的起始地址。
/// @param len 插件返回的字节长度。
/// @param source 用于诊断的插件来源。
fn json_bytes<'a>(data: *const u8, len: usize, source: &str) -> PluginResult<&'a [u8]> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(PluginError::Protocol {
            path: PathBuf::from(source),
            message: "非空 JSON 缓冲区的 data 为空".to_string(),
        });
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

/// 释放插件所有的 JSON 缓冲区。
/// @param bytes 插件返回的字节缓冲区。
fn free_json_bytes(bytes: PluginJsonBytes) {
    if let Some(free) = bytes.free {
        unsafe { free(bytes.data, bytes.len) };
    }
}

/// 宿主默认插件日志回调。
unsafe extern "C" fn host_log(_context: *mut c_void, event: JsonBytes) {
    let bytes = unsafe { std::slice::from_raw_parts(event.data, event.len) };
    eprintln!("插件诊断: {}", String::from_utf8_lossy(bytes));
}

/// 通过宿主默认 LocalExecutionEnv 执行插件 JSON 请求。
unsafe extern "C" fn host_env_call(
    context: *mut c_void,
    request: JsonBytes,
    output: *mut PluginJsonBytes,
) -> PluginStatus {
    if context.is_null() || output.is_null() || (request.len > 0 && request.data.is_null()) {
        return PluginStatus::InvalidArgument;
    }
    let context = unsafe { &*(context.cast::<HostEnvContext>()) };
    let request = unsafe { std::slice::from_raw_parts(request.data, request.len) };
    let request: Value = match serde_json::from_slice(request) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("插件宿主 Env 请求 JSON 无效: {error:?}");
            return PluginStatus::InvalidArgument;
        }
    };
    let response = match context.runtime.block_on(call_default_env(request)) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("插件宿主 Env 调用失败: {error:?}");
            return PluginStatus::Failed;
        }
    };
    let response = match serde_json::to_vec(&response) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("插件宿主 Env 响应序列化失败: {error:?}");
            return PluginStatus::Failed;
        }
    };
    let response = std::mem::ManuallyDrop::new(response);
    unsafe {
        *output = PluginJsonBytes {
            data: response.as_ptr(),
            len: response.len(),
            free: Some(free_host_json_bytes),
        };
    }
    PluginStatus::Ok
}

/// 释放宿主为 Env 回调分配的 JSON 响应。
unsafe extern "C" fn free_host_json_bytes(data: *const u8, len: usize) {
    if !data.is_null() {
        unsafe { drop(Vec::from_raw_parts(data.cast_mut(), len, len)) };
    }
}

/// 防止未初始化调用函数指针。
unsafe extern "C" fn missing_call(
    _context: *mut c_void,
    _request: JsonBytes,
    _output: *mut PluginJsonBytes,
) -> PluginStatus {
    PluginStatus::Failed
}
