//! moodflow-plugin-host：L1 沙箱插件宿主（ADR 0054）。
//!
//! 用 wasmi（纯解释器，无 JIT、无可写可执行内存）执行插件；同一个 `.wasm` 在 Windows 与 Android 通用。
//!
//! # ABI（`sdk = 1`）
//!
//! 插件导出：
//! - `memory`、`mf_alloc(len: i32) -> i32`（宿主写入参数前向插件要一块内存）；
//! - 按扩展点导出 `mf_candidates` / `mf_transform` / `mf_cmd` / `mf_buffer_action`，签名都是
//!   `(ptr: i32, len: i32) -> i64`：入参是 UTF-8 JSON，返回值高 32 位为结果指针、低 32 位为长度。
//!
//! 宿主导入（模块 `mf`），**每个都要对应的能力**：
//!
//! | 函数 | 能力 |
//! |---|---|
//! | `log(ptr, len)` | 无（宿主只在 debug 级别记录，`info` 不落正文） |
//! | `storage_get(kptr, klen, out_ptr, out_cap) -> i32` | `storage` |
//! | `storage_set(kptr, klen, vptr, vlen) -> i32` | `storage` |
//! | `notify(ptr, len) -> i32` | `notify` |
//!
//! 加载时就检查导入：导入了没声明能力的函数、或导入了不存在的函数，直接拒绝加载（市场审核 CI 用同一套检查）。
//!
//! # 运行约束
//!
//! - 每次调用设执行步数上限（fuel）与内存上限；超限只作废这一次调用。
//! - 连续 [`Limits::max_failures`] 次超限或陷阱后**自动停用**，之后的调用直接返回 [`CallError::Disabled`]。
//! - 输入隐私：[`InputContext`] 表示密码框、安全桌面、无痕或黑名单应用时，**不调用插件**，直接返回空结果。

use std::collections::BTreeMap;

use moodflow_plugin_api::{Kind, Manifest};
use serde::{Deserialize, Serialize};
use wasmi::{
    Caller, Config, Engine, Extern, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

/// 资源上限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 每次调用的执行步数上限（逐键扩展点要小，宿主可按扩展点给不同值）。
    pub fuel_per_call: u64,
    /// 插件线性内存上限（字节）。
    pub max_memory_bytes: usize,
    /// 单次返回结果的最大字节数。
    pub max_output_bytes: usize,
    /// 连续失败多少次后停用。
    pub max_failures: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel_per_call: 2_000_000,
            max_memory_bytes: 16 * 1024 * 1024,
            max_output_bytes: 256 * 1024,
            max_failures: 3,
        }
    }
}

/// 调用时的输入环境。任何一项为真，插件都收不到输入内容（ADR 0054 第 5 条）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputContext {
    pub private_field: bool,
    pub secure_desktop: bool,
    pub incognito: bool,
    pub app_blocked: bool,
}

impl InputContext {
    pub fn input_allowed(&self) -> bool {
        !(self.private_field || self.secure_desktop || self.incognito || self.app_blocked)
    }
}

/// 宿主为插件提供的能力实现。所有方法都已按插件 id 隔离。
pub trait HostServices: Send {
    fn storage_get(&mut self, plugin: &str, key: &str) -> Option<Vec<u8>>;
    fn storage_set(&mut self, plugin: &str, key: &str, value: &[u8]) -> bool;
    fn notify(&mut self, plugin: &str, text: &str);
}

/// 内存实现，测试与「不落盘」场景用。
#[derive(Debug, Default)]
pub struct MemoryHost {
    pub storage: BTreeMap<(String, String), Vec<u8>>,
    pub notices: Vec<(String, String)>,
}

impl HostServices for MemoryHost {
    fn storage_get(&mut self, plugin: &str, key: &str) -> Option<Vec<u8>> {
        self.storage
            .get(&(plugin.to_owned(), key.to_owned()))
            .cloned()
    }
    fn storage_set(&mut self, plugin: &str, key: &str, value: &[u8]) -> bool {
        self.storage
            .insert((plugin.to_owned(), key.to_owned()), value.to_vec());
        true
    }
    fn notify(&mut self, plugin: &str, text: &str) {
        self.notices.push((plugin.to_owned(), text.to_owned()));
    }
}

