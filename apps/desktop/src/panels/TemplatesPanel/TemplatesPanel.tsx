import { invoke } from '@tauri-apps/api/core';
import type { ChangeEvent } from 'react';
import { useEffect, useMemo, useState } from 'react';
import { IconGlyph, ScrollArea, SettingsPanelHeader } from '../../components';
import { I18n } from '../../i18n';
import { ReportBackendError } from '../../utils/backendError';

/// 后端返回的模板文件。
interface TemplateFile {
  content: string;
  description: string;
  dir: TemplateScope;
  name: string;
}

/// 后端返回的模板文件列表项。
interface TemplateFileListItem {
  dir: TemplateScope;
  name: string;
}

/// 模板的存储作用域。
type TemplateScope = 'global' | 'project';

/// 模板文件读取命令名。
const ListTemplateFilesCommand = 'list_template_files';

/// 单个模板文件读取命令名。
const GetTemplateFileCommand = 'get_template_file';

/// 模板文件保存命令名。
const SaveTemplateFileCommand = 'save_template_file';

/// 模板文件删除命令名。
const DeleteTemplateFileCommand = 'delete_template_file';

/// 获取模板的存储位置文案。
/// @param dir 模板的存储位置。
function GetTemplateScopeLabel(dir: TemplateScope) {
  return dir === 'global' ? I18n.common.globalScope : I18n.common.projectScope;
}

/// 渲染模板列表的一行。
/// @param props.file 模板文件数据。
/// @param props.deleting 当前模板是否正在删除。
/// @param props.onDelete 删除模板回调。
/// @param props.onEdit 打开模板编辑回调。
function TemplateRow({
  deleting,
  file,
  onDelete,
  onEdit,
}: {
  deleting: boolean;
  file: TemplateFileListItem;
  onDelete: () => void;
  onEdit: () => void;
}) {
  return (
    <article className="TemplateRow">
      <div className="TemplateInfo">
        <span className="ResourceScopeBadge">{GetTemplateScopeLabel(file.dir)}</span>
        <h2>{file.name}</h2>
      </div>
      <div className="TemplateActions">
        <button aria-label={I18n.templates.editAria.replace('{name}', file.name)} className="MainEditButton" disabled={deleting} onClick={onEdit} type="button">
          <IconGlyph name="square-pen" size={14} />
        </button>
        <button aria-label={I18n.templates.deleteAria.replace('{name}', file.name)} className="MainDeleteButton" disabled={deleting} onClick={onDelete} type="button">
          <IconGlyph name="trash-2" size={14} />
        </button>
      </div>
    </article>
  );
}

