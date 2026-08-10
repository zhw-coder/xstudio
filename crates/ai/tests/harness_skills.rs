//! Harness Skill 格式化集成测试。

use ai::agent::harness::{format_skills_for_system_prompt, Skill};

/// 验证系统提示词仅公开允许模型调用的 Skill，并转义 XML 特殊字符。
#[test]
fn formats_visible_skills_for_system_prompt() {
    let skills = vec![
        Skill {
            name: "review<&".to_string(),
            description: "review > code".to_string(),
            content: String::new(),
            file_path: "/skills/review'\".md".to_string(),
            disable_model_invocation: false,
        },
        Skill {
            name: "private".to_string(),
            description: "hidden".to_string(),
            content: String::new(),
            file_path: "/skills/private.md".to_string(),
            disable_model_invocation: true,
        },
    ];

    assert_eq!(
        format_skills_for_system_prompt(&skills),
        "The following skills provide specialized instructions for specific tasks.\nRead the full skill file when the task matches its description.\nWhen a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n<available_skills>\n  <skill>\n    <name>review&lt;&amp;</name>\n    <description>review &gt; code</description>\n    <location>/skills/review&apos;&quot;.md</location>\n  </skill>\n</available_skills>"
    );
}
