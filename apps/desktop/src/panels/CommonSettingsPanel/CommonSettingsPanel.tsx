import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState, type KeyboardEvent, type ReactNode } from 'react';
import { IconGlyph, SelectField, SettingsPanelHeader } from '../../components';
import type { SelectFieldOption } from '../../components';
import { I18n, LocaleLabels } from '../../i18n';
import type { Locale } from '../../i18n';
import type { Config, ConfigStorageType, ConfigTheme } from '../../types/config';
import { ReportBackendError } from '../../utils/backendError';

/// 打开应用数据目录的后端命令名。
const OpenAppDirCommand = 'open_app_dir';

interface CommonSettingsPanelProps {
  config: Config | null;
  loading: boolean;
  sessionRepos: string[];
  saveDisabled: boolean;
  saveError: string;
  saving: boolean;
  onChangeConfig: (config: Config) => void;
  onSave: () => void;
}

/// 渲染会话上下文自动压缩阈值输入框。
/// @param props.config 当前配置草稿。
/// @param props.onChangeConfig 修改配置草稿回调。
function CompactRatioInput({
  config,
  onChangeConfig,
}: {
  config: Config;
  onChangeConfig: (config: Config) => void;
}) {
  const [draft, setDraft] = useState(String(config.compactRatio));

  useEffect(() => {
    setDraft(String(config.compactRatio));
  }, [config.compactRatio]);

  /// 提交有效的 token 使用百分比。
  function CommitCompactRatio() {
    const nextRatio = Number(draft);

    if (!Number.isInteger(nextRatio) || nextRatio < 1 || nextRatio > 100 || nextRatio === config.compactRatio) {
      setDraft(String(config.compactRatio));
      return;
    }

    onChangeConfig({ ...config, compactRatio: nextRatio });
  }

  /// Enter 提交输入值。
  /// @param event 输入框键盘事件。
  function HandleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key !== 'Enter') {
      return;
    }

    event.currentTarget.blur();
  }

  return (
    <div className="CommonCompactRatioControl">
      <input
        aria-label={I18n.settings.compactRatioTitle}
        className="CommonPreferenceValue CommonCompactRatioInput"
        max={100}
        min={1}
        onBlur={CommitCompactRatio}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={HandleKeyDown}
        step={1}
        type="number"
        value={draft}
      />
      <span aria-hidden="true">%</span>
    </div>
  );
}

interface PreferenceItem {
  copy: string;
  title: string;
}

/// 语言下拉选项。
const LanguageOptions: SelectFieldOption<Locale>[] = (Object.keys(LocaleLabels) as Locale[]).map((locale) => ({
  label: LocaleLabels[locale],
  value: locale,
}));

/// 主题下拉选项。
const ThemeOptions: SelectFieldOption<ConfigTheme>[] = [
  { label: 'light', value: 'light' },
  { label: 'dark', value: 'dark' },
];

/// 渲染工作路径选择按钮。
/// @param props.config 当前配置草稿。
/// @param props.onChangeConfig 修改配置草稿回调。
function PathValueButton({
  config,
  onChangeConfig,
}: {
  config: Config;
  onChangeConfig: (config: Config) => void;
}) {
  /// 打开目录选择器。
  async function SelectWorkspacePath() {
    try {
      const selectedPath = await open({
        defaultPath: config.path.length > 0 ? config.path : undefined,
        directory: true,
        multiple: false,
      });

      if (typeof selectedPath !== 'string') {
        return;
      }

      onChangeConfig({ ...config, path: selectedPath });
    } catch (error) {
      console.error('选择工作路径失败', error);
    }
  }

  return (
    <button
      aria-label={I18n.settings.pathSelectAria}
      className="CommonPreferenceValue CommonPreferencePathButton"
      onClick={SelectWorkspacePath}
      type="button"
    >
      <IconGlyph name="folder-kanban" size={14} />
      <span>{config.path}</span>
    </button>
  );
}

/// 渲染语言下拉框。
/// @param props.config 当前配置草稿。
/// @param props.onChangeConfig 修改配置草稿回调。
function LanguageSelect({
  config,
  onChangeConfig,
}: {
  config: Config;
  onChangeConfig: (config: Config) => void;
}) {
  /// 修改界面语言。
  /// @param language 目标语言。
  function ChangeLanguage(language: Locale) {
    onChangeConfig({ ...config, language });
  }

  return (
    <SelectField
      ariaLabel={I18n.settings.languageTitle}
      className="CommonPreferenceSelect"
      options={LanguageOptions}
      value={config.language}
      width={135}
      onChange={ChangeLanguage}
    />
  );
}

/// 渲染主题下拉框。
/// @param props.config 当前配置草稿。
/// @param props.onChangeConfig 修改配置草稿回调。
function ThemeSelect({
  config,
  onChangeConfig,
}: {
  config: Config;
  onChangeConfig: (config: Config) => void;
}) {
  /// 修改界面主题。
  /// @param theme 目标主题。
  function ChangeTheme(theme: ConfigTheme) {
    onChangeConfig({ ...config, theme });
  }

  return (
    <SelectField
      ariaLabel={I18n.settings.themeTitle}
      className="CommonPreferenceSelect"
      options={ThemeOptions}
      value={config.theme}
      width={105}
      onChange={ChangeTheme}
    />
  );
}