/// 渲染模板编辑二级界面。
/// @param props.file 当前编辑的模板文件。
/// @param props.isNew 是否为新增模板。
/// @param props.onBack 返回列表回调。
/// @param props.onSaved 保存成功回调。
function TemplateEditor({
  file,
  isNew,
  onBack,
  onSaved,
}: {
  file: TemplateFile;
  isNew: boolean;
  onBack: () => void;
  onSaved: (file: TemplateFile) => void;
}) {
  const [content, setContent] = useState(file.content);
  const [description, setDescription] = useState(file.description);
  const [initialFile, setInitialFile] = useState(file);
  const [templateName, setTemplateName] = useState(file.name);
  const [templateScope, setTemplateScope] = useState<TemplateScope>(file.dir);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState('');
  const isValid = templateName.trim().length > 0 && content.trim().length > 0;
  const isDirty = content !== initialFile.content
    || description !== initialFile.description
    || templateName.trim() !== initialFile.name;

  /// 更新模板名。
  /// @param event 模板名输入事件。
  function HandleTemplateNameChange(event: ChangeEvent<HTMLInputElement>) {
    setTemplateName(event.target.value);
  }

  /// 更新模板描述。
  /// @param event 描述输入事件。
  function HandleDescriptionChange(event: ChangeEvent<HTMLInputElement>) {
    setDescription(event.target.value);
  }

  /// 更新模板正文。
  /// @param event 正文输入事件。
  function HandleContentChange(event: ChangeEvent<HTMLTextAreaElement>) {
    setContent(event.target.value);
  }

  /// 更新新模板的存储作用域。
  /// @param event 作用域选择事件。
  function HandleTemplateScopeChange(event: ChangeEvent<HTMLInputElement>) {
    setTemplateScope(event.target.value as TemplateScope);
  }

  /// 保存模板文件到应用模板目录。
  async function SaveTemplate() {
    if (!isValid || !isDirty || saving) {
      return;
    }

    setSaving(true);
    setSaveError('');

    try {
      await invoke(SaveTemplateFileCommand, {
        input: {
          content,
          description,
          dir: isNew ? templateScope : file.dir,
          name: templateName.trim(),
        },
      });
      const savedFile = {
        content,
        description,
        dir: isNew ? templateScope : file.dir,
        name: templateName.trim(),
      };
      setInitialFile(savedFile);
      onSaved(savedFile);
    } catch (error) {
      setSaveError(ReportBackendError(I18n.templates.saveError, error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="TemplateEditor" aria-labelledby="template-editor-title">
      <SettingsPanelHeader
        description={I18n.templates.editorDescription}
        onSave={SaveTemplate}
        saveDisabled={!isValid || !isDirty || saving}
        saveError={saveError}
        title={isNew ? I18n.templates.newTitle : file.name}
        titleId="template-editor-title"
      />

      <div className="TemplateEditorBody">
        <fieldset className="TemplateStorageField">
          <legend className="SrOnly">{I18n.templates.storageLocation}</legend>
          <label className={`TemplateStorageOption${templateScope === 'global' ? ' TemplateStorageOptionSelected' : ''}${!isNew ? ' TemplateStorageOptionDisabled' : ''}`}>
            <input checked={templateScope === 'global'} className="SrOnly" disabled={!isNew} name="template-scope" onChange={HandleTemplateScopeChange} type="radio" value="global" />
            <span>{I18n.common.globalScope}</span>
          </label>
          <label className={`TemplateStorageOption${templateScope === 'project' ? ' TemplateStorageOptionSelected' : ''}${!isNew ? ' TemplateStorageOptionDisabled' : ''}`}>
            <input checked={templateScope === 'project'} className="SrOnly" disabled={!isNew} name="template-scope" onChange={HandleTemplateScopeChange} type="radio" value="project" />
            <span>{I18n.common.projectScope}</span>
          </label>
        </fieldset>
        <label className="TemplateFormField">
          <span>{I18n.templates.name}</span>
          <input
            aria-label={I18n.templates.name}
            className="TemplateFormInput"
            disabled={!isNew}
            onChange={HandleTemplateNameChange}
            placeholder="template"
            spellCheck={false}
            value={templateName}
          />
        </label>
        <label className="TemplateFormField">
          <span>{I18n.templates.descriptionLabel}</span>
          <input
            aria-label={I18n.templates.descriptionLabel}
            className="TemplateFormInput"
            onChange={HandleDescriptionChange}
            placeholder={I18n.templates.descriptionPlaceholder}
            value={description}
          />
        </label>
        <label className="TemplateFormField TemplateFormContentField">
          <span>{I18n.templates.content}</span>
          <textarea
            aria-label={I18n.templates.content}
            className="TemplateFormTextarea"
            onChange={HandleContentChange}
            placeholder={I18n.templates.contentPlaceholder}
            spellCheck={false}
            value={content}
          />
        </label>
      </div>

      <button aria-label={I18n.templates.backToListAria} className="TemplateBackButton" onClick={onBack} type="button">
        <IconGlyph name="arrow-left" size={16} />
        <span>{I18n.templates.backToList}</span>
      </button>
    </section>
  );
}

/// 渲染点击侧栏模板后展示的模板文件管理面板。
function TemplatesPanel() {
  const [files, setFiles] = useState<TemplateFileListItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingFile, setEditingFile] = useState<TemplateFile | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [deletingTemplateName, setDeletingTemplateName] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');

  /// 加载应用模板目录中的模板文件。
  async function LoadTemplateFiles() {
    setLoading(true);

    try {
      setFiles(await invoke<TemplateFileListItem[]>(ListTemplateFilesCommand));
    } catch (error) {
      ReportBackendError('加载模板列表失败', error);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void LoadTemplateFiles();
  }, []);

  const filteredFiles = useMemo(
    () => files
      .filter((file) => file.name.toLowerCase().includes(searchTerm.trim().toLowerCase()))
      .sort((left, right) => (left.dir === right.dir ? 0 : left.dir === 'global' ? -1 : 1)
        || left.name.localeCompare(right.name)),
    [files, searchTerm],
  );

  /// 更新模板文件搜索关键词。
  /// @param event 搜索输入事件。
  function HandleSearchChange(event: ChangeEvent<HTMLInputElement>) {
    setSearchTerm(event.target.value);
  }

  /// 打开现有模板的二级编辑界面。
  /// @param file 模板文件列表项。
  async function EditTemplate(file: TemplateFileListItem) {
    try {
      const selectedFile = await invoke<TemplateFile>(GetTemplateFileCommand, { dir: file.dir, name: file.name });
      setIsNew(false);
      setEditingFile(selectedFile);
    } catch (error) {
      ReportBackendError('读取模板失败', error);
    }
  }

  /// 打开新增模板的二级编辑界面。
  function CreateTemplate() {
    setIsNew(true);
    setEditingFile({ content: '', description: '', dir: 'global', name: '' });
  }

  /// 删除指定模板文件并同步列表。
  /// @param file 待删除模板文件。
  async function DeleteTemplate(file: TemplateFileListItem) {
    setDeletingTemplateName(file.name);

    try {
      await invoke(DeleteTemplateFileCommand, { input: { dir: file.dir, name: file.name } });
      setFiles((currentFiles) => currentFiles.filter((currentFile) => currentFile.dir !== file.dir || currentFile.name !== file.name));
    } catch (error) {
      ReportBackendError('删除模板失败', error);
    } finally {
      setDeletingTemplateName(null);
    }
  }

  /// 保存成功后同步列表并留在二级编辑界面。
  /// @param file 已保存模板文件。
  function HandleTemplateSaved(file: TemplateFile) {
    setFiles((currentFiles) => {
      const existingIndex = currentFiles.findIndex((currentFile) => currentFile.name === file.name);

      if (existingIndex < 0) {
        return [...currentFiles, file];
      }

      return currentFiles.map((currentFile) => currentFile.name === file.name ? file : currentFile);
    });
    setIsNew(false);
    setEditingFile(file);
  }

  /// 返回模板文件列表。
  function ReturnToList() {
    setEditingFile(null);
    void LoadTemplateFiles();
  }

  if (editingFile !== null) {
    return (
      <TemplateEditor
        file={editingFile}
        isNew={isNew}
        onBack={ReturnToList}
        onSaved={HandleTemplateSaved}
      />
    );
  }

  return (
    <section className="TemplatesPanel" aria-labelledby="templates-panel-title">
      <header className="TemplatesPanelHeader">
        <div className="TemplatesTitleWrap">
          <h1 id="templates-panel-title">{I18n.templates.title}</h1>
          <p>{I18n.templates.description}</p>
        </div>
        <button className="TemplatesAddButton" onClick={CreateTemplate} type="button">
          <IconGlyph name="plus" size={16} />
          <span>{I18n.templates.add}</span>
        </button>
      </header>

      <div className="TemplatesControlsRow">
        <label className="MainSearch" htmlFor="templates-search">
          <IconGlyph name="search" size={16} />
          <input
            id="templates-search"
            onChange={HandleSearchChange}
            placeholder={I18n.templates.searchPlaceholder}
            type="search"
            value={searchTerm}
          />
        </label>
        <span className="TemplatesCountBadge">{filteredFiles.length} {I18n.common.itemUnit}</span>
      </div>

      <div className="TemplatesListHeader" aria-hidden="true">
        <span>{I18n.templates.name}</span>
      </div>

      <div className="TemplatesListWrap">
        <ScrollArea ariaLabel={I18n.templates.listAria} className="TemplatesList">
          {loading ? <span className="TemplatesEmptyText">{I18n.common.loading}</span> : null}
          {!loading && filteredFiles.length === 0 ? <span className="TemplatesEmptyText">{I18n.templates.empty}</span> : null}
          {!loading ? filteredFiles.map((file) => (
            <TemplateRow
              deleting={deletingTemplateName === file.name}
              file={file}
              key={file.name}
              onDelete={() => void DeleteTemplate(file)}
              onEdit={() => void EditTemplate(file)}
            />
          )) : null}
        </ScrollArea>
      </div>
    </section>
  );
}

export default TemplatesPanel;
