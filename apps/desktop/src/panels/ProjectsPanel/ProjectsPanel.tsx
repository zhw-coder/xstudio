import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useMemo, useState } from 'react';
import { IconGlyph, ScrollArea } from '../../components';
import { I18n } from '../../i18n';
import { ReportBackendError } from '../../utils/backendError';

interface ProjectItem {
  path: string;
  updatedAt: number;
}

interface ProjectsPanelProps {
  onSelectProject: () => void;
  onSelectUpdatedProject: () => Promise<void>;
}

/// 查询全部项目的后端命令名。
const ListProjectsCommand = 'list_projects';

/// 保存单个项目的后端命令名。
const SaveProjectCommand = 'save_project';

/// 删除单个项目的后端命令名。
const DeleteProjectCommand = 'delete_project';

/// 格式化项目最近更新时间。
/// @param updatedAt 项目最近更新时间的 Unix 毫秒时间戳。
function FormatUpdatedAt(updatedAt: number) {
  const elapsedSeconds = Math.max(0, Math.floor((Date.now() - updatedAt) / 1000));

  if (elapsedSeconds < 60) {
    return I18n.projects.updatedAt.replace('{value}', I18n.projects.second.replace('{count}', String(elapsedSeconds)));
  }

  const elapsedMinutes = Math.floor(elapsedSeconds / 60);

  if (elapsedMinutes < 60) {
    return I18n.projects.updatedAt.replace('{value}', I18n.projects.minute.replace('{count}', String(elapsedMinutes)));
  }

  const elapsedHours = Math.floor(elapsedMinutes / 60);

  if (elapsedHours < 24) {
    return I18n.projects.updatedAt.replace('{value}', I18n.projects.hour.replace('{count}', String(elapsedHours)));
  }

  return I18n.projects.updatedAt.replace('{value}', I18n.projects.day.replace('{count}', String(Math.floor(elapsedHours / 24))));
}

/// 渲染单条项目记录。
/// @param props.project 项目记录。
/// @param props.deletable 当前项目是否允许删除。
/// @param props.selected 当前项目是否选中。
/// @param props.saving 是否正在保存项目。
/// @param props.onDelete 删除项目回调。
/// @param props.onSelect 选择项目回调。
function ProjectRow({
  project,
  deletable,
  selected,
  saving,
  onDelete,
  onSelect,
}: {
  project: ProjectItem;
  deletable: boolean;
  selected: boolean;
  saving: boolean;
  onDelete: () => void;
  onSelect: () => void;
}) {
  return (
    <article
      className={selected ? 'SkillRow ProjectRow ProjectRowSelected' : 'SkillRow ProjectRow'}
    >
      <button
        aria-current={selected ? 'true' : undefined}
        className="ProjectRowSelect"
        disabled={saving}
        onClick={onSelect}
        type="button"
      >
        <span className="ProjectPath">{project.path}</span>
      </button>
      <time className="ProjectUpdatedAt" dateTime={new Date(project.updatedAt).toISOString()}>
        {FormatUpdatedAt(project.updatedAt)}
      </time>
      <button
        aria-label={I18n.projects.deleteAria.replace('{path}', project.path)}
        className="MainDeleteButton"
        disabled={saving || !deletable}
        onClick={(event) => {
          event.stopPropagation();
          onDelete();
        }}
        type="button"
      >
        <IconGlyph name="trash-2" size={14} />
      </button>
    </article>
  );
}