/// 渲染存储类型下拉框。
/// @param props.config 当前配置草稿。
/// @param props.onChangeConfig 修改配置草稿回调。
function StorageTypeSelect({
  config,
  onChangeConfig,
  sessionRepos,
}: {
  config: Config;
  onChangeConfig: (config: Config) => void;
  sessionRepos: string[];
}) {
  /// 修改存储类型。
  /// @param value 目标下拉框值。
  function ChangeStorageType(value: ConfigStorageType) {
    onChangeConfig({ ...config, storageType: value });
  }

  return (
    <SelectField
      ariaLabel={I18n.settings.storageTypeTitle}
      className="CommonPreferenceSelect"
      options={sessionRepos.map((name) => ({ label: name, value: name }))}
      value={config.storageType}
      width={113}
      onChange={ChangeStorageType}
    />
  );
}

/// 渲染偏好设置条目。
/// @param props.action 右侧操作控件。
/// @param props.item 偏好设置项。
function PreferenceRow({ action, item }: { action?: ReactNode; item: PreferenceItem }) {
  return (
    <section className="CommonPreferenceRow" aria-label={item.title}>
      <div className="CommonPreferenceText">
        <h2>{item.title}</h2>
        <p>{item.copy}</p>
      </div>
      {action}
    </section>
  );
}

/// 渲染点击 Settings / 通用后展示的右侧面板。
/// @param props.config 当前配置草稿。
/// @param props.loading 配置是否正在加载。
/// @param props.saveDisabled 保存按钮是否禁用。
/// @param props.saveError 保存错误信息。
/// @param props.saving 配置是否正在保存。
/// @param props.onChangeConfig 修改配置草稿回调。
/// @param props.onSave 保存配置回调。
function CommonSettingsPanel({
  config,
  loading,
  onChangeConfig,
  onSave,
  saveDisabled,
  saveError,
  saving,
  sessionRepos,
}: CommonSettingsPanelProps) {
  const [openingAppDir, setOpeningAppDir] = useState(false);
  const preferenceItems = {
    compactRatio: { copy: I18n.settings.compactRatioCopy, title: I18n.settings.compactRatioTitle },
    language: { copy: I18n.settings.languageCopy, title: I18n.settings.languageTitle },
    path: { copy: I18n.settings.pathCopy, title: I18n.settings.pathTitle },
    storageType: { copy: I18n.settings.storageTypeCopy, title: I18n.settings.storageTypeTitle },
    theme: { copy: I18n.settings.themeCopy, title: I18n.settings.themeTitle },
  };

  /// 在系统文件管理器中打开应用目录。
  async function OpenAppDir() {
    if (openingAppDir) {
      return;
    }

    setOpeningAppDir(true);
    try {
      await invoke(OpenAppDirCommand);
    } catch (error) {
      ReportBackendError(I18n.settings.appDirectoryOpenError, error);
    } finally {
      setOpeningAppDir(false);
    }
  }

  return (
    <section
      aria-busy={loading || saving}
      className="CommonSettingsPanel"
      aria-labelledby="common-settings-title"
    >
      <SettingsPanelHeader
        description={I18n.settings.commonDescription}
        saveDisabled={saveDisabled}
        saveError={saveError}
        title={I18n.settings.commonTitle}
        titleId="common-settings-title"
        onSave={onSave}
      />

      <div className="CommonSettingsContent">
        <span className="CommonPreferenceSectionLabel">{I18n.settings.preferenceSection}</span>

        <div className="CommonPreferenceList">
          {config === null ? (
            <PreferenceRow
              item={{
                copy: saveError.length > 0 ? saveError : I18n.settings.commonDescription,
                title: I18n.settings.loadingConfig,
              }}
            />
          ) : (
            <>
              <PreferenceRow
                action={<PathValueButton config={config} onChangeConfig={onChangeConfig} />}
                item={preferenceItems.path}
              />
              <PreferenceRow
                action={<LanguageSelect config={config} onChangeConfig={onChangeConfig} />}
                item={preferenceItems.language}
              />
              <PreferenceRow
                action={<ThemeSelect config={config} onChangeConfig={onChangeConfig} />}
                item={preferenceItems.theme}
              />
              <PreferenceRow
                action={(
                  <StorageTypeSelect
                    config={config}
                    sessionRepos={sessionRepos}
                    onChangeConfig={onChangeConfig}
                  />
                )}
                item={preferenceItems.storageType}
              />
              <PreferenceRow
                action={(
                  <CompactRatioInput
                    config={config}
                    onChangeConfig={onChangeConfig}
                  />
                )}
                item={preferenceItems.compactRatio}
              />
            </>
          )}
        </div>
        <footer className="CommonSettingsFooter">
          <button
            aria-label={I18n.settings.appDirectoryButton}
            className="CommonAppDirectoryButton"
            disabled={loading || saving || openingAppDir}
            onClick={() => void OpenAppDir()}
            type="button"
          >
            <IconGlyph name="folder-kanban" size={14} />
            <span>{I18n.settings.appDirectoryButton}</span>
          </button>
        </footer>
      </div>
    </section>
  );
}

export default CommonSettingsPanel;
