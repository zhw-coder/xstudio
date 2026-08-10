import { invoke } from '@tauri-apps/api/core';
import type { ChangeEvent } from 'react';
import { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { IconGlyph, ScrollArea } from '../../components';
import { I18n } from '../../i18n';
import { ReportBackendError } from '../../utils/backendError';

/// Skill 文件的存储位置。
type SkillScope = 'global' | 'project';

/// 后端返回的 Skill 文件。
interface SkillFile {
  description: string;
  dir: SkillScope;
  disableModelInvocation: boolean;
  name: string;
  path: string;
}

/// 查询全部 Skill 文件的后端命令名。
const ListSkillFilesCommand = 'list_skill_files';

/// 更新 Skill 模型自主调用开关的后端命令名。
const SetSkillDisableModelInvocationCommand = 'set_skill_disable_model_invocation';

/// 获取 Skill 的存储位置文案。
/// @param dir Skill 文件的存储位置。
function GetSkillScopeLabel(dir: SkillScope) {
  return dir === 'global' ? I18n.common.globalScope : I18n.common.projectScope;
}

/// 渲染技能启用开关。
/// @param props.enabled 是否允许模型自主调用。
/// @param props.disabled 是否正在保存。
/// @param props.name Skill 名称。
/// @param props.onChange 更新开关回调。
function SkillToggle({
  disabled,
  enabled,
  name,
  onChange,
}: {
  disabled: boolean;
  enabled: boolean;
  name: string;
  onChange: () => void;
}) {
  return (
    <button
      aria-checked={enabled}
      aria-label={enabled ? I18n.skills.disableAria.replace('{name}', name) : I18n.skills.enableAria.replace('{name}', name)}
      className={enabled ? 'SkillToggle SkillToggleEnabled' : 'SkillToggle'}
      disabled={disabled}
      onClick={onChange}
      role="switch"
      type="button"
    >
      <span />
    </button>
  );
}

/// 渲染单条 Skill 文件。
/// @param props.file Skill 文件数据。
/// @param props.updating 是否正在更新该文件。
/// @param props.onToggle 更新模型自主调用开关回调。
function SkillRow({
  file,
  onToggle,
  updating,
}: {
  file: SkillFile;
  onToggle: () => void;
  updating: boolean;
}) {
  const enabled = !file.disableModelInvocation;

  return (
    <article className="SkillFileRow">
      <div className="SkillInfo">
        <span className="ResourceScopeBadge">{GetSkillScopeLabel(file.dir)}</span>
        <h2 title={file.description ? `${file.name}: ${file.description}` : file.name}>
          <span>{file.name}</span>
          {file.description ? <span className="SkillDescription">: {file.description}</span> : null}
        </h2>
      </div>

      <SkillToggle disabled={updating} enabled={enabled} name={file.name} onChange={onToggle} />
    </article>
  );
}

/// 渲染点击左侧 Skills 后展示的右侧面板。
function SkillsPanel() {
  const [files, setFiles] = useState<SkillFile[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchText, setSearchText] = useState('');
  const [updatingPath, setUpdatingPath] = useState<string | null>(null);
  const deferredSearchText = useDeferredValue(searchText);

  /// 从后端加载当前全局和项目的 Skill 文件。
  async function LoadSkillFiles() {
    setLoading(true);

    try {
      setFiles(await invoke<SkillFile[]>(ListSkillFilesCommand));
    } catch (error) {
      ReportBackendError('加载技能列表失败', error);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void LoadSkillFiles();
  }, []);

  const filteredFiles = useMemo(() => {
    const keyword = deferredSearchText.trim().toLocaleLowerCase();

    return files
      .filter((file) => keyword.length === 0
        || file.name.toLocaleLowerCase().includes(keyword)
        || file.description.toLocaleLowerCase().includes(keyword))
      .sort((left, right) => (left.dir === right.dir ? 0 : left.dir === 'global' ? -1 : 1)
        || left.name.localeCompare(right.name));
  }, [deferredSearchText, files]);

  /// 更新 Skill 搜索关键词。
  /// @param event 搜索输入事件。
  function HandleSearchChange(event: ChangeEvent<HTMLInputElement>) {
    setSearchText(event.target.value);
  }

  /// 更新单个 Skill 是否允许模型自主调用。
  /// @param file 待更新的 Skill 文件。
  async function ToggleSkill(file: SkillFile) {
    if (updatingPath !== null) {
      return;
    }

    const disableModelInvocation = !file.disableModelInvocation;
    setUpdatingPath(file.path);

    try {
      await invoke(SetSkillDisableModelInvocationCommand, {
        input: { disableModelInvocation, path: file.path },
      });
      setFiles((currentFiles) => currentFiles.map((currentFile) => currentFile.path === file.path
        ? { ...currentFile, disableModelInvocation }
        : currentFile));
    } catch (error) {
      ReportBackendError('更新技能启用状态失败', error);
    } finally {
      setUpdatingPath(null);
    }
  }

  return (
    <section className="SkillsPanel" aria-labelledby="skills-panel-title">
      <header className="SkillsPanelHeader">
        <div className="SkillsTitleWrap">
          <h1 id="skills-panel-title">{I18n.skills.title}</h1>
          <p>{I18n.skills.description}</p>
        </div>
      </header>

      <div className="SkillsControlsRow">
        <label className="MainSearch" htmlFor="skills-search">
          <IconGlyph name="search" size={16} />
          <input
            id="skills-search"
            onChange={HandleSearchChange}
            placeholder={I18n.skills.searchPlaceholder}
            type="search"
            value={searchText}
          />
        </label>
        <span className="SkillsEnabledBadge">{filteredFiles.length} {I18n.common.itemUnit}</span>
      </div>

      <div className="SkillsListHeader" aria-hidden="true">
        <span>{I18n.skills.name}</span>
        <span>{I18n.skills.enabled}</span>
      </div>

      <div className="SkillsListWrap">
        <ScrollArea ariaLabel={I18n.skills.listAria} className="SkillsList">
          {loading ? <span className="SkillsEmptyText">{I18n.common.loading}</span> : null}
          {!loading && filteredFiles.length === 0 ? <span className="SkillsEmptyText">{I18n.skills.empty}</span> : null}
          {!loading && filteredFiles.map((file) => (
            <SkillRow
              file={file}
              key={file.path}
              onToggle={() => void ToggleSkill(file)}
              updating={updatingPath !== null}
            />
          ))}
        </ScrollArea>
      </div>
    </section>
  );
}

export default SkillsPanel;
