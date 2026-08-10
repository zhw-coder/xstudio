use serde::{Deserialize, Serialize};

/// 模板文件返回数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateFileOutput {
    /// 模板名，不包含 `.md` 扩展名。
    pub name: String,
    /// 模板所在目录类型。
    pub dir: TemplateDir,
    /// YAML frontmatter 中的可选描述。
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Markdown 模板正文。
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content: String,
}

/// 模板文件列表项返回数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateFileListOutput {
    /// 模板名，不包含 `.md` 扩展名。
    pub name: String,
    /// 模板所在目录类型。
    pub dir: TemplateDir,
}

/// Skill 文件返回数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileOutput {
    /// Skill 稳定名称。
    pub name: String,
    /// Skill 所在目录类型。
    pub dir: TemplateDir,
    /// Skill 文件绝对路径。
    pub path: String,
    /// YAML frontmatter 中的描述。
    pub description: String,
    /// 是否禁止模型自主调用。
    pub disable_model_invocation: bool,
}

impl TemplateFileListOutput {
    /// 构造不读取文件内容的模板列表项。
    /// @param name 模板名。
    /// @param dir 模板所在目录类型。
    pub fn list_item(name: String, dir: TemplateDir) -> Self {
        Self { name, dir }
    }
}

/// 模板存储目录类型。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TemplateDir {
    /// 用户家目录下的全局模板目录。
    Global,
    /// 当前项目下的模板目录。
    Project,
}

/// 保存模板文件请求。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTemplateFileInput {
    /// 模板名，不包含 `.md` 扩展名。
    pub name: String,
    /// 客户端选择的模板目录类型。
    pub dir: TemplateDir,
    /// 写入 YAML frontmatter 的可选描述。
    pub description: String,
    /// 写入文件的 Markdown 模板正文。
    pub content: String,
}

/// 删除模板文件请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTemplateFileInput {
    /// 模板名，不包含 `.md` 扩展名。
    pub name: String,
    /// 客户端选择的模板目录类型。
    pub dir: TemplateDir,
}

/// 更新 Skill 模型自主调用开关请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSkillDisableModelInvocationInput {
    /// Skill 文件绝对路径。
    pub path: String,
    /// 是否禁止模型自主调用。
    pub disable_model_invocation: bool,
}
