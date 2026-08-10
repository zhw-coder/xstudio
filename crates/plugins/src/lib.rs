//! XStudio 插件运行时。
//!
//! 上层传入应用目录；本 crate 负责发现原生插件、保持动态库句柄并装配插件能力。

pub mod adapters;
pub mod env;
pub mod error;
pub mod runtime;

pub use error::{PluginError, PluginResult};
pub use runtime::{PluginRuntime, PluginRuntimeOptions};
