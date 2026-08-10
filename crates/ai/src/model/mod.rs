//! 内置精简 OpenAI Responses 文本 Provider；其他 Provider 侧能力通过注册表和 trait 注入。

pub mod api_registry;
pub mod providers;
pub mod stream;
pub mod types;
pub mod utils;

pub use api_registry::*;
pub use stream::*;
pub use types::*;
pub use utils::{diagnostics::*, json_parse::*};
