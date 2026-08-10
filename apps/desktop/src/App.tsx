import { useCallback, useEffect, useRef, useState } from 'react';
import { setTheme as setAppTheme } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ActionButton, IconButton, IconGlyph, ScrollArea } from './components';
import type { IconName } from './components';
import { CurrentLocale, I18n, IsLocale, SetI18nLocale, type MessagesSchema } from './i18n';
import AboutSettingsPanel from './panels/AboutSettingsPanel';
import CommonSettingsPanel from './panels/CommonSettingsPanel';
import ModelSettingsPanel from './panels/ModelSettingsPanel';
import ProjectsPanel from './panels/ProjectsPanel';
import SearchSettingsPanel from './panels/SearchSettingsPanel';
import SkillsPanel from './panels/SkillsPanel';
import TemplatesPanel from './panels/TemplatesPanel';
import ThreadPanel, {
  BuildThreadModelValue,
  DefaultThreadThinkingLevelValue,
  NormalizeThreadModelSelection,
  ParseThreadModelValue,
  type ChatPromptSubmission,
  type ChatSessionContext,
  type PromptImage,
  type ThreadModelOptions,
  type ThreadModelPreference,
  type ThreadModelSelection,
} from './panels/ThreadPanel';
import type { Config, ConfigStorageType, ConfigTheme } from './types/config';
import {
  BackendErrorEventName,
  LogErrorWithStack,
  ReportBackendError,
  type BackendErrorNotice,
} from './utils/backendError';

type AppView = 'thread' | 'templates' | 'skills' | 'project' | 'aboutSettings' | 'commonSettings' | 'modelSettings' | 'searchSettings';
type SidebarMode = 'workspace' | 'settings';
type SettingsSection = 'general' | 'models' | 'search' | 'about';

interface NavItem {
  icon: IconName;
  labelKey: keyof MessagesSchema['sidebar'];
  view?: AppView;
}

interface SettingsNavItem {
  icon: IconName;
  label: string;
  section: SettingsSection;
}

interface SessionMetadata {
  id: string;
  name: string;
  createdAt: string;
  cwd: string;
  path: string;
  parentSessionPath?: string;
}

interface ChatSessionNameEventPayload {
  sessionId: string;
  name: string;
  timestamp: number;
}

interface AiModel {
  id: string;
  provider: string;
  [key: string]: unknown;
}

interface ModelRecord {
  recordKey: string;
  providerName: string;
  modelId: string;
  status: boolean;
  modelJson: string;
}

interface ProviderModels {
  provider: unknown;
  models: ModelRecord[];
}

type ProviderModelsMap = Record<string, ProviderModels>;

type SessionRunningMap = Record<string, boolean>;

interface AllProviderModelsOutput {
  providerModelsMap: ProviderModelsMap;
  apiProviderApis: string[];
}

interface SidebarProps {
  activeSession: SessionMetadata | null;
  activeView: AppView;
  collapsed: boolean;
  sessionRunningById: SessionRunningMap;
  sessions: SessionMetadata[];
  onDeleteSession: (session: SessionMetadata) => void;
  onNewChat: () => void;
  onSelectSession: (session: SessionMetadata) => void;
  onSelectView: (view: AppView) => void;
  onToggleCollapsed: () => void;
}

/// App Shell 左侧导航固定宽度，与主界面优化后的侧栏一致。
const SidebarWidthClass = 'SidebarWidth';

/// 主导航条目。
const navItems: NavItem[] = [
  { icon: 'code-2', labelKey: 'templates', view: 'templates' },
  { icon: 'sparkles', labelKey: 'skills', view: 'skills' },
  { icon: 'bot', labelKey: 'automations' },
  { icon: 'folder-kanban', labelKey: 'project', view: 'project' },
];

/// 获取新会话展示标题。
function GetNewChatTitle() {
  return I18n.sidebar.newChat;
}

/// 线程模型选项后端命令名。
const ThreadModelOptionsCommand = 'provider_model_ids_map';

/// 工具名称列表后端命令名。
const ListToolNamesCommand = 'list_tool_names';

/// 工具启用状态偏好后端命令名。
const SetPreferenceToolEnabledCommand = 'set_preference_tool_enabled';

/// 完整工具偏好后端命令名。
const SetPreferenceToolsCommand = 'set_preference_tools';

/// 审批权限偏好后端命令名。
const SetPreferenceApprovalCommand = 'set_preference_approval';

/// 添加工具偏好后端命令名。
const AddPreferenceToolCommand = 'add_preference_tool';

/// 移除工具偏好后端命令名。
const RemovePreferenceToolCommand = 'remove_preference_tool';

/// 完整 Provider 模型后端命令名。
const AllProviderModelsCommand = 'all_provider_models_map';

/// 默认配置查询后端命令名。
const GetConfigCommand = 'get_config';

/// 默认配置保存后端命令名。
const SetConfigCommand = 'set_config';

/// 会话列表后端命令名。
const ListChatSessionsCommand = 'list_chat_sessions';

/// 打开会话后端命令名。
const OpenChatSessionCommand = 'open_chat_session';

/// 创建会话后端命令名。
const CreateChatSessionCommand = 'create_chat_session';

/// Fork 会话后端命令名。
const ForkChatSessionCommand = 'fork_chat_session';

/// 删除会话后端命令名。
const DeleteChatSessionCommand = 'delete_chat_session';

/// 会话仓储列表后端命令名。
const ListSessionReposCommand = 'list_session_repos';

/// 发送 prompt 后端命令名。
const PromptChatCommand = 'prompt_chat';

/// 发送 Skill 后端命令名。
const SkillChatCommand = 'skill_chat';

/// 发送模板后端命令名。
const TemplateChatCommand = 'template_chat';

/// 终止当前 chat run 后端命令名。
const AbortChatCommand = 'abort_chat';

/// 回撤会话区间后端命令名。
const WithdrawChatTurnCommand = 'withdraw_chat_turn';

/// 编辑并重新发送用户消息后端命令名。
const EditAndPromptChatUserMessageCommand = 'edit_and_prompt_chat_user_message';

/// 模型选择偏好后端命令名。
const SetModelRecordSelectionCommand = 'set_model_record_selection';

/// 模型思考档位偏好后端命令名。
const SetModelThinkingLevelCommand = 'set_model_thinking_level';

/// 会话模型切换后端命令名。
const SetChatModelCommand = 'set_chat_model';

/// 会话 thinking level 切换后端命令名。
const SetChatThinkingLevelCommand = 'set_chat_thinking_level';

/// 会话工具切换后端命令名。
const SetChatToolsCommand = 'set_chat_tools';

/// 会话改名前端事件名。
const ChatSessionNameEventName = 'chat://session-name-event';

/// 后端错误 toast 自动关闭时长。
const BackendErrorToastDurationMs = 5000;

/// 前端默认配置兜底值。
const InitialConfig: Config = {
  compactRatio: 80,
  configKey: 'default',
  language: CurrentLocale,
  path: './',
  storageType: 'SQLite 数据库',
  theme: 'light',
};

/// 默认线程模型选择状态。
const InitialThreadModelSelection: ThreadModelSelection = {
  thinkingLevel: DefaultThreadThinkingLevelValue,
  modelKey: '',
};

/// 归一化后端配置，避免旧数据或异常数据影响前端状态。
/// @param input 后端返回的配置。
function NormalizeConfig(input: Config): Config {
  return {
    ...input,
    language: IsLocale(input.language) ? input.language : InitialConfig.language,
    storageType: typeof input.storageType === 'string' ? input.storageType : InitialConfig.storageType,
    theme: input.theme === 'dark' ? 'dark' : 'light',
  };
}

/// 判断两份配置是否一致。
/// @param left 左侧配置。
/// @param right 右侧配置。
function IsSameConfig(left: Config | null, right: Config | null) {
  if (left === null || right === null) {
    return left === right;
  }

  return (
    left.configKey === right.configKey
    && left.compactRatio === right.compactRatio
    && left.language === right.language
    && left.path === right.path
    && left.storageType === right.storageType
    && left.theme === right.theme
  );
}

/// 获取会话展示标题。
/// @param session 会话元信息。
function GetSessionTitle(session: SessionMetadata) {
  return session.name.trim() || GetNewChatTitle();
}

/// 将模型偏好转换成线程模型选择。
/// @param preference 模型偏好配置。
function PreferenceToThreadModelSelection(preference: ThreadModelPreference): ThreadModelSelection {
  return {
    modelKey: preference.modelRecordSelection,
    thinkingLevel: preference.modelThinkingLevel,
  };
}

