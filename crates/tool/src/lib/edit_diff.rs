/// 单个精确文本替换。
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    /// 原文件中唯一的目标文本。
    pub old_text: String,
    /// 替换后的文本。
    pub new_text: String,
}

/// 归一化换行符为 LF。
pub fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 还原指定换行符风格。
pub fn restore_line_endings(text: String, ending: &str) -> String {
    if ending == "\r\n" {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

/// 拆离 UTF-8 BOM。
pub fn strip_bom(content: &str) -> (&str, &str) {
    content
        .strip_prefix('\u{feff}')
        .map_or(("", content), |text| ("\u{feff}", text))
}

/// 将互不重叠的唯一文本替换应用到原始归一化内容。
pub fn apply_edits(content: &str, edits: &[Edit], path: &str) -> Result<String, String> {
    let mut matches = Vec::with_capacity(edits.len());
    for (index, edit) in edits.iter().enumerate() {
        let old_text = normalize_to_lf(&edit.old_text);
        if old_text.is_empty() {
            return Err(format!(
                "edits[{index}].oldText must not be empty in {path}."
            ));
        }
        let occurrences = content.match_indices(&old_text).collect::<Vec<_>>();
        if occurrences.is_empty() {
            return Err(format!("Could not find edits[{index}] in {path}. The oldText must match exactly including all whitespace and newlines."));
        }
        if occurrences.len() > 1 {
            return Err(format!(
                "Found {} occurrences of edits[{index}] in {path}. Each oldText must be unique.",
                occurrences.len()
            ));
        }
        matches.push((
            occurrences[0].0,
            old_text.len(),
            normalize_to_lf(&edit.new_text),
            index,
        ));
    }
    matches.sort_by_key(|entry| entry.0);
    for pair in matches.windows(2) {
        if pair[0].0 + pair[0].1 > pair[1].0 {
            return Err(format!(
                "edits[{}] and edits[{}] overlap in {path}. Merge them into one edit.",
                pair[0].3, pair[1].3
            ));
        }
    }
    let mut output = content.to_string();
    for (index, length, replacement, _) in matches.into_iter().rev() {
        output.replace_range(index..index + length, &replacement);
    }
    if output == content {
        return Err(format!(
            "No changes made to {path}. The replacements produced identical content."
        ));
    }
    Ok(output)
}