/// 插件给出的候选。排序以 `weight` 为准（AGENTS.md：候选排序必须落到 weight）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginCandidate {
    pub text: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    NotWasm,
    Compile(String),
    /// 导入了宿主没有的函数或不在 `mf` 模块里的东西。
    UnknownImport(String),
    /// 导入了需要某能力的函数，但清单没声明该能力。
    MissingCapability {
        import: String,
        capability: &'static str,
    },
    /// 缺少必需的导出（`memory`、`mf_alloc` 或声明的扩展点函数）。
    MissingExport(&'static str),
    Instantiate(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// 插件已因连续失败被停用。
    Disabled,
    /// 清单没声明这个扩展点。
    NotContributed,
    OutOfFuel,
    MemoryLimit,
    Trap(String),
    /// 插件返回的结果越界、过大或不是期望的 JSON。
    BadOutput,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for LoadError {}
impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for CallError {}

struct State {
    plugin_id: String,
    storage: bool,
    notify: bool,
    host: Box<dyn HostServices>,
    limits: StoreLimits,
}

type ExtFn = TypedFunc<(i32, i32), i64>;

/// 一个已加载的插件实例。
pub struct PluginInstance {
    manifest: Manifest,
    store: Store<State>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    candidates: Option<ExtFn>,
    transform: Option<ExtFn>,
    cmd: Option<ExtFn>,
    buffer_action: Option<ExtFn>,
    limits: Limits,
    failures: u32,
    disabled: bool,
}

/// 宿主提供的导入及其所需能力。
const IMPORTS: &[(&str, Option<&str>)] = &[
    ("log", None),
    ("storage_get", Some("storage")),
    ("storage_set", Some("storage")),
    ("notify", Some("notify")),
];

impl PluginInstance {
    /// 加载插件。`manifest` 应已通过 [`Manifest::validate`]。
    pub fn load(
        manifest: Manifest,
        wasm: &[u8],
        limits: Limits,
        host: Box<dyn HostServices>,
    ) -> Result<Self, LoadError> {
        if !matches!(manifest.kind, Kind::Wasm { .. }) {
            return Err(LoadError::NotWasm);
        }
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| LoadError::Compile(e.to_string()))?;

        let caps = &manifest.capabilities;
        for import in module.imports() {
            let name = import.name();
            let Some((_, need)) = (import.module() == "mf")
                .then(|| IMPORTS.iter().find(|(n, _)| *n == name))
                .flatten()
            else {
                return Err(LoadError::UnknownImport(format!(
                    "{}::{}",
                    import.module(),
                    name
                )));
            };
            let granted = match need {
                None => true,
                Some("storage") => caps.storage,
                Some("notify") => caps.notify,
                Some(_) => false,
            };
            if !granted {
                return Err(LoadError::MissingCapability {
                    import: name.to_owned(),
                    capability: need.unwrap_or(""),
                });
            }
        }

        let state = State {
            plugin_id: manifest.id.clone(),
            storage: caps.storage,
            notify: caps.notify,
            host,
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.max_memory_bytes)
                .instances(1)
                .memories(1)
                .build(),
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(limits.fuel_per_call)
            .map_err(|e| LoadError::Instantiate(e.to_string()))?;

        let mut linker: Linker<State> = Linker::new(&engine);
        define_imports(&mut linker).map_err(|e| LoadError::Instantiate(e.to_string()))?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| LoadError::Instantiate(e.to_string()))?;

        let memory = match instance.get_export(&store, "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(LoadError::MissingExport("memory")),
        };
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "mf_alloc")
            .map_err(|_| LoadError::MissingExport("mf_alloc"))?;
        let c = &manifest.contributes;
        let get = |name: &'static str, wanted: bool| -> Result<Option<ExtFn>, LoadError> {
            if !wanted {
                return Ok(None);
            }
            typed(&instance, &store, name)
                .map(Some)
                .ok_or(LoadError::MissingExport(name))
        };
        let candidates = get("mf_candidates", c.candidate_provider.is_some())?;
        let transform = get("mf_transform", c.commit_transform.is_some())?;
        let cmd = get("mf_cmd", !c.cmdbar_functions.is_empty())?;
        let buffer_action = get("mf_buffer_action", c.buffer_action.is_some())?;

        Ok(Self {
            manifest,
            store,
            memory,
            alloc,
            candidates,
            transform,
            cmd,
            buffer_action,
            limits,
            failures: 0,
            disabled: false,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// 用户手动重新启用（例如更新了插件之后）。
    pub fn reenable(&mut self) {
        self.disabled = false;
        self.failures = 0;
    }

    /// 候选来源：编码 → 候选。输入不允许时直接返回空，不调用插件。
    pub fn candidates(
        &mut self,
        ctx: &InputContext,
        code: &str,
    ) -> Result<Vec<PluginCandidate>, CallError> {
        let f = self.candidates.ok_or(CallError::NotContributed)?;
        if !ctx.input_allowed() {
            return Ok(Vec::new());
        }
        let out = self.call(f, &serde_json::json!({ "code": code }))?;
        self.settle(serde_json::from_slice(&out))
    }

    /// 上屏变换：文本 → 文本。输入不允许时原样返回。
    pub fn transform(&mut self, ctx: &InputContext, text: &str) -> Result<String, CallError> {
        let f = self.transform.ok_or(CallError::NotContributed)?;
        if !ctx.input_allowed() {
            return Ok(text.to_owned());
        }
        let out = self.call(f, &serde_json::json!({ "text": text }))?;
        #[derive(Deserialize)]
        struct Out {
            text: String,
        }
        self.settle(serde_json::from_slice::<Out>(&out).map(|o| o.text))
    }

    /// 命令直通车函数。
    pub fn cmd(&mut self, name: &str, args: &[String]) -> Result<String, CallError> {
        let f = self.cmd.ok_or(CallError::NotContributed)?;
        if !self
            .manifest
            .contributes
            .cmdbar_functions
            .iter()
            .any(|n| n == name)
        {
            return Err(CallError::NotContributed);
        }
        let out = self.call(f, &serde_json::json!({ "name": name, "args": args }))?;
        #[derive(Deserialize)]
        struct Out {
            value: String,
        }
        self.settle(serde_json::from_slice::<Out>(&out).map(|o| o.value))
    }

    /// 缓冲区动作：Buffer 原文 → 结果文本（结果进 Buffer，由用户确认后投递）。
    pub fn buffer_action(&mut self, ctx: &InputContext, source: &str) -> Result<String, CallError> {
        let f = self.buffer_action.ok_or(CallError::NotContributed)?;
        if !ctx.input_allowed() {
            return Ok(String::new());
        }
        let out = self.call(f, &serde_json::json!({ "source": source }))?;
        #[derive(Deserialize)]
        struct Out {
            text: String,
        }
        self.settle(serde_json::from_slice::<Out>(&out).map(|o| o.text))
    }

    /// 调用插件，取回原始输出。失败计入连续失败数；成功与否要等解析完才定，见 [`Self::settle`]。
    fn call(&mut self, f: ExtFn, input: &serde_json::Value) -> Result<Vec<u8>, CallError> {
        if self.disabled {
            return Err(CallError::Disabled);
        }
        let bytes = serde_json::to_vec(input).expect("JSON 可序列化");
        let result = self.call_raw(f, &bytes);
        if result.is_err() {
            self.note_failure();
        }
        result
    }

    /// 解析完成后结算：成功清零连续失败数，解析失败计一次失败。
    fn settle<T>(&mut self, parsed: Result<T, serde_json::Error>) -> Result<T, CallError> {
        match parsed {
            Ok(v) => {
                self.failures = 0;
                Ok(v)
            }
            Err(_) => {
                self.note_failure();
                Err(CallError::BadOutput)
            }
        }
    }

    fn note_failure(&mut self) {
        self.failures += 1;
        if self.failures >= self.limits.max_failures {
            self.disabled = true;
        }
    }

    fn call_raw(&mut self, f: ExtFn, input: &[u8]) -> Result<Vec<u8>, CallError> {
        self.store
            .set_fuel(self.limits.fuel_per_call)
            .map_err(|e| CallError::Trap(e.to_string()))?;
        let len = i32::try_from(input.len()).map_err(|_| CallError::BadOutput)?;
        let ptr = self.alloc.call(&mut self.store, len).map_err(map_err)?;
        self.memory
            .write(&mut self.store, ptr as u32 as usize, input)
            .map_err(|_| CallError::BadOutput)?;
        let packed = f.call(&mut self.store, (ptr, len)).map_err(map_err)?;
        let out_ptr = (packed as u64 >> 32) as usize;
        let out_len = (packed as u64 & 0xFFFF_FFFF) as usize;
        if out_len > self.limits.max_output_bytes {
            return Err(CallError::BadOutput);
        }
        let mut buf = vec![0u8; out_len];
        self.memory
            .read(&self.store, out_ptr, &mut buf)
            .map_err(|_| CallError::BadOutput)?;
        Ok(buf)
    }
}

fn typed(instance: &Instance, store: &Store<State>, name: &str) -> Option<ExtFn> {
    instance.get_typed_func::<(i32, i32), i64>(store, name).ok()
}

fn map_err(e: wasmi::Error) -> CallError {
    use wasmi::TrapCode;
    match e.as_trap_code() {
        Some(TrapCode::OutOfFuel) => CallError::OutOfFuel,
        Some(TrapCode::GrowthOperationLimited) | Some(TrapCode::OutOfSystemMemory) => {
            CallError::MemoryLimit
        }
        _ => CallError::Trap(e.to_string()),
    }
}

/// 从插件内存读一段 UTF-8。越界或非 UTF-8 返回 `None`。
fn read_str(caller: &Caller<'_, State>, ptr: i32, len: i32) -> Option<String> {
    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
        return None;
    };
    let data = mem.data(caller);
    let (start, len) = (ptr as u32 as usize, len as u32 as usize);
    let bytes = data.get(start..start.checked_add(len)?)?;
    String::from_utf8(bytes.to_vec()).ok()
}