/// 渲染点击左侧项目后展示的右侧面板。
/// @param props.onSelectProject 成功选中项目后切换新会话回调。
/// @param props.onSelectUpdatedProject 更新非首行项目后刷新会话并切换新会话回调。
function ProjectsPanel({ onSelectProject, onSelectUpdatedProject }: ProjectsPanelProps) {
  const [projects, setProjects] = useState<ProjectItem[]>([]);
  const [searchText, setSearchText] = useState('');
  const [saving, setSaving] = useState(false);
  const normalizedSearchText = searchText.trim().toLocaleLowerCase();
  const filteredProjects = useMemo(
    () => projects.filter((project) => project.path.toLocaleLowerCase().includes(normalizedSearchText)),
    [normalizedSearchText, projects]
  );
  const activeProjectPath = projects[0]?.path;
  const canDeleteProjects = projects.length > 1;

  /// 加载后端保存的全部项目。
  async function LoadProjects() {
    try {
      setProjects(await invoke<ProjectItem[]>(ListProjectsCommand));
    } catch (error) {
      ReportBackendError('加载 list_projects 失败', error);
    }
  }

  /// 选择目录并保存为最近项目。
  async function OpenProject() {
    if (saving) {
      return;
    }

    try {
      const path = await open({
        defaultPath: activeProjectPath,
        directory: true,
        multiple: false,
      });

      if (typeof path !== 'string') {
        return;
      }

      setSaving(true);
      const project = await invoke<ProjectItem>(SaveProjectCommand, {
        input: { path },
      });

      setProjects((items) => [project, ...items.filter((item) => item.path !== project.path)]);
      await onSelectUpdatedProject();
    } catch (error) {
      ReportBackendError('保存 save_project 失败', error);
    } finally {
      setSaving(false);
    }
  }

  /// 删除指定项目，并重新加载项目列表。
  /// @param project 待删除的项目记录。
  async function DeleteProject(project: ProjectItem) {
    if (saving || !canDeleteProjects) {
      return;
    }

    try {
      setSaving(true);
      await invoke(DeleteProjectCommand, {
        input: { path: project.path },
      });
      await LoadProjects();
    } catch (error) {
      ReportBackendError('删除 delete_project 失败', error);
    } finally {
      setSaving(false);
    }
  }

  /// 选择已保存项目，并在需要时更新其最近访问时间。
  /// @param project 用户选择的项目记录。
  async function SelectProject(project: ProjectItem) {
    if (saving) {
      return;
    }

    if (project.path === activeProjectPath) {
      onSelectProject();
      return;
    }

    try {
      setSaving(true);
      const savedProject = await invoke<ProjectItem>(SaveProjectCommand, {
        input: { path: project.path },
      });

      setProjects((items) => [savedProject, ...items.filter((item) => item.path !== savedProject.path)]);
      await onSelectUpdatedProject();
    } catch (error) {
      ReportBackendError('保存 save_project 失败', error);
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    void LoadProjects();
  }, []);

  return (
    <section className="SkillsPanel ProjectsPanel" aria-labelledby="projects-panel-title">
      <header className="SkillsPanelHeader">
        <div className="SkillsTitleWrap">
          <h1 id="projects-panel-title">{I18n.projects.title}</h1>
          <p>{I18n.projects.description}</p>
        </div>

        <div className="SkillsPanelActions">
          <button className="SkillsPrimaryButton" disabled={saving} onClick={OpenProject} type="button">
            <IconGlyph name="folder-kanban" size={16} />
            <span>{I18n.projects.open}</span>
          </button>
        </div>
      </header>

      <div className="SkillsControlsRow">
        <label className="MainSearch" htmlFor="projects-search">
          <IconGlyph name="search" size={16} />
          <input
            id="projects-search"
            placeholder={I18n.projects.searchPlaceholder}
            type="search"
            value={searchText}
            onChange={(event) => setSearchText(event.target.value)}
          />
        </label>
        <span className="SkillsEnabledBadge">{projects.length} {I18n.common.itemUnit}</span>
      </div>

      <div className="SkillsListWrap">
        <ScrollArea ariaLabel={I18n.projects.listAria} className="SkillsList ProjectList">
          {filteredProjects.map((project) => (
            <ProjectRow
              key={project.path}
              project={project}
              deletable={canDeleteProjects}
              saving={saving}
              selected={project.path === activeProjectPath}
              onDelete={() => {
                void DeleteProject(project);
              }}
              onSelect={() => {
                void SelectProject(project);
              }}
            />
          ))}
          {filteredProjects.length === 0 ? (
            <p className="ProjectEmptyState">
              {projects.length === 0 ? I18n.projects.empty : I18n.projects.noMatch}
            </p>
          ) : null}
        </ScrollArea>
      </div>
    </section>
  );
}

export default ProjectsPanel;