/// 构造 chat 模块存储入参。
/// @param currentConfig 当前桌面配置。
function BuildChatStorageInput(currentConfig: Config) {
  return {
    storageType: BuildChatStorageType(currentConfig.storageType),
  };
}

/// 判断消息是否为用户消息。
/// @param message 待判断的会话消息。
function IsUserChatMessage(message: unknown) {
  return typeof message === 'object'
    && message !== null
    && !Array.isArray(message)
    && (message as { role?: unknown }).role === 'user';
}

/// 按后端倒序索引定位用户消息在会话数组中的位置。
/// @param messages 当前会话全部消息。
/// @param agentRunIndex 后端使用的用户消息倒序索引。
function FindChatTurnMessageIndex(messages: unknown[], agentRunIndex: number) {
  const userMessageIndexes = messages.reduce<number[]>((indexes, message, messageIndex) => {
    if (IsUserChatMessage(message)) {
      indexes.push(messageIndex);
    }

    return indexes;
  }, []);

  return userMessageIndexes[userMessageIndexes.length - agentRunIndex - 1] ?? -1;
}

/// 将配置存储类型转换为 chat DTO 字符串。
/// @param storageType 配置中的会话存储类型。
function BuildChatStorageType(storageType: ConfigStorageType): string {
  return storageType;
}

/// 将前端 thinking 字符串转换为 chat DTO 入参。
/// @param thinkingLevel 模型思考档位。
function BuildChatThinkingLevel(thinkingLevel: string) {
  return thinkingLevel === 'off' ? null : thinkingLevel;
}

/// 判断值是否为可传给后端的模型对象。
/// @param value 待判断值。
function IsAiModel(value: unknown): value is AiModel {
  return typeof value === 'object'
    && value !== null
    && !Array.isArray(value)
    && typeof (value as { id?: unknown }).id === 'string';
}

/// 应用界面主题到 CSS 和 Tauri 外壳。
/// @param theme 目标主题。
async function ApplyConfigTheme(theme: ConfigTheme) {
  document.documentElement.dataset.theme = theme;

  try {
    await setAppTheme(theme);
  } catch (error) {
    console.error('切换 Tauri 主题失败', error);
  }
}

/// 渲染主导航按钮。
/// @param props.active 当前导航项是否选中。
/// @param props.item 导航项数据。
/// @param props.onSelect 选中导航项回调。
function NavButton({
  active,
  item,
  onSelect,
}: {
  active: boolean;
  item: NavItem;
  onSelect: () => void;
}) {
  const label = I18n.sidebar[item.labelKey];

  return (
    <ActionButton ariaCurrent={active ? 'page' : undefined} className="NavButton" onClick={onSelect}>
      <IconGlyph className="NavIcon" name={item.icon} />
      <span>{label}</span>
    </ActionButton>
  );
}

/// 渲染会话卡片按钮。
/// @param props.active 当前会话是否选中。
/// @param props.onDelete 删除会话回调。
/// @param props.onSelect 选中会话回调。
/// @param props.running 当前会话是否运行中。
/// @param props.session 会话数据。
function SessionCard({
  active,
  onDelete,
  onSelect,
  running,
  session,
}: {
  active: boolean;
  onDelete: () => void;
  onSelect: () => void;
  running: boolean;
  session: SessionMetadata;
}) {
  const title = GetSessionTitle(session);
  const statusIcon = running ? 'refresh-cw' : 'circle-check-big';
  const statusLabel = running ? I18n.common.running : I18n.common.stopped;

  return (
    <div className={active ? 'SessionCard SessionCardActive' : 'SessionCard'}>
      <ActionButton
        ariaCurrent={active ? 'page' : undefined}
        ariaLabel={`${title}，${statusLabel}`}
        className="SessionSelectButton"
        onClick={onSelect}
      >
        <IconGlyph
          className={running ? 'SessionStatusIcon SessionStatusIconRunning' : 'SessionStatusIcon'}
          name={statusIcon}
          size={16}
        />
        <span className="SessionTitle">{title}</span>
      </ActionButton>
      <IconButton
        ariaLabel={`Delete ${title}`}
        className="SessionDeleteButton"
        icon="trash-2"
        onClick={onDelete}
        size={14}
      />
    </div>
  );
}

/// 渲染 Settings Sidebar 导航按钮。
/// @param props.active 当前设置分组是否选中。
/// @param props.item 设置分组数据。
/// @param props.onSelect 选中设置分组回调。
function SettingsNavButton({
  active,
  item,
  onSelect,
}: {
  active: boolean;
  item: SettingsNavItem;
  onSelect: () => void;
}) {
  return (
    <ActionButton ariaCurrent={active ? 'page' : undefined} className="NavButton" onClick={onSelect}>
      <IconGlyph className="NavIcon" name={item.icon} />
      <span>{item.label}</span>
    </ActionButton>
  );
}