fn define_imports(linker: &mut Linker<State>) -> Result<(), wasmi::errors::LinkerError> {
    linker.func_wrap(
        "mf",
        "log",
        |_c: Caller<'_, State>, _ptr: i32, _len: i32| {
            // 插件日志不进 info：正文可能含用户输入。需要排查时在 debug 构建里接 tracing。
        },
    )?;
    linker.func_wrap(
        "mf",
        "storage_get",
        |mut caller: Caller<'_, State>, kptr: i32, klen: i32, out_ptr: i32, out_cap: i32| -> i32 {
            if !caller.data().storage {
                return -3;
            }
            let Some(key) = read_str(&caller, kptr, klen) else {
                return -4;
            };
            let id = caller.data().plugin_id.clone();
            let Some(value) = caller.data_mut().host.storage_get(&id, &key) else {
                return -1;
            };
            if value.len() > out_cap.max(0) as usize {
                return -2;
            }
            let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
                return -4;
            };
            if mem
                .write(&mut caller, out_ptr as u32 as usize, &value)
                .is_err()
            {
                return -4;
            }
            value.len() as i32
        },
    )?;
    linker.func_wrap(
        "mf",
        "storage_set",
        |mut caller: Caller<'_, State>, kptr: i32, klen: i32, vptr: i32, vlen: i32| -> i32 {
            if !caller.data().storage {
                return -3;
            }
            let (Some(key), Some(value)) =
                (read_str(&caller, kptr, klen), read_str(&caller, vptr, vlen))
            else {
                return -4;
            };
            let id = caller.data().plugin_id.clone();
            if caller
                .data_mut()
                .host
                .storage_set(&id, &key, value.as_bytes())
            {
                0
            } else {
                -1
            }
        },
    )?;
    linker.func_wrap(
        "mf",
        "notify",
        |mut caller: Caller<'_, State>, ptr: i32, len: i32| -> i32 {
            if !caller.data().notify {
                return -3;
            }
            let Some(text) = read_str(&caller, ptr, len) else {
                return -4;
            };
            let id = caller.data().plugin_id.clone();
            caller.data_mut().host.notify(&id, &text);
            0
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