/// 渲染工作台侧栏内容。
/// @param props.activeSession 当前会话元信息。
/// @param props.activeView 当前右侧面板。
/// @param props.collapsed 左侧栏是否折叠。
/// @param props.onDeleteSession 删除会话回调。
/// @param props.onNewChat 新建会话回调。
/// @param props.onOpenSettings 打开设置侧栏回调。
/// @param props.onSelectSession 选中会话回调。
/// @param props.onSelectView 选中主导航面板回调。
/// @param props.onToggleCollapsed 切换侧栏折叠回调。
/// @param props.sessionRunningById 会话运行状态映射。
/// @param props.sessions 会话列表。
function WorkspaceSidebarContent({
  activeSession,
  activeView,
  collapsed,
  onDeleteSession,
  onNewChat,
  onOpenSettings,
  onSelectSession,
  onSelectView,
  onToggleCollapsed,
  sessionRunningById,
  sessions,
}: {
  activeSession: SessionMetadata | null;
  activeView: AppView;
  collapsed: boolean;
  onDeleteSession: (session: SessionMetadata) => void;
  onNewChat: () => void;
  onOpenSettings: () => void;
  onSelectSession: (session: SessionMetadata) => void;
  onSelectView: (view: AppView) => void;
  onToggleCollapsed: () => void;
  sessionRunningById: SessionRunningMap;
  sessions: SessionMetadata[];
}) {
  return (
    <>
      <div className="SidebarTop">
        <section className="PrimaryNav" aria-label="Primary navigation">
          <ActionButton className="NewChatButton" onClick={onNewChat}>
            <IconGlyph name="square-pen" />
            <span>{I18n.sidebar.newChat}</span>
          </ActionButton>

          <nav className="NavList">
            {navItems.map((item) => (
              <NavButton
                active={item.view === activeView}
                item={item}
                key={item.labelKey}
                onSelect={() => item.view && onSelectView(item.view)}
              />
            ))}
          </nav>
        </section>

        <section className="ConversationArea" aria-labelledby="session-heading">
          <header className="SessionHeader">
            <h2 id="session-heading">{I18n.sidebar.sessions}</h2>
            <span>{sessions.length}</span>
          </header>

          <div className="SessionScrollWrap">
            <ScrollArea ariaLabel="Session list" className="HistoryList">
              {sessions.map((session) => (
                <SessionCard
                  active={session.id === activeSession?.id}
                  key={session.id}
                  onDelete={() => {
                    onDeleteSession(session);
                  }}
                  onSelect={() => {
                    onSelectSession(session);
                  }}
                  running={sessionRunningById[session.id] ?? false}
                  session={session}
                />
              ))}
            </ScrollArea>
          </div>
        </section>
      </div>

      <footer className="SidebarFooter">
        <ActionButton className="SettingsButton" onClick={onOpenSettings}>
          <span className="SettingsLeft">
            <IconGlyph name="settings-2" size={16} />
            <span>{I18n.settings.settingsButton}</span>
          </span>
        </ActionButton>
        <IconButton
          ariaLabel={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          className="CollapseButton"
          icon={collapsed ? 'panel-left-open' : 'panel-left-close'}
          onClick={onToggleCollapsed}
        />
      </footer>
    </>
  );
}

/// 渲染设置侧栏内容。
/// @param props.activeSection 当前设置分组。
/// @param props.onBack 返回工作台侧栏回调。
/// @param props.onSelectSection 选中设置分组回调。
/// @param props.onToggleCollapsed 切换侧栏折叠回调。
/// @param props.collapsed 左侧栏是否折叠。
function SettingsSidebarContent({
  activeSection,
  collapsed,
  onBack,
  onSelectSection,
  onToggleCollapsed,
}: {
  activeSection: SettingsSection;
  collapsed: boolean;
  onBack: () => void;
  onSelectSection: (section: SettingsSection) => void;
  onToggleCollapsed: () => void;
}) {
  /// Settings Sidebar 画板中的设置分组。
  const settingsNavItems: SettingsNavItem[] = [
    { icon: 'sliders-horizontal', label: I18n.settings.commonNav, section: 'general' },
    { icon: 'boxes', label: I18n.settings.modelNav, section: 'models' },
    { icon: 'search', label: I18n.settings.searchNav, section: 'search' },
    { icon: 'circle-info', label: I18n.settings.aboutNav, section: 'about' },
  ];

  return (
    <>
      <div className="SettingsSidebarTop">
        <div className="SettingsTopSpacer" aria-hidden="true" />
        <nav className="SettingsNavList" aria-label="Settings navigation">
          {settingsNavItems.map((item) => (
            <SettingsNavButton
              active={item.section === activeSection}
              item={item}
              key={item.section}
              onSelect={() => onSelectSection(item.section)}
            />
          ))}
        </nav>
      </div>

      <footer className="SidebarFooter SettingsSidebarFooter">
        <ActionButton ariaLabel={collapsed ? 'Back' : undefined} className="SettingsBackButton" onClick={onBack}>
          <span className="SettingsBackLeft">
            <IconGlyph name="arrow-left" size={16} />
            <span>{I18n.settings.settingsBackButton}</span>
          </span>
        </ActionButton>
        <IconButton
          ariaLabel={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          className="CollapseButton"
          icon={collapsed ? 'panel-left-open' : 'panel-left-close'}
          onClick={onToggleCollapsed}
        />
      </footer>
    </>
  );
}

/// 渲染左侧 App Shell 导航栏。
/// @param props.activeSession 当前会话元信息。
/// @param props.activeView 当前右侧面板。
/// @param props.collapsed 左侧栏是否折叠。
/// @param props.onDeleteSession 删除会话回调。
/// @param props.onNewChat 新建会话回调。
/// @param props.onSelectSession 选中会话回调。
/// @param props.onSelectView 选中主导航面板回调。
/// @param props.onToggleCollapsed 切换侧栏折叠状态。
/// @param props.sessionRunningById 会话运行状态映射。
/// @param props.sessions 会话列表。
function Sidebar({
  activeSession,
  activeView,
  collapsed,
  onDeleteSession,
  onNewChat,
  onSelectSession,
  onSelectView,
  onToggleCollapsed,
  sessionRunningById,
  sessions,
}: SidebarProps) {
  const [activeSettingsSection, setActiveSettingsSection] = useState<SettingsSection>('general');
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('workspace');

  /// Settings 模式沿用当前侧栏的宽度和折叠过渡。
  const sidebarClassName = `Sidebar ${SidebarWidthClass}${collapsed ? ' SidebarCollapsed' : ''}${
    sidebarMode === 'settings' ? ' SidebarSettingsMode' : ''
  }`;

  /// 打开 Settings Sidebar，并在折叠状态下先展开以展示分组文案。
  function OpenSettingsSidebar() {
    if (collapsed) {
      onToggleCollapsed();
    }

    setSidebarMode('settings');
    setActiveSettingsSection('general');
    onSelectView('commonSettings');
  }

  /// 返回工作台侧栏。
  function BackToWorkspaceSidebar() {
    setSidebarMode('workspace');
    onSelectView('thread');
  }

  /// 选择设置分组。
  /// @param section 设置分组。
  function SelectSettingsSection(section: SettingsSection) {
    setActiveSettingsSection(section);

    if (section === 'general') {
      onSelectView('commonSettings');
    }

    if (section === 'models') {
      onSelectView('modelSettings');
    }

    if (section === 'search') {
      onSelectView('searchSettings');
    }

    if (section === 'about') {
      onSelectView('aboutSettings');
    }
  }

  return (
    <aside className={sidebarClassName} aria-label={sidebarMode === 'settings' ? 'Settings sidebar' : 'App sidebar'}>
      {sidebarMode === 'settings' ? (
        <SettingsSidebarContent
          activeSection={activeSettingsSection}
          collapsed={collapsed}
          onBack={BackToWorkspaceSidebar}
          onSelectSection={SelectSettingsSection}
          onToggleCollapsed={onToggleCollapsed}
        />
      ) : (
        <WorkspaceSidebarContent
          activeSession={activeSession}
          activeView={activeView}
          collapsed={collapsed}
          onDeleteSession={onDeleteSession}
          onNewChat={onNewChat}
          onOpenSettings={OpenSettingsSidebar}
          onSelectSession={onSelectSession}
          onSelectView={onSelectView}
          onToggleCollapsed={onToggleCollapsed}
          sessionRunningById={sessionRunningById}
          sessions={sessions}
        />
      )}
    </aside>
  );
}

/// 渲染右侧主线程区。
/// @param props.activeSessionId 当前会话 id。
/// @param props.activeView 当前右侧面板。
/// @param props.configDraft 当前配置草稿。
/// @param props.configLoading 配置是否正在加载。
/// @param props.configSaveDisabled 保存配置按钮是否禁用。
/// @param props.configSaveError 配置保存错误信息。
/// @param props.configSaving 配置是否正在保存。
/// @param props.modelOptions 线程模型选项。
/// @param props.modelOptionsError 线程模型选项加载错误。
/// @param props.modelOptionsLoading 线程模型选项是否加载中。
/// @param props.modelSelection 当前线程模型选择。
/// @param props.onChangeConfigDraft 修改配置草稿回调。
/// @param props.onChangeThinkingLevel 思考档位选择变化回调。
/// @param props.onChangeModel 模型选择变化回调。
/// @param props.onAbortChat 终止当前会话回调。
/// @param props.onEditAgentRun 编辑并重新发送指定 agent 区间回调。
/// @param props.onForkAgentRun Fork 指定 agent 区间回调。
/// @param props.onPromptEditedAgentRun 发送编辑后消息回调。
/// @param props.onModelSettingsChange 模型设置变更回调。
/// @param props.onSelectProject 成功选择项目后切换新会话回调。
/// @param props.onSelectUpdatedProject 更新非首行项目后刷新会话并切换新会话回调。
/// @param props.onRunningChange 指定会话运行状态变化回调。
/// @param props.onSaveConfig 保存配置回调。
/// @param props.onAppendLocalUserMessage 追加本地用户消息回调。
/// @param props.onSubmitPrompt 提交 prompt 回调。
/// @param props.onWithdrawAgentRun 回撤指定 agent 区间回调。
/// @param props.sessionRunning 当前会话是否运行中。
/// @param props.sessionContext 当前会话上下文。
/// @param props.toolNames 后端返回的工具名称。
/// @param props.toolPreference 当前工具偏好。
/// @param props.title 当前线程标题。
/// @param props.onChangeToolEnabled 切换工具启用状态回调。
/// @param props.onChangeToolSelected 切换指定工具选择状态回调。
/// @param props.onSaveTools 保存完整工具配置回调。
/// @param props.onChangeApproval 切换工具审批权限回调。
function MainPanel({
  activeView,
  activeSessionId,
  config,
  configDraft,
  configLoading,
  configSaveDisabled,
  configSaveError,
  configSaving,
  sessionRepos,
  modelOptions,
  modelOptionsError,
  modelOptionsLoading,
  modelSelection,
  onChangeConfigDraft,
  onChangeThinkingLevel,
  onChangeModel,
  onAbortChat,
  onEditAgentRun,
  onForkAgentRun,
  onPromptEditedAgentRun,
  onModelSettingsChange,
  onSelectProject,
  onSelectUpdatedProject,
  onRunningChange,
  onSaveConfig,
  onAppendLocalUserMessage,
  onSubmitPrompt,
  onWithdrawAgentRun,
  sessionRunning,
  sessionContext,
  toolNames,
  toolPreference,
  title,
  onChangeToolEnabled,
  onChangeToolSelected,
  onSaveTools,
  onChangeApproval,
}: {
  activeView: AppView;
  activeSessionId: string;
  config: Config | null;
  configDraft: Config | null;
  configLoading: boolean;
  configSaveDisabled: boolean;
  configSaveError: string;
  configSaving: boolean;
  sessionRepos: string[];
  modelOptions: ThreadModelOptions | null;
  modelOptionsError: string;
  modelOptionsLoading: boolean;
  modelSelection: ThreadModelSelection;
  onChangeConfigDraft: (config: Config) => void;
  onChangeThinkingLevel: (thinkingLevel: string) => void;
  onChangeModel: (modelKey: string) => void;
  onAbortChat: () => Promise<void>;
  onEditAgentRun: (agentRunIndex: number, text: string) => Promise<void>;
  onForkAgentRun: (agentRunIndex: number) => Promise<void>;
  onPromptEditedAgentRun: (sessionId: string, text: string) => Promise<void>;
  onModelSettingsChange: () => void;
  onSelectProject: () => void;
  onSelectUpdatedProject: () => Promise<void>;
  onRunningChange: (sessionId: string, isRunning: boolean) => void;
  onSaveConfig: () => void;
  onAppendLocalUserMessage: (prompt: string, images: PromptImage[]) => void;
  onSubmitPrompt: (submission: ChatPromptSubmission, images: PromptImage[], userMessageDisplayed: boolean) => Promise<void>;
  onWithdrawAgentRun: (agentRunIndex: number) => Promise<void>;
  sessionRunning: boolean;
  sessionContext: ChatSessionContext | null;
  toolNames: string[];
  toolPreference: ThreadModelPreference | null;
  title: string;
  onChangeToolEnabled: (enabled: boolean) => Promise<void>;
  onChangeToolSelected: (tool: string, selected: boolean) => Promise<void>;
  onSaveTools: (tools: string[]) => Promise<void>;
  onChangeApproval: (approval: number) => Promise<void>;
}) {
  if (activeView === 'aboutSettings') {
    return (
      <section className="MainPanel" aria-labelledby="about-settings-title">
        <AboutSettingsPanel />
      </section>
    );
  }

  if (activeView === 'commonSettings') {
    return (
      <section className="MainPanel" aria-labelledby="common-settings-title">
        <CommonSettingsPanel
          config={configDraft}
          loading={configLoading}
          saveDisabled={configSaveDisabled}
          saveError={configSaveError}
          saving={configSaving}
          sessionRepos={sessionRepos}
          onChangeConfig={onChangeConfigDraft}
          onSave={onSaveConfig}
        />
      </section>
    );
  }

  if (activeView === 'modelSettings') {
    return (
      <section className="MainPanel" aria-labelledby="model-settings-title">
        <ModelSettingsPanel onModelsChange={onModelSettingsChange} />
      </section>
    );
  }

  if (activeView === 'searchSettings') {
    return (
      <section className="MainPanel" aria-labelledby="search-settings-title">
        <SearchSettingsPanel />
      </section>
    );
  }

  if (activeView === 'skills') {
    return (
      <section className="MainPanel" aria-labelledby="skills-panel-title">
        <SkillsPanel />
      </section>
    );
  }

  if (activeView === 'templates') {
    return (
      <section className="MainPanel" aria-labelledby="templates-panel-title">
        <TemplatesPanel />
      </section>
    );
  }

  if (activeView === 'project') {
    return (
      <section className="MainPanel" aria-labelledby="projects-panel-title">
        <ProjectsPanel
          onSelectProject={onSelectProject}
          onSelectUpdatedProject={onSelectUpdatedProject}
        />
      </section>
    );
  }

  return (
    <section className="MainPanel" aria-labelledby="thread-title">
      <ThreadPanel
        activeSessionId={activeSessionId}
        compactRatio={config?.compactRatio ?? InitialConfig.compactRatio}
        modelOptions={modelOptions}
        modelOptionsError={modelOptionsError}
        modelOptionsLoading={modelOptionsLoading}
        modelSelection={modelSelection}
        isRunning={sessionRunning}
        onAbort={onAbortChat}
        onEditAgentRun={onEditAgentRun}
        onForkAgentRun={onForkAgentRun}
        onPromptEditedAgentRun={onPromptEditedAgentRun}
        onChangeThinkingLevel={onChangeThinkingLevel}
        onChangeModel={onChangeModel}
        onRunningChange={onRunningChange}
        onAppendUserMessage={onAppendLocalUserMessage}
        onSubmitPrompt={onSubmitPrompt}
        onWithdrawAgentRun={onWithdrawAgentRun}
        sessionContext={sessionContext}
        toolNames={toolNames}
        toolPreference={toolPreference}
        title={title}
        onChangeToolEnabled={onChangeToolEnabled}
        onChangeToolSelected={onChangeToolSelected}
        onSaveTools={onSaveTools}
        onChangeApproval={onChangeApproval}
      />
    </section>
  );
}

/// 渲染全局后端错误提示。
/// @param props.notice 后端错误提示数据。
/// @param props.onDismiss 关闭提示回调。
function BackendErrorToast({
  notice,
  onDismiss,
}: {
  notice: BackendErrorNotice;
  onDismiss: () => void;
}) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const detailId = `backend-error-detail-${notice.id}`;

  useEffect(() => {
    if (detailsOpen) {
      return undefined;
    }

    const timeoutId = window.setTimeout(onDismiss, BackendErrorToastDurationMs);

    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [detailsOpen, notice.id, onDismiss]);

  /// 切换错误详情展开状态。
  function ToggleDetails() {
    setDetailsOpen((open) => !open);
  }

  return (
    <div className="BackendErrorToastRegion" aria-atomic="true" aria-live="assertive">
      <section className="BackendErrorToast" role="alert">
        <header className="BackendErrorToastHeader">
          <span className="BackendErrorToastIcon" aria-hidden="true">
            <IconGlyph name="circle-alert" size={16} />
          </span>
          <div className="BackendErrorToastText">
            <strong>{I18n.errors.backendTitle}</strong>
            <p>{notice.summary}</p>
          </div>
          <IconButton
            ariaLabel={I18n.errors.dismiss}
            className="BackendErrorToastClose"
            icon="x"
            onClick={onDismiss}
            size={14}
          />
        </header>

        <ActionButton
          ariaControls={detailId}
          ariaExpanded={detailsOpen}
          className="BackendErrorToastDetailsButton"
          onClick={ToggleDetails}
        >
          <span>{detailsOpen ? I18n.errors.hideDetails : I18n.errors.viewDetails}</span>
          <IconGlyph
            className={detailsOpen ? 'BackendErrorToastChevron BackendErrorToastChevronOpen' : 'BackendErrorToastChevron'}
            name="chevron-down"
            size={13}
          />
        </ActionButton>

        {detailsOpen ? (
          <pre className="BackendErrorToastDetail" id={detailId} tabIndex={0}>
            {notice.detail}
          </pre>
        ) : null}
      </section>
    </div>
  );
}

/// 渲染桌面 App Shell 主页。
function App() {
  const [activeSession, setActiveSession] = useState<SessionMetadata | null>(null);
  const [activeView, setActiveView] = useState<AppView>('thread');
  const [backendErrorNotice, setBackendErrorNotice] = useState<BackendErrorNotice | null>(null);
  const [chatSessions, setChatSessions] = useState<SessionMetadata[]>([]);
  const [chatSessionContext, setChatSessionContext] = useState<ChatSessionContext | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [configDraft, setConfigDraft] = useState<Config | null>(null);
  const [configError, setConfigError] = useState('');
  const [configLoading, setConfigLoading] = useState(true);
  const [configSaving, setConfigSaving] = useState(false);
  const [sessionRepos, setSessionRepos] = useState<string[]>([]);
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const [threadModelOptions, setThreadModelOptions] = useState<ThreadModelOptions | null>(null);
  const [threadModelOptionsError, setThreadModelOptionsError] = useState('');
  const [threadModelOptionsLoading, setThreadModelOptionsLoading] = useState(false);
  const [threadModelSelection, setThreadModelSelection] = useState<ThreadModelSelection>(
    InitialThreadModelSelection
  );
  const [threadModelPreference, setThreadModelPreference] = useState<ThreadModelPreference | null>(null);
  const [threadToolNames, setThreadToolNames] = useState<string[]>([]);
  const [sessionRunningById, setSessionRunningById] = useState<SessionRunningMap>({});
  const activeSessionRef = useRef<SessionMetadata | null>(null);
  const configRef = useRef<Config | null>(null);
  const providerModelsMapCacheRef = useRef<ProviderModelsMap | null>(null);
  const providerModelsMapLoadPromiseRef = useRef<Promise<ProviderModelsMap | null> | null>(null);
  const threadModelOptionsCacheRef = useRef<ThreadModelOptions | null>(null);
  const threadModelOptionsLoadPromiseRef = useRef<Promise<ThreadModelOptions | null> | null>(null);
  const modelCacheVersionRef = useRef(0);
  const threadModelPreferenceRef = useRef<ThreadModelPreference | null>(null);
  const threadModelSelectionRef = useRef<ThreadModelSelection>(InitialThreadModelSelection);
  const loadingConfigRef = useRef(false);
  const sessionRunningByIdRef = useRef<SessionRunningMap>({});
  const activeSessionId = activeSession?.id ?? '';
  const activeSessionRunning = sessionRunningById[activeSessionId] ?? false;
  const activeTitle = activeSession ? GetSessionTitle(activeSession) : GetNewChatTitle();
  const configSaveDisabled = configLoading || configSaving || configDraft === null || IsSameConfig(config, configDraft);

  /// 关闭当前全局后端错误提示。
  function DismissBackendErrorNotice() {
    setBackendErrorNotice(null);
  }

  /// 应用配置到前端运行态。
  /// @param nextConfig 目标配置。
  function ApplyConfig(nextConfig: Config) {
    SetI18nLocale(nextConfig.language);
    void ApplyConfigTheme(nextConfig.theme);
  }

  /// 写入当前配置状态和同步引用。
  /// @param nextConfig 目标配置。
  function SetCurrentConfig(nextConfig: Config) {
    configRef.current = nextConfig;
    setConfig(nextConfig);
    setConfigDraft(nextConfig);
  }

  /// 写入当前会话状态和同步引用。
  /// @param nextSession 目标会话元信息。
  function SetActiveThreadSession(nextSession: SessionMetadata | null) {
    activeSessionRef.current = nextSession;
    setActiveSession(nextSession);
  }

  /// 写入全部会话运行状态映射和同步引用。
  /// @param nextMap 目标会话运行状态映射。
  function SetThreadRunningMap(nextMap: SessionRunningMap) {
    sessionRunningByIdRef.current = nextMap;
    setSessionRunningById(nextMap);
  }

  /// 写入指定会话运行状态和同步引用。
  /// @param sessionId 目标会话 id，空字符串代表待创建的新会话。
  /// @param isRunning 目标会话是否运行中。
  const SetThreadSessionRunning = useCallback((sessionId: string, isRunning: boolean) => {
    const currentIsRunning = sessionRunningByIdRef.current[sessionId] ?? false;

    if (currentIsRunning === isRunning) {
      return;
    }

    const nextMap = { ...sessionRunningByIdRef.current };

    if (isRunning) {
      nextMap[sessionId] = true;
    } else {
      delete nextMap[sessionId];
    }

    sessionRunningByIdRef.current = nextMap;
    setSessionRunningById(nextMap);
  }, []);

  /// 判断指定会话是否运行中。
  /// @param sessionId 目标会话 id。
  function IsThreadSessionRunning(sessionId: string) {
    return sessionRunningByIdRef.current[sessionId] ?? false;
  }

  /// 写入线程模型选择状态和同步引用。
  /// @param nextSelection 目标模型选择。
  function SetThreadModelSelectionState(nextSelection: ThreadModelSelection) {
    threadModelSelectionRef.current = nextSelection;
    setThreadModelSelection(nextSelection);
  }

  /// 写入线程模型偏好状态和同步引用。
  /// @param nextPreference 目标模型偏好。
  function SetThreadModelPreferenceState(nextPreference: ThreadModelPreference) {
    threadModelPreferenceRef.current = nextPreference;
    setThreadModelPreference(nextPreference);
  }

  /// 判断模型选择是否相同。
  /// @param model 当前会话模型选择。
  /// @param modelKey 线程模型组合值。
  function IsSameThreadModel(model: ChatSessionContext['model'], modelKey: string) {
    const selection = ParseThreadModelValue(modelKey);

    return Boolean(
      model
      && selection
      && model.provider === selection.providerName
      && model.modelId === selection.modelId
    );
  }

  /// 加载会话列表，并保持后端返回数组顺序。
  /// @param currentConfig 当前桌面配置。
  async function LoadChatSessions(currentConfig: Config) {
    try {
      const output = await invoke<SessionMetadata[]>(ListChatSessionsCommand, {
        input: {
          ...BuildChatStorageInput(currentConfig),
        },
      });

      setChatSessions(output);
    } catch (error) {
      ReportBackendError('加载 list_chat_sessions 失败', error);
      setChatSessions([]);
    }
  }

  /// 加载默认配置。
  async function LoadConfig() {
    if (loadingConfigRef.current) {
      return;
    }

    loadingConfigRef.current = true;
    setConfigError('');
    setConfigLoading(true);

    try {
      const [output, repos] = await Promise.all([
        invoke<Config>(GetConfigCommand),
        invoke<string[]>(ListSessionReposCommand),
      ]);
      const nextConfig = NormalizeConfig(output);

      setSessionRepos(repos);
      ApplyConfig(nextConfig);
      SetCurrentConfig(nextConfig);
      await LoadChatSessions(nextConfig);
    } catch (error) {
      setConfigError(ReportBackendError('加载 get_config 失败', error));
    } finally {
      loadingConfigRef.current = false;
      setConfigLoading(false);
    }
  }

  /// 修改配置草稿并立即切换本地运行态。
  /// @param nextConfig 目标配置草稿。
  function ChangeConfigDraft(nextConfig: Config) {
    ApplyConfig(nextConfig);
    setConfigDraft(nextConfig);
  }

  /// 保存配置草稿。
  async function SaveConfig() {
    if (configDraft === null || configSaving) {
      return;
    }

    setConfigError('');
    setConfigSaving(true);

    try {
      const output = await invoke<Config>(SetConfigCommand, { config: configDraft });
      const nextConfig = NormalizeConfig(output);

      ApplyConfig(nextConfig);
      SetCurrentConfig(nextConfig);
      SetActiveThreadSession(null);
      SetThreadRunningMap({});
      setChatSessionContext(null);
      await LoadChatSessions(nextConfig);
    } catch (error) {
      setConfigError(ReportBackendError('保存 set_config 失败', error));
    } finally {
      setConfigSaving(false);
    }
  }

  /// 只在没有缓存时加载线程模型选项。
  async function LoadThreadModelOptionsIfNeeded() {
    if (threadModelOptionsCacheRef.current !== null) {
      return threadModelOptionsCacheRef.current;
    }

    if (threadModelOptionsLoadPromiseRef.current !== null) {
      return threadModelOptionsLoadPromiseRef.current;
    }

    /// 当前模型缓存版本，用于忽略失效前开始的加载结果。
    const cacheVersion = modelCacheVersionRef.current;
    const promise = (async () => {
      setThreadModelOptionsError('');
      setThreadModelOptionsLoading(true);

      try {
        const output = await invoke<ThreadModelOptions>(ThreadModelOptionsCommand);

        /// 工具名称依赖模型偏好请求完成后加载，以保持会话输入区配置一致。
        let toolNames: string[] = [];
        try {
          toolNames = await invoke<string[]>(ListToolNamesCommand);
        } catch (error) {
          ReportBackendError('加载 list_tool_names 失败', error);
        }

        if (cacheVersion !== modelCacheVersionRef.current) {
          return null;
        }

        const baseSelection = activeSessionRef.current === null
          ? PreferenceToThreadModelSelection(output.preference)
          : threadModelSelectionRef.current;
        const nextSelection = NormalizeThreadModelSelection(output, baseSelection);

        threadModelOptionsCacheRef.current = output;
        SetThreadModelPreferenceState(output.preference);
        SetThreadModelSelectionState(nextSelection);
        setThreadModelOptions(output);
        setThreadToolNames(toolNames);
        return output;
      } catch (error) {
        if (cacheVersion !== modelCacheVersionRef.current) {
          return null;
        }

        ReportBackendError('加载 provider_model_ids_map 失败', error);
        setThreadModelOptionsError(ThreadModelOptionsCommand);
        return null;
      } finally {
        if (cacheVersion === modelCacheVersionRef.current) {
          threadModelOptionsLoadPromiseRef.current = null;
          setThreadModelOptionsLoading(false);
        }
      }
    })();

    threadModelOptionsLoadPromiseRef.current = promise;
    return promise;
  }

  /// 只在没有缓存时加载完整模型记录。
  async function LoadProviderModelsMapIfNeeded() {
    if (providerModelsMapCacheRef.current !== null) {
      return providerModelsMapCacheRef.current;
    }

    if (providerModelsMapLoadPromiseRef.current !== null) {
      return providerModelsMapLoadPromiseRef.current;
    }

    /// 当前模型缓存版本，用于忽略失效前开始的加载结果。
    const cacheVersion = modelCacheVersionRef.current;
    const promise = (async () => {
      try {
        const output = await invoke<AllProviderModelsOutput>(AllProviderModelsCommand);

        if (cacheVersion !== modelCacheVersionRef.current) {
          return null;
        }

        providerModelsMapCacheRef.current = output.providerModelsMap;
        return output.providerModelsMap;
      } catch (error) {
        if (cacheVersion !== modelCacheVersionRef.current) {
          return null;
        }

        LogErrorWithStack('加载 all_provider_models_map 失败', error);
        return null;
      } finally {
        if (cacheVersion === modelCacheVersionRef.current) {
          providerModelsMapLoadPromiseRef.current = null;
        }
      }
    })();

    providerModelsMapLoadPromiseRef.current = promise;
    return promise;
  }

  /// 清空模型缓存并重新加载线程模型选项。
  function RefreshThreadModelCaches() {
    modelCacheVersionRef.current += 1;
    providerModelsMapCacheRef.current = null;
    providerModelsMapLoadPromiseRef.current = null;
    threadModelOptionsCacheRef.current = null;
    threadModelOptionsLoadPromiseRef.current = null;
    setThreadModelOptions(null);
    setThreadModelOptionsError('');
    void LoadThreadModelOptionsIfNeeded();
  }

  /// 通过模型下拉值解析完整模型对象。
  /// @param modelKey Provider 和模型 ID 组合值。
  async function ResolveModelByKey(modelKey: string) {
    const selection = ParseThreadModelValue(modelKey);

    if (selection === null) {
      throw new Error('未选择可用模型');
    }

    const providerModelsMap = await LoadProviderModelsMapIfNeeded();
    const record = providerModelsMap?.[selection.providerName]?.models.find(
      (modelRecord) => modelRecord.modelId === selection.modelId && modelRecord.status
    );

    if (!record) {
      throw new Error(`未找到模型: ${selection.providerName}/${selection.modelId}`);
    }

    const model = JSON.parse(record.modelJson) as unknown;

    if (!IsAiModel(model)) {
      throw new Error(`模型数据格式无效: ${selection.providerName}/${selection.modelId}`);
    }

    return {
      ...model,
      id: selection.modelId,
      provider: selection.providerName,
    };
  }

  /// 解析当前线程模型选择对应的完整模型对象。
  async function ResolveCurrentModel() {
    const options = await LoadThreadModelOptionsIfNeeded();
    const nextSelection = options
      ? NormalizeThreadModelSelection(options, threadModelSelectionRef.current)
      : threadModelSelectionRef.current;

    SetThreadModelSelectionState(nextSelection);
    return ResolveModelByKey(nextSelection.modelKey);
  }

  /// 应用打开会话返回的上下文。
  /// @param sessionContext 打开会话返回的上下文。
  function ApplyChatSessionContext(sessionContext: ChatSessionContext) {
    const nextSelection = {
      modelKey: sessionContext.model
        ? BuildThreadModelValue(sessionContext.model.provider, sessionContext.model.modelId)
        : threadModelSelectionRef.current.modelKey,
      thinkingLevel: sessionContext.thinkingLevel || DefaultThreadThinkingLevelValue,
    };
    const options = threadModelOptionsCacheRef.current;
    const normalizedSelection = options ? NormalizeThreadModelSelection(options, nextSelection) : nextSelection;

    setChatSessionContext(sessionContext);
    SetThreadModelSelectionState(normalizedSelection);
  }

  /// 重新读取当前活跃会话的持久化上下文。
  async function ReloadActiveChatSessionContext() {
    const session = activeSessionRef.current;
    const currentConfig = configRef.current;

    if (session === null || currentConfig === null) {
      return;
    }

    try {
      const sessionContext = await invoke<ChatSessionContext>(OpenChatSessionCommand, {
        input: {
          ...BuildChatStorageInput(currentConfig),
          metadata: session,
        },
      });

      if (activeSessionRef.current?.id === session.id) {
        ApplyChatSessionContext(sessionContext);
      }
    } catch (error) {
      ReportBackendError('重新加载 open_chat_session 失败', error);
    }
  }

  /// 切换右侧主面板。
  /// @param view 目标主面板。
  function SelectView(view: AppView) {
    setActiveView(view);

    if (view === 'thread') {
      void LoadThreadModelOptionsIfNeeded();
      void ReloadActiveChatSessionContext();
    }
  }

  /// 开始无 SessionMetadata 的新会话状态。
  function StartNewChat() {
    const options = threadModelOptionsCacheRef.current;
    const preference = threadModelPreference ?? threadModelPreferenceRef.current;

    SetActiveThreadSession(null);
    SetThreadSessionRunning('', false);
    setChatSessionContext(null);
    setActiveView('thread');

    if (options && preference) {
      SetThreadModelSelectionState(NormalizeThreadModelSelection(options, PreferenceToThreadModelSelection(preference)));
    }

    void LoadThreadModelOptionsIfNeeded();
  }

  /// 重新加载会话列表后切换到新会话。
  async function ReloadChatSessionsAndStartNewChat() {
    const currentConfig = configRef.current;

    if (currentConfig !== null) {
      await LoadChatSessions(currentConfig);
    }

    StartNewChat();
  }

  /// 选择会话并回到会话面板。
  /// @param session 目标会话元信息。
  async function SelectThreadSession(session: SessionMetadata) {
    const currentConfig = configRef.current;

    setActiveView('thread');
    void LoadThreadModelOptionsIfNeeded();

    if (currentConfig === null) {
      return;
    }

    try {
      const sessionContext = await invoke<ChatSessionContext>(OpenChatSessionCommand, {
        input: {
          ...BuildChatStorageInput(currentConfig),
          metadata: session,
        },
      });

      SetActiveThreadSession(session);
      ApplyChatSessionContext(sessionContext);
    } catch (error) {
      ReportBackendError('打开 open_chat_session 失败', error);
    }
  }

  /// 删除会话并同步移除界面记录。
  /// @param session 目标会话元信息。
  async function DeleteThreadSession(session: SessionMetadata) {
    const currentConfig = configRef.current;

    if (currentConfig === null) {
      return;
    }

    try {
      await invoke(DeleteChatSessionCommand, {
        input: {
          ...BuildChatStorageInput(currentConfig),
          metadata: session,
        },
      });

      setChatSessions((sessions) => sessions.filter((item) => item.id !== session.id));
      SetThreadSessionRunning(session.id, false);

      if (activeSessionRef.current?.id === session.id) {
        SetActiveThreadSession(null);
        setChatSessionContext(null);
      }
    } catch (error) {
      ReportBackendError('删除 delete_chat_session 失败', error);
    }
  }

  /// 回撤指定 agent 区间及其后续消息。
  /// @param agentRunIndex 后端使用的用户消息倒序索引。
  async function WithdrawAgentRun(agentRunIndex: number) {
    const session = activeSessionRef.current;
    const currentConfig = configRef.current;

    if (session === null || currentConfig === null) {
      return;
    }

    try {
      await invoke(WithdrawChatTurnCommand, {
        input: {
          sessionId: session.id,
          index: agentRunIndex,
        },
      });

      /// 回撤会清除前端流式缓存；重新读取活跃 leaf 以保留此前已完成的 AI 回复。
      const sessionContext = await invoke<ChatSessionContext>(OpenChatSessionCommand, {
        input: {
          ...BuildChatStorageInput(currentConfig),
          metadata: session,
        },
      });

      if (activeSessionRef.current?.id !== session.id) {
        return;
      }

      setChatSessionContext(sessionContext);
      SetThreadSessionRunning(session.id, false);
    } catch (error) {
      ReportBackendError('回撤 withdraw_chat_turn 失败', error);
      throw error;
    }
  }

  /// Fork 指定 agent 区间之前的会话，并自动打开新会话。
  /// @param agentRunIndex 后端使用的用户消息倒序索引。
  async function ForkAgentRun(agentRunIndex: number) {
    const session = activeSessionRef.current;
    const currentConfig = configRef.current;

    if (session === null || currentConfig === null) {
      return;
    }

    try {
      const forkedSession = await invoke<SessionMetadata>(ForkChatSessionCommand, {
        input: {
          ...BuildChatStorageInput(currentConfig),
          sourceSessionId: session.id,
          index: agentRunIndex,
        },
      });

      setChatSessions((sessions) => [forkedSession, ...sessions]);
      await SelectThreadSession(forkedSession);
    } catch (error) {
      ReportBackendError('Fork fork_chat_session 失败', error);
      throw error;
    }
  }

  /// 编辑指定 agent 区间的首条用户消息并重新发送。
  /// @param agentRunIndex 后端使用的用户消息倒序索引。
  /// @param text 修改后的用户输入。
  async function EditAgentRun(agentRunIndex: number, text: string) {
    const session = activeSessionRef.current;

    if (session === null) {
      return;
    }

    try {
      await invoke(EditAndPromptChatUserMessageCommand, {
        input: {
          sessionId: session.id,
          index: agentRunIndex,
        },
      });

      if (activeSessionRef.current?.id !== session.id) {
        return;
      }

      setChatSessionContext((sessionContext) => {
        if (sessionContext === null) {
          return sessionContext;
        }

        const messageIndex = FindChatTurnMessageIndex(sessionContext.messages, agentRunIndex);

        if (messageIndex < 0) {
          return sessionContext;
        }

        return {
          ...sessionContext,
          messages: [
            ...sessionContext.messages.slice(0, messageIndex),
            {
              role: 'user',
              content: text,
              timestamp: Date.now(),
            },
          ],
        };
      });
    } catch (error) {
      ReportBackendError('编辑 edit_and_prompt_chat_user_message 失败', error);
      throw error;
    }
  }

  /// 对编辑后已回撤的当前会话重新发送用户消息。
  /// @param sessionId 已完成回撤的会话 id。
  /// @param text 修改后的用户输入。
  async function PromptEditedAgentRun(sessionId: string, text: string) {
    try {
      await invoke(PromptChatCommand, {
        input: {
          sessionId,
          text,
          images: null,
        },
      });
    } catch (error) {
      ReportBackendError('编辑后发送 prompt_chat 失败', error);
      throw error;
    }
  }

  /// 持久化线程模型选择。
  /// @param modelKey 模型组合值。
  async function PersistThreadModel(modelKey: string) {
    try {
      const preference = await invoke<ThreadModelPreference>(SetModelRecordSelectionCommand, {
        modelRecordSelection: modelKey,
      });

      SetThreadModelPreferenceState(preference);
    } catch (error) {
      ReportBackendError('更新模型选择失败', error);
    }
  }

  /// 同步会话上下文中的模型选择。
  /// @param modelKey 模型组合值。
  function UpdateChatSessionModelContext(modelKey: string) {
    const parsedSelection = ParseThreadModelValue(modelKey);

    setChatSessionContext((currentContext) => ({
      messages: currentContext?.messages ?? [],
      thinkingLevel: currentContext?.thinkingLevel ?? threadModelSelectionRef.current.thinkingLevel,
      model: parsedSelection ? {
        provider: parsedSelection.providerName,
        modelId: parsedSelection.modelId,
      } : currentContext?.model ?? null,
    }));
  }

  /// 运行中立即切换会话模型。
  /// @param session 当前会话元信息。
  /// @param modelKey 模型组合值。
  async function ApplyRunningThreadModel(session: SessionMetadata, modelKey: string) {
    if (IsSameThreadModel(chatSessionContext?.model ?? null, modelKey)) {
      return;
    }

    try {
      const model = await ResolveModelByKey(modelKey);

      await invoke(SetChatModelCommand, {
        input: {
          sessionId: session.id,
          model,
        },
      });
      UpdateChatSessionModelContext(modelKey);
    } catch (error) {
      ReportBackendError('运行中更新 set_chat_model 失败', error);
    }
  }

  /// 修改线程模型选择。
  /// @param modelKey 模型组合值。
  function ChangeThreadModel(modelKey: string) {
    const session = activeSessionRef.current;
    const nextSelection = {
      ...threadModelSelectionRef.current,
      modelKey,
    };

    SetThreadModelSelectionState(nextSelection);

    if (session === null) {
      void PersistThreadModel(modelKey);
      return;
    }

    if (IsThreadSessionRunning(session.id)) {
      void ApplyRunningThreadModel(session, modelKey);
    }
  }

  /// 持久化线程思考档位。
  /// @param thinkingLevel 模型思考档位。
  async function PersistThreadThinkingLevel(thinkingLevel: string) {
    try {
      const preference = await invoke<ThreadModelPreference>(SetModelThinkingLevelCommand, {
        modelThinkingLevel: thinkingLevel,
      });

      SetThreadModelPreferenceState(preference);
    } catch (error) {
      ReportBackendError('更新 thinking level 失败', error);
    }
  }

  /// 将已保存的工具配置应用到当前已有会话。
  /// @param preference 已保存的模型偏好。
  async function ApplyThreadTools(preference: ThreadModelPreference) {
    const session = activeSessionRef.current;

    if (session === null) {
      return;
    }

    try {
      await invoke(SetChatToolsCommand, {
        input: {
          sessionId: session.id,
          tools: preference.tools,
        },
      });
    } catch (error) {
      ReportBackendError('更新 set_chat_tools 失败', error);
    }
  }

  /// 持久化工具启用状态。
  /// @param enabled 是否启用工具。
  async function ChangeThreadToolEnabled(enabled: boolean) {
    try {
      const preference = await invoke<ThreadModelPreference>(SetPreferenceToolEnabledCommand, {
        input: { enabled },
      });

      SetThreadModelPreferenceState(preference);
      await ApplyThreadTools(preference);
    } catch (error) {
      ReportBackendError('更新工具启用状态失败', error);
    }
  }

  /// 持久化单个工具的选择状态。
  /// @param tool 工具名称。
  /// @param selected 是否选择工具。
  async function ChangeThreadToolSelected(tool: string, selected: boolean) {
    const command = selected ? AddPreferenceToolCommand : RemovePreferenceToolCommand;

    try {
      const preference = await invoke<ThreadModelPreference>(command, {
        input: { tool },
      });

      SetThreadModelPreferenceState(preference);
      await ApplyThreadTools(preference);
    } catch (error) {
      ReportBackendError(`更新工具 ${tool} 失败`, error);
    }
  }

  /// 持久化完整工具配置。
  /// @param tools 工具配置数组。
  async function SaveThreadTools(tools: string[]) {
    try {
      const preference = await invoke<ThreadModelPreference>(SetPreferenceToolsCommand, {
        input: { tools },
      });

      SetThreadModelPreferenceState(preference);
      await ApplyThreadTools(preference);
    } catch (error) {
      ReportBackendError('更新工具配置失败', error);
    }
  }

  /// 持久化工具审批权限。
  /// @param approval 审批权限：0 表示默认审批，1 表示绕过审批。
  async function ChangeThreadApproval(approval: number) {
    try {
      const preference = await invoke<ThreadModelPreference>(SetPreferenceApprovalCommand, {
        input: { approval },
      });

      SetThreadModelPreferenceState(preference);
    } catch (error) {
      ReportBackendError('更新工具审批权限失败', error);
    }
  }

  /// 同步会话上下文中的 thinking level。
  /// @param thinkingLevel 模型思考档位。
  function UpdateChatSessionThinkingContext(thinkingLevel: string) {
    setChatSessionContext((currentContext) => ({
      messages: currentContext?.messages ?? [],
      thinkingLevel,
      model: currentContext?.model ?? null,
    }));
  }

  /// 运行中立即切换会话 thinking level。
  /// @param session 当前会话元信息。
  /// @param thinkingLevel 模型思考档位。
  async function ApplyRunningThreadThinkingLevel(session: SessionMetadata, thinkingLevel: string) {
    if ((chatSessionContext?.thinkingLevel ?? DefaultThreadThinkingLevelValue) === thinkingLevel) {
      return;
    }

    try {
      await invoke(SetChatThinkingLevelCommand, {
        input: {
          sessionId: session.id,
          thinkingLevel: BuildChatThinkingLevel(thinkingLevel),
        },
      });
      UpdateChatSessionThinkingContext(thinkingLevel);
    } catch (error) {
      ReportBackendError('运行中更新 set_chat_thinking_level 失败', error);
    }
  }

  /// 修改线程思考档位选择。
  /// @param thinkingLevel 模型思考档位。
  function ChangeThreadThinkingLevel(thinkingLevel: string) {
    const session = activeSessionRef.current;
    const nextSelection = {
      ...threadModelSelectionRef.current,
      thinkingLevel,
    };

    SetThreadModelSelectionState(nextSelection);

    if (session === null) {
      void PersistThreadThinkingLevel(thinkingLevel);
      return;
    }

    if (IsThreadSessionRunning(session.id)) {
      void ApplyRunningThreadThinkingLevel(session, thinkingLevel);
    }
  }

  /// 提交已有会话缓存的模型和 thinking level 改动。
  /// @param session 当前会话元信息。
  async function CommitThreadSelectionIfChanged(session: SessionMetadata) {
    const sessionContext = chatSessionContext;
    const selection = threadModelSelectionRef.current;
    const modelChanged = !IsSameThreadModel(sessionContext?.model ?? null, selection.modelKey);
    const thinkingLevelChanged = (sessionContext?.thinkingLevel ?? DefaultThreadThinkingLevelValue) !== selection.thinkingLevel;

    if (modelChanged) {
      const model = await ResolveModelByKey(selection.modelKey);

      await invoke(SetChatModelCommand, {
        input: {
          sessionId: session.id,
          model,
        },
      });
    }

    if (thinkingLevelChanged) {
      await invoke(SetChatThinkingLevelCommand, {
        input: {
          sessionId: session.id,
          thinkingLevel: BuildChatThinkingLevel(selection.thinkingLevel),
        },
      });
    }

    if (modelChanged || thinkingLevelChanged) {
      const parsedSelection = ParseThreadModelValue(selection.modelKey);

      setChatSessionContext((currentContext) => ({
        messages: currentContext?.messages ?? [],
        thinkingLevel: selection.thinkingLevel,
        model: parsedSelection ? {
          provider: parsedSelection.providerName,
          modelId: parsedSelection.modelId,
        } : currentContext?.model ?? null,
      }));
    }
  }

  /// 构造本地用户消息内容。
  /// @param text 用户输入文本。
  /// @param images 用户输入图片。
  function BuildLocalUserMessageContent(text: string, images: PromptImage[]) {
    if (images.length === 0) {
      return text;
    }

    return [
      ...(text ? [{ text, type: 'text' }] : []),
      ...images.map((image) => ({
        data: image.data,
        mimeType: image.mimeType,
        type: 'image',
      })),
    ];
  }

  /// 追加本地用户消息展示。
  /// @param text 用户输入文本。
  /// @param images 用户输入图片。
  function AppendLocalUserMessage(text: string, images: PromptImage[]) {
    const selection = ParseThreadModelValue(threadModelSelectionRef.current.modelKey);

    setChatSessionContext((sessionContext) => ({
      messages: [
        ...(sessionContext?.messages ?? []),
        {
          role: 'user',
          content: BuildLocalUserMessageContent(text, images),
          timestamp: Date.now(),
        },
      ],
      thinkingLevel: sessionContext?.thinkingLevel ?? threadModelSelectionRef.current.thinkingLevel,
      model: sessionContext?.model ?? (selection ? {
        provider: selection.providerName,
        modelId: selection.modelId,
      } : null),
    }));
  }

/// 提交聊天输入到当前或新建会话。
/// @param submission 用户输入及可选资源调用。
/// @param images 用户输入图像。
/// @param userMessageDisplayed 用户消息是否已在聊天区本地展示。
  async function SubmitPrompt(
    submission: ChatPromptSubmission,
    images: PromptImage[],
    userMessageDisplayed: boolean
  ) {
    const text = submission.text.trim();
    const currentConfig = configRef.current;
    const displayText = submission.resource === null
      ? text
      : `/${submission.resource.name}${text ? ` ${text}` : ''}`;

    if ((!text && images.length === 0 && submission.resource === null) || currentConfig === null) {
      return;
    }

    try {
      let session = activeSessionRef.current;

      if (session === null) {
        const model = await ResolveCurrentModel();
        const createdSession = await invoke<SessionMetadata>(CreateChatSessionCommand, {
          input: {
            ...BuildChatStorageInput(currentConfig),
            model,
            thinkingLevel: BuildChatThinkingLevel(threadModelSelectionRef.current.thinkingLevel),
          },
        });

        session = createdSession;
        SetActiveThreadSession(createdSession);
        setActiveView('thread');
        setChatSessionContext({
          messages: [],
          thinkingLevel: threadModelSelectionRef.current.thinkingLevel,
          model: {
            provider: model.provider,
            modelId: model.id,
          },
        });

        await new Promise((resolve) => window.setTimeout(resolve, 0));
      } else {
        await CommitThreadSelectionIfChanged(session);
      }

      if (!userMessageDisplayed) {
        AppendLocalUserMessage(displayText, images);
      }

      if (submission.resource === null) {
        await invoke(PromptChatCommand, {
          input: {
            sessionId: session.id,
            text,
            images: images.length > 0 ? images : null,
          },
        });
      } else if (submission.resource.kind === 'template') {
        await invoke(TemplateChatCommand, {
          input: {
            sessionId: session.id,
            name: submission.resource.name,
            args: text ? text.split(/ +/) : [],
          },
        });
      } else {
        await invoke(SkillChatCommand, {
          input: {
            sessionId: session.id,
            name: submission.resource.name,
            additionalInstructions: text || null,
          },
        });
      }
    } catch (error) {
      ReportBackendError('发送聊天请求失败', error);
      throw error;
    }
  }

  /// 终止当前会话的 Agent run。
  async function AbortCurrentChat() {
    const session = activeSessionRef.current;

    if (session === null) {
      return;
    }

    try {
      await invoke(AbortChatCommand, {
        input: {
          sessionId: session.id,
        },
      });
      SetThreadSessionRunning(session.id, false);
    } catch (error) {
      ReportBackendError('终止 abort_chat 失败', error);
      throw error;
    }
  }

  /// 切换左侧栏折叠状态。
  function ToggleSidebarCollapsed() {
    setIsSidebarCollapsed((collapsed) => !collapsed);
  }

  useEffect(() => {
    /// 接收子面板派发的后端错误提示事件。
    /// @param event 全局后端错误事件。
    function HandleBackendErrorEvent(event: Event) {
      const backendErrorEvent = event as CustomEvent<BackendErrorNotice>;

      if (backendErrorEvent.detail) {
        setBackendErrorNotice(backendErrorEvent.detail);
      }
    }

    window.addEventListener(BackendErrorEventName, HandleBackendErrorEvent);

    return () => {
      window.removeEventListener(BackendErrorEventName, HandleBackendErrorEvent);
    };
  }, []);

  useEffect(() => {
    void LoadConfig();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listen<ChatSessionNameEventPayload>(ChatSessionNameEventName, (event) => {
      const payload = event.payload;
      const active = activeSessionRef.current;

      if (active?.id === payload.sessionId) {
        SetActiveThreadSession({
          ...active,
          name: payload.name,
        });
      }

      setChatSessions((sessions) => {
        const existing = sessions.find((session) => session.id === payload.sessionId);

        if (existing) {
          return sessions.map((session) => (
            session.id === payload.sessionId
              ? { ...session, name: payload.name }
              : session
          ));
        }

        const current = activeSessionRef.current;

        if (current?.id !== payload.sessionId) {
          return sessions;
        }

        return [
          {
            ...current,
            name: payload.name,
          },
          ...sessions,
        ];
      });
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch((error) => {
        console.error('监听会话改名事件失败', error);
      });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (activeView === 'thread') {
      void LoadThreadModelOptionsIfNeeded();
    }
  }, [activeView]);

  return (
    <main className="AppShellPage">
      <div className="Shell">
        <Sidebar
          activeSession={activeSession}
          activeView={activeView}
          collapsed={isSidebarCollapsed}
          sessionRunningById={sessionRunningById}
          sessions={chatSessions}
          onDeleteSession={DeleteThreadSession}
          onNewChat={StartNewChat}
          onSelectSession={SelectThreadSession}
          onSelectView={SelectView}
          onToggleCollapsed={ToggleSidebarCollapsed}
        />
        <div className="SidebarDivider" aria-hidden="true">
          <div />
        </div>
        <MainPanel
          activeSessionId={activeSessionId}
          activeView={activeView}
          config={config}
          configDraft={configDraft}
          configLoading={configLoading}
          configSaveDisabled={configSaveDisabled}
          configSaveError={configError}
          configSaving={configSaving}
          sessionRepos={sessionRepos}
          modelOptions={threadModelOptions}
          modelOptionsError={threadModelOptionsError}
          modelOptionsLoading={threadModelOptionsLoading}
          modelSelection={threadModelSelection}
          onChangeConfigDraft={ChangeConfigDraft}
          onChangeThinkingLevel={ChangeThreadThinkingLevel}
          onChangeModel={ChangeThreadModel}
          onChangeToolEnabled={ChangeThreadToolEnabled}
          onChangeToolSelected={ChangeThreadToolSelected}
          onSaveTools={SaveThreadTools}
          onChangeApproval={ChangeThreadApproval}
          onAbortChat={AbortCurrentChat}
          onEditAgentRun={EditAgentRun}
          onForkAgentRun={ForkAgentRun}
          onPromptEditedAgentRun={PromptEditedAgentRun}
          onModelSettingsChange={RefreshThreadModelCaches}
          onSelectProject={StartNewChat}
          onSelectUpdatedProject={ReloadChatSessionsAndStartNewChat}
          onRunningChange={SetThreadSessionRunning}
          onSaveConfig={SaveConfig}
          onAppendLocalUserMessage={AppendLocalUserMessage}
          onSubmitPrompt={SubmitPrompt}
          onWithdrawAgentRun={WithdrawAgentRun}
          sessionRunning={activeSessionRunning}
          sessionContext={chatSessionContext}
          toolNames={threadToolNames}
          toolPreference={threadModelPreference}
          title={activeTitle}
        />
      </div>
      {backendErrorNotice ? (
        <BackendErrorToast
          key={backendErrorNotice.id}
          notice={backendErrorNotice}
          onDismiss={DismissBackendErrorNotice}
        />
      ) : null}
    </main>
  );
}

export default App;
