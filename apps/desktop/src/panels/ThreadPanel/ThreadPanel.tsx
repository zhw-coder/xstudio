import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ChangeEvent, ClipboardEvent, DragEvent, FormEvent, KeyboardEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconButton, IconGlyph, ScrollArea, SelectField } from '../../components';
import type { IconName } from '../../components';
import { I18n } from '../../i18n';
import {
  BuildAssistantMessageView,
  BuildToolResultBlock,
  CreateLocalWaitingRun,
  ExtractMessageText,
  GetMessageRole,
  IsRecord,
} from './harnessEventReducer';
import { ReportBackendError } from '../../utils/backendError';
import {
  BuildThreadThinkingLevelSelectOptions,
  BuildThreadModelSelectItems,
  EmptyThreadModelValue,
  GetThreadModelTokenLimit,
  ResolveThreadThinkingLevelValue,
  ResolveThreadModelValue,
} from './utils';
import { useActiveHarnessEvents } from './useActiveHarnessEvents';
import type {
  ChatSessionContext,
  ChatPromptSubmission,
  ChatResourceItem,
  LiveAgentRun,
  LiveAgentRunMap,
  PromptImage,
  ThreadAgentRunSnapshot,
  ThreadAgentStep,
  ThreadAssistantMessage,
  ThreadContentBlock,
  ThreadModelOptions,
  ThreadModelPreference,
  ThreadModelSelection,
  PendingToolApproval,
} from './types';

interface ThreadPanelProps {
  activeSessionId: string;
  compactRatio: number;
  modelOptions: ThreadModelOptions | null;
  modelOptionsError: string;
  modelOptionsLoading: boolean;
  modelSelection: ThreadModelSelection;
  isRunning: boolean;
  onAbort: () => Promise<void>;
  onEditAgentRun: (agentRunIndex: number, text: string) => Promise<void>;
  onForkAgentRun: (agentRunIndex: number) => Promise<void>;
  onPromptEditedAgentRun: (sessionId: string, text: string) => Promise<void>;
  onChangeThinkingLevel: (thinkingLevel: string) => void;
  onChangeModel: (modelKey: string) => void;
  onRunningChange: (sessionId: string, isRunning: boolean) => void;
  onAppendUserMessage: (prompt: string, images: PromptImage[]) => void;
  onSubmitPrompt: (submission: ChatPromptSubmission, images: PromptImage[], userMessageDisplayed: boolean) => Promise<void>;
  onWithdrawAgentRun: (agentRunIndex: number) => Promise<void>;
  sessionContext: ChatSessionContext | null;
  toolNames: string[];
  toolPreference: ThreadModelPreference | null;
  title: string;
  onChangeToolEnabled: (enabled: boolean) => Promise<void>;
  onChangeToolSelected: (tool: string, selected: boolean) => Promise<void>;
  onSaveTools: (tools: string[]) => Promise<void>;
  onChangeApproval: (approval: number) => Promise<void>;
}

/// 模型数据加载中下拉占位文案。
const ThreadModelLoadingLabel = () => I18n.thread.modelLoadingLabel;

/// 无模型数据下拉占位文案。
const ThreadModelEmptyLabel = () => I18n.thread.modelEmptyLabel;

/// 模型数据加载失败提示。
const ThreadModelErrorLabel = () => I18n.thread.modelErrorLabel;

/// 联网开关绑定的固定工具名称。
const SearchToolName = 'search';

/// 默认审批对应的偏好值。
const DefaultApprovalValue = '0';

/// 绕过审批对应的偏好值。
const BypassApprovalValue = '1';

type ApprovalValue = typeof DefaultApprovalValue | typeof BypassApprovalValue;

/// 压缩会话历史后端命令名。
const CompactChatCommand = 'compact_chat';

/// 聊天资源列表后端命令名。
const ListResourcesNamesCommand = 'list_chat_resources_names';

/// 单张待发送图片及其内存缩略图。
interface ComposerImageAttachment {
  file: File;
  previewUrl: string;
}

type ChatResourceGroups = [ChatResourceItem[], ChatResourceItem[]];

/// 生成用户消息中可见的斜杠资源调用文本。
/// @param submission 聊天输入区提交内容。
function BuildChatSubmissionDisplayText(submission: ChatPromptSubmission) {
  const text = submission.text.trim();

  if (submission.resource === null) {
    return text;
  }

  return `/${submission.resource.name}${text ? ` ${text}` : ''}`;
}

/// 输入区最多可附加的图片数量。
const MaxAttachedImageCount = 3;

/// 判断文件是否为可发送的图片。
/// @param file 待判断文件。
function IsImageFile(file: File) {
  return file.type.startsWith('image/');
}

/// 从文件列表中获取全部图片。
/// @param files 待检查文件列表。
function FindImageFiles(files: FileList | File[]) {
  return Array.from(files).filter(IsImageFile);
}

/// 从剪贴板条目中获取全部图片。
/// @param items 剪贴板条目列表。
function FindClipboardImageFiles(items: DataTransferItemList) {
  return Array.from(items).flatMap((item) => {
    if (item.kind !== 'file' || !item.type.startsWith('image/')) {
      return [];
    }

    const file = item.getAsFile();

    return file === null ? [] : [file];
  });
}

/// 判断拖放数据是否包含文件。
/// @param transfer 拖放数据。
function HasFiles(transfer: DataTransfer) {
  return Array.from(transfer.types).includes('Files');
}

/// 将本地图片编码为后端所需的 Base64 内容。
/// @param file 待编码图片文件。
function EncodePromptImage(file: File): Promise<PromptImage> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();

    reader.onerror = () => {
      reject(reader.error ?? new Error(I18n.errors.fallback));
    };
    reader.onload = () => {
      if (typeof reader.result !== 'string') {
        reject(new Error(I18n.errors.fallback));
        return;
      }

      const separatorIndex = reader.result.indexOf(',');

      if (separatorIndex < 0) {
        reject(new Error(I18n.errors.fallback));
        return;
      }

      resolve({
        data: reader.result.slice(separatorIndex + 1),
        mimeType: file.type,
      });
    };
    reader.readAsDataURL(file);
  });
}

interface ThreadMessageView {
  agentRunIndex?: number;
  id: string;
  images?: ThreadUserMessageImage[];
  kind: 'user' | 'assistant' | 'steps';
  message?: ThreadAssistantMessage;
  steps?: ThreadAgentStep[];
  text?: string;
}

/// 用户消息中的图片内容。
interface ThreadUserMessageImage {
  data: string;
  mimeType: string;
}

type ComposerBusyMap = Record<string, boolean>;

/// 判断工具能力是否启用。
/// @param preference 当前工具偏好。
function IsToolEnabled(preference: ThreadModelPreference | null) {
  return preference?.tools[0] === '1';
}

/// 判断指定工具是否已选择。
/// @param preference 当前工具偏好。
/// @param tool 工具名称。
function IsToolSelected(preference: ThreadModelPreference | null, tool: string) {
  return preference?.tools.slice(1).includes(tool) ?? false;
}

/// 构造指定工具切换后的完整配置。
/// @param tools 当前工具配置。
/// @param tool 待切换的工具名称。
/// @param selected 是否选择工具。
function BuildToolsWithSelection(tools: string[], tool: string, selected: boolean) {
  const [enabled = '0', ...selectedTools] = tools;
  const nextTools = selected
    ? [...selectedTools, tool]
    : selectedTools.filter((selectedTool) => selectedTool !== tool);

  return [enabled, ...nextTools];
}

/// 获取消息内容字段。
/// @param message 原始消息。
function GetMessageContent(message: unknown) {
  return IsRecord(message) ? message.content : undefined;
}

/// 收集用户消息内容中的图片块。
/// @param content 用户消息内容。
function CollectUserMessageImages(content: unknown): ThreadUserMessageImage[] {
  const images: ThreadUserMessageImage[] = [];

  /// 遍历消息内容的数组与嵌套容器。
  /// @param value 当前待解析内容。
  function VisitContent(value: unknown) {
    if (Array.isArray(value)) {
      value.forEach(VisitContent);
      return;
    }

    if (!IsRecord(value)) {
      return;
    }

    if (value.type === 'image' || value.type === 'ImageContent') {
      const data = typeof value.data === 'string' ? value.data : '';
      const mimeType = typeof value.mimeType === 'string'
        ? value.mimeType
        : typeof value.mime_type === 'string' ? value.mime_type : '';

      if (data && mimeType) {
        images.push({ data, mimeType });
      }
      return;
    }

    if (Array.isArray(value.content)) {
      VisitContent(value.content);
    }

    if (Array.isArray(value.blocks)) {
      VisitContent(value.blocks);
    }
  }

  VisitContent(content);
  return images;
}

/// 获取工具结果关联的工具调用 id。
/// @param message 原始工具结果消息。
function GetToolResultCallId(message: unknown) {
  if (!IsRecord(message)) {
    return '';
  }

  return typeof message.toolCallId === 'string'
    ? message.toolCallId
    : typeof message.tool_call_id === 'string' ? message.tool_call_id : '';
}

/// 判断助手消息是否属于工具循环步骤。
/// @param message 助手展示消息。
/// @param toolResults 紧随其后的工具结果消息。
function IsStepAssistantMessage(message: ThreadAssistantMessage, toolResults: unknown[]) {
  return toolResults.length > 0 || message.blocks.some((block) => block.kind === 'toolCall');
}

/// 将待归档 step 刷入消息视图。
/// @param views 消息视图列表。
/// @param pendingSteps 待归档 step。
function FlushPendingSteps(views: ThreadMessageView[], pendingSteps: ThreadAgentStep[]) {
  if (pendingSteps.length === 0) {
    return;
  }

  views.push({
    id: `persisted-steps-${views.length}`,
    kind: 'steps',
    steps: [...pendingSteps],
  });
  pendingSteps.length = 0;
}

/// 将工具结果补回包含对应工具调用的步骤。
/// @param steps 待匹配的步骤列表。
/// @param toolCallId 工具结果关联的工具调用 id。
/// @param block 工具结果内容块。
function AppendToolResultToMatchingStep(
  steps: ThreadAgentStep[],
  toolCallId: string,
  block: ThreadContentBlock
) {
  if (!toolCallId) {
    return false;
  }

  for (let index = steps.length - 1; index >= 0; index -= 1) {
    const step = steps[index];

    if (!step.blocks.some((stepBlock) => stepBlock.kind === 'toolCall' && stepBlock.id === toolCallId)) {
      continue;
    }

    steps[index] = {
      ...step,
      blocks: [...step.blocks, block],
    };
    return true;
  }

  return false;
}

/// 将延迟到达的工具结果补回已构造视图中的对应步骤。
/// @param views 已构造的消息视图。
/// @param toolCallId 工具结果关联的工具调用 id。
/// @param block 工具结果内容块。
function AppendToolResultToMatchingView(
  views: ThreadMessageView[],
  toolCallId: string,
  block: ThreadContentBlock
) {
  for (let index = views.length - 1; index >= 0; index -= 1) {
    const view = views[index];

    if (view.kind !== 'steps' || !view.steps?.length) {
      continue;
    }

    if (AppendToolResultToMatchingStep(view.steps, toolCallId, block)) {
      return true;
    }
  }

  return false;
}

/// 收集紧随助手消息后的工具结果消息。
/// @param messages 会话消息列表。
/// @param startIndex 起始位置。
function CollectFollowingToolResults(messages: unknown[], startIndex: number) {
  const toolResults: unknown[] = [];
  let nextIndex = startIndex + 1;

  while (nextIndex < messages.length && GetMessageRole(messages[nextIndex]) === 'toolResult') {
    toolResults.push(messages[nextIndex]);
    nextIndex += 1;
  }

  return { toolResults, nextIndex };
}

/// 构造持久化消息展示模型。
/// @param messages 后端返回的会话消息列表。
function BuildThreadMessageViews(messages: unknown[]): ThreadMessageView[] {
  const views: ThreadMessageView[] = [];
  const pendingSteps: ThreadAgentStep[] = [];
  const userMessageCount = CountUserMessages(messages);
  let userMessageOrdinal = 0;

  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    const role = GetMessageRole(message);

    if (role === 'user') {
      FlushPendingSteps(views, pendingSteps);
      const content = GetMessageContent(message);
      views.push({
        agentRunIndex: userMessageCount - userMessageOrdinal - 1,
        id: `persisted-user-${index}`,
        images: CollectUserMessageImages(content),
        kind: 'user',
        text: ExtractMessageText(content),
      });
      userMessageOrdinal += 1;
      continue;
    }

    if (role === 'assistant') {
      const { toolResults, nextIndex } = CollectFollowingToolResults(messages, index);
      const assistant = BuildAssistantMessageView(message, `persisted-assistant-${index}`, true);

      if (IsStepAssistantMessage(assistant, toolResults)) {
        pendingSteps.push({
          id: `persisted-step-${index}`,
          index: pendingSteps.length + 1,
          blocks: [
            ...assistant.blocks,
            ...toolResults.map((toolResult, toolResultIndex) => (
              BuildToolResultBlock(toolResult, `persisted-tool-result-${index}-${toolResultIndex}`, true)
            )),
          ],
        });
        index = nextIndex - 1;
        continue;
      }

      FlushPendingSteps(views, pendingSteps);
      if (assistant.blocks.length > 0) {
        views.push({
          id: `persisted-assistant-${index}`,
          kind: 'assistant',
          message: assistant,
        });
      }
      continue;
    }

    if (role === 'toolResult') {
      const block = BuildToolResultBlock(message, `persisted-tool-result-${index}`, true);
      const toolCallId = GetToolResultCallId(message);

      if (AppendToolResultToMatchingStep(pendingSteps, toolCallId, block)) {
        continue;
      }

      if (AppendToolResultToMatchingView(views, toolCallId, block)) {
        continue;
      }

      pendingSteps.push({
        id: `persisted-step-${index}`,
        index: pendingSteps.length + 1,
        blocks: [block],
      });
    }
  }

  FlushPendingSteps(views, pendingSteps);
  return views.filter((view) => (
    view.kind !== 'user'
    || (view.text?.trim().length ?? 0) > 0
    || (view.images?.length ?? 0) > 0
  ));
}

/// 统计会话中的用户消息数量。
/// @param messages 会话消息列表。
function CountUserMessages(messages: unknown[]): number {
  return messages.reduce<number>((count, message) => (
    GetMessageRole(message) === 'user' ? count + 1 : count
  ), 0);
}

/// 获取内容块图标。
/// @param block 内容块。
function ResolveBlockIcon(block: ThreadContentBlock): IconName {
  if (block.kind === 'thinking') {
    return 'brain';
  }

  if (block.kind === 'toolCall') {
    return 'wrench';
  }

  if (block.kind === 'toolResult') {
    return block.isError ? 'circle-alert' : 'circle-check-big';
  }

  if (block.kind === 'image') {
    return 'image';
  }

  return 'messages-square';
}

/// 渲染正在等待的三个点。
function WaitingDots() {
  return (
    <div className="WaitingDots" aria-label={I18n.thread.waitingResponseAria} role="status">
      <span />
      <span />
      <span />
    </div>
  );
}

/// 渲染会话上下文压缩中的消息状态。
function ConversationCompaction() {
  return (
    <div aria-live="polite" className="ConversationCompaction" role="status">
      <span>Conversation Compaction</span>
      <span aria-hidden="true" className="ConversationCompactionDots">
        <i />
        <i />
        <i />
      </span>
    </div>
  );
}

/// 渲染可折叠过程内容块。
/// @param props.block 内容块。
function CollapsibleContentBlock({ block }: { block: ThreadContentBlock }) {
  const [collapsed, setCollapsed] = useState(block.collapsed);
  const contentId = `${block.id}-content`;

  useEffect(() => {
    setCollapsed(block.collapsed);
  }, [block.collapsed, block.id]);

  /// 切换内容块折叠状态。
  function ToggleCollapsed() {
    setCollapsed((value) => !value);
  }

  return (
    <section className={`AssistantMetaBlock AssistantMetaBlock-${block.kind}`} aria-label={block.title}>
      <button
        aria-controls={contentId}
        aria-expanded={!collapsed}
        className="AssistantMetaToggle"
        onClick={ToggleCollapsed}
        type="button"
      >
        <IconGlyph name={ResolveBlockIcon(block)} size={13} />
        <span>{block.title}</span>
        <IconGlyph className={collapsed ? 'AssistantMetaChevron' : 'AssistantMetaChevron AssistantMetaChevronOpen'} name="chevron-down" size={12} />
      </button>

      {collapsed ? (
        <span className="AssistantCollapsedPreview">{'{...}'}</span>
      ) : (
        <pre className="AssistantMetaText" id={contentId}>
          {block.detail || block.text}
        </pre>
      )}
    </section>
  );
}

/// 渲染单个助手内容块。
/// @param props.block 内容块。
function AssistantContentBlock({ block }: { block: ThreadContentBlock }) {
  if (block.kind === 'text') {
    return <p className="AssistantTextBlock">{block.text}</p>;
  }

  if (block.kind === 'image') {
    if (block.imageData && block.mimeType) {
      return (
        <img
          alt="Assistant generated content"
          className="AssistantImageBlock"
          src={`data:${block.mimeType};base64,${block.imageData}`}
        />
      );
    }

    return <div className="AssistantImagePlaceholder" aria-label={I18n.thread.imageLoadingAria} role="status" />;
  }

  return <CollapsibleContentBlock block={block} />;
}

/// 渲染助手内容块列表。
/// @param props.blocks 内容块列表。
function AssistantContentBlocks({ blocks }: { blocks: ThreadContentBlock[] }) {
  return (
    <div className="AssistantContentBlocks">
      {blocks.map((block) => (
        <AssistantContentBlock block={block} key={block.id} />
      ))}
    </div>
  );
}

/// 渲染助手消息。
/// @param props.message 助手消息展示模型。
function AssistantMessage({ message }: { message: ThreadAssistantMessage }) {
  return (
    <article className="AssistantResponse">
      <AssistantContentBlocks blocks={message.blocks} />
    </article>
  );
}

/// 渲染 steps 折叠块。
/// @param props.steps step 列表。
function AgentStepsDisclosure({ steps }: { steps: ThreadAgentStep[] }) {
  const [open, setOpen] = useState(false);
  const contentId = `agent-steps-${steps[0]?.id ?? steps.length}`;

  useEffect(() => {
    setOpen(false);
  }, [steps.length]);

  /// 切换 steps 折叠状态。
  function ToggleOpen() {
    setOpen((value) => !value);
  }

  if (steps.length === 0) {
    return null;
  }

  return (
    <section className="AgentStepsDisclosure" aria-label="Agent steps">
      <button
        aria-controls={contentId}
        aria-expanded={open}
        className="AgentStepsToggle"
        onClick={ToggleOpen}
        type="button"
      >
        <span>{steps.length} steps</span>
        <IconGlyph className={open ? 'AgentStepsChevron AgentStepsChevronOpen' : 'AgentStepsChevron'} name="chevron-down" size={13} />
      </button>

      {open ? (
        <div className="AgentStepsBody" id={contentId}>
          {steps.map((step) => (
            <section className="AgentStepItem" key={step.id}>
              <span className="AgentStepLabel">Step {step.index}</span>
              <AssistantContentBlocks blocks={step.blocks} />
            </section>
          ))}
        </div>
      ) : null}
    </section>
  );
}

/// 渲染 agent 区间的检查点操作。
/// @param props.agentRunIndex 后端使用的用户消息倒序索引。
/// @param props.disabled 是否禁用操作。
/// @param props.onFork Fork 回调。
/// @param props.onWithdraw 回撤回调。
function AgentCheckpointActions({
  agentRunIndex,
  disabled,
  onFork,
  onWithdraw,
}: {
  agentRunIndex: number;
  disabled: boolean;
  onFork: (agentRunIndex: number) => void;
  onWithdraw: (agentRunIndex: number) => void;
}) {
  return (
    <div className="AgentCheckpointActions" aria-label={I18n.thread.checkpointActionsAria}>
      <span aria-hidden="true" className="AgentCheckpointLine" />
      <button
        className="AgentCheckpointRestore"
        disabled={disabled}
        onClick={() => onWithdraw(agentRunIndex)}
        type="button"
      >
        {I18n.thread.restoreCheckpoint}
      </button>
      <span aria-hidden="true" className="AgentCheckpointSeparator">·</span>
      <button
        aria-label={I18n.thread.forkAria}
        className="AgentCheckpointFork"
        disabled={disabled}
        onClick={() => onFork(agentRunIndex)}
        title={I18n.thread.forkAria}
        type="button"
      >
        <IconGlyph name="git-branch" size={13} />
      </button>
      <span aria-hidden="true" className="AgentCheckpointLine" />
    </div>
  );
}

/// 渲染可双击编辑的 agent 区间首条用户消息。
/// @param props.disabled 是否禁用编辑。
/// @param props.images 用户消息中的图片。
/// @param props.onEdit 编辑后重新发送回调。
/// @param props.text 用户消息文本。
function EditableUserMessage({
  disabled,
  images,
  onEdit,
  text,
}: {
  disabled: boolean;
  images: ThreadUserMessageImage[];
  onEdit: (text: string) => Promise<void>;
  text: string;
}) {
  const [draft, setDraft] = useState(text);
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (!editing) {
      setDraft(text);
    }
  }, [editing, text]);

  useEffect(() => {
    if (!editing) {
      return;
    }

    inputRef.current?.focus();
    inputRef.current?.select();
  }, [editing]);

  useEffect(() => {
    if (disabled) {
      setEditing(false);
    }
  }, [disabled]);

  useEffect(() => {
    if (!editing || saving) {
      return undefined;
    }

    /// 点击编辑区域外时取消编辑。
    /// @param event 指针事件。
    function HandleOutsidePointerDown(event: PointerEvent) {
      if (inputRef.current?.contains(event.target as Node)) {
        return;
      }

      setDraft(text);
      setEditing(false);
    }

    document.addEventListener('pointerdown', HandleOutsidePointerDown);
    return () => {
      document.removeEventListener('pointerdown', HandleOutsidePointerDown);
    };
  }, [editing, saving, text]);

  /// 进入消息编辑状态。
  function StartEditing() {
    if (!disabled && !saving) {
      setEditing(true);
    }
  }

  /// 取消消息编辑并恢复原始文本。
  function CancelEditing() {
    if (saving) {
      return;
    }

    setDraft(text);
    setEditing(false);
  }

  /// 提交消息编辑。
  async function SubmitEditing() {
    const nextText = draft.trim();

    if (!nextText || saving) {
      return;
    }

    if (nextText === text) {
      setEditing(false);
      return;
    }

    setSaving(true);
    try {
      await onEdit(nextText);
      setEditing(false);
    } catch (error) {
      console.error('编辑用户消息失败', error);
    } finally {
      setSaving(false);
    }
  }

  /// 处理编辑输入框快捷键。
  /// @param event 键盘事件。
  function HandleEditKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.nativeEvent.isComposing) {
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      CancelEditing();
      return;
    }

    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void SubmitEditing();
    }
  }

  if (editing) {
    return (
      <textarea
        aria-label={I18n.thread.editUserMessageAria}
        className="UserMessage UserMessageEditing"
        disabled={disabled || saving}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={HandleEditKeyDown}
        ref={inputRef}
        rows={Math.max(1, draft.split('\n').length)}
        value={draft}
      />
    );
  }

  return (
    <div
      className={disabled ? 'UserMessage' : 'UserMessage UserMessageEditable'}
      onDoubleClick={StartEditing}
      title={disabled ? undefined : I18n.thread.doubleClickEditTitle}
    >
      {text ? <p className="UserMessageText">{text}</p> : null}
      {images.length > 0 ? (
        <div className="UserMessageImages">
          {images.map((image, index) => (
            <div className="UserMessageImagePreview" key={`${image.mimeType}-${index}`}>
              <img
                alt={I18n.thread.userMessageImageAlt.replace('{index}', String(index + 1))}
                src={`data:${image.mimeType};base64,${image.data}`}
              />
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/// 渲染单条持久化消息视图。
/// @param props.agentActionsAvailable 是否允许变更任意 Agent 区间。
/// @param props.disabled 是否禁用消息操作。
/// @param props.message 消息视图。
/// @param props.onEditAgentRun 编辑指定 agent 区间回调。
/// @param props.onForkAgentRun Fork 指定 agent 区间回调。
/// @param props.onWithdrawAgentRun 回撤指定 agent 区间回调。
function ThreadMessageItem({
  agentActionsAvailable,
  disabled,
  message,
  onEditAgentRun,
  onForkAgentRun,
  onWithdrawAgentRun,
}: {
  agentActionsAvailable: boolean;
  disabled: boolean;
  message: ThreadMessageView;
  onEditAgentRun: (agentRunIndex: number, text: string) => Promise<void>;
  onForkAgentRun: (agentRunIndex: number) => void;
  onWithdrawAgentRun: (agentRunIndex: number) => void;
}) {
  if (message.kind === 'user') {
    const agentRunIndex = message.agentRunIndex;

    return (
      <>
        {agentRunIndex === undefined || !agentActionsAvailable ? null : (
          <AgentCheckpointActions
            agentRunIndex={agentRunIndex}
            disabled={disabled}
            onFork={onForkAgentRun}
            onWithdraw={onWithdrawAgentRun}
          />
        )}
        <div className="UserMessageRow">
          <EditableUserMessage
            disabled={disabled || agentRunIndex === undefined || !agentActionsAvailable || (message.images?.length ?? 0) > 0}
            images={message.images ?? []}
            onEdit={(text) => onEditAgentRun(agentRunIndex ?? -1, text)}
            text={message.text ?? ''}
          />
        </div>
      </>
    );
  }

  if (message.kind === 'steps' && message.steps) {
    return <AgentStepsDisclosure steps={message.steps} />;
  }

  return message.message ? <AssistantMessage message={message.message} /> : null;
}

/// 渲染已完成 live run 快照。
/// @param props.snapshot 已完成 run 快照。
function AgentRunSnapshot({ snapshot }: { snapshot: ThreadAgentRunSnapshot }) {
  return (
    <>
      {snapshot.steps.length ? <AgentStepsDisclosure steps={snapshot.steps} /> : null}
      {snapshot.assistantMessage && snapshot.assistantMessage.blocks.length > 0 ? (
        <AssistantMessage message={snapshot.assistantMessage} />
      ) : null}
    </>
  );
}

/// 渲染单条工具审批卡片。
/// @param props.approval 待审批工具调用。
/// @param props.sessionId 所属会话 id。
function ToolApprovalCard({ approval, sessionId }: { approval: PendingToolApproval; sessionId: string }) {
  const [submitting, setSubmitting] = useState(false);

  /// 提交工具审批决定。
  /// @param approved 是否允许工具执行。
  async function ResolveApproval(approved: boolean) {
    if (submitting) {
      return;
    }

    setSubmitting(true);
    try {
      await invoke('resolve_chat_tool_approval', {
        input: { sessionId, approvalId: approval.approvalId, approved },
      });
    } catch (error) {
      console.error('结算工具审批失败', error);
      setSubmitting(false);
    }
  }

  return (
    <section className="ToolApprovalCard" aria-label={I18n.thread.toolApprovalAction.replace('{name}', approval.toolName)}>
      <div className="ToolApprovalSummary">
        <strong className="ToolApprovalTitle">{I18n.thread.toolApprovalAction.replace('{name}', approval.toolName)}</strong>
        <code className="ToolApprovalArgs" title={JSON.stringify(approval.args)}>{JSON.stringify(approval.args)}</code>
      </div>
      <div className="ToolApprovalActions">
        <button className="SkillsSecondaryButton" disabled={submitting} onClick={() => void ResolveApproval(false)} type="button">{I18n.thread.toolApprovalReject}</button>
        <button className="SkillsPrimaryButton" disabled={submitting} onClick={() => void ResolveApproval(true)} type="button">
          {submitting ? I18n.common.loading : I18n.thread.toolApprovalResolve}
        </button>
      </div>
    </section>
  );
}

/// 渲染紧贴输入区的待审批工具调用列表。
/// @param props.approvals 待审批工具调用列表。
/// @param props.sessionId 所属会话 id。
function ToolApprovalStrip({ approvals, sessionId }: { approvals: PendingToolApproval[]; sessionId: string }) {
  if (approvals.length === 0) {
    return null;
  }

  return (
    <section aria-label={I18n.thread.toolApprovalPendingAria} aria-live="assertive" className="ToolApprovalStrip">
      {approvals.map((approval) => (
        <ToolApprovalCard approval={approval} key={approval.approvalId} sessionId={sessionId} />
      ))}
    </section>
  );
}

/// 渲染主消息视口。
/// @param props.activeSessionId 当前会话 id。
/// @param props.isCompacting 会话上下文是否正在压缩。
/// @param props.liveRun 当前会话 live run 状态。
/// @param props.onClearLiveRun 清理当前会话 live run 回调。
/// @param props.onInvalidateLiveRun 使当前会话的旧 Harness 事件失效并清理 live run 回调。
/// @param props.onEditAgentRun 编辑指定 agent 区间回调。
/// @param props.onForkAgentRun Fork 指定 agent 区间回调。
/// @param props.onPromptEditedAgentRun 发送编辑后消息回调。
/// @param props.onStartEditedRun 创建编辑后的等待状态回调。
/// @param props.sessionContext 当前会话持久化上下文。
/// @param props.onWithdrawAgentRun 回撤指定 agent 区间回调。
function MessageViewport({
  activeSessionId,
  isCompacting,
  liveRun,
  onClearLiveRun,
  onInvalidateLiveRun,
  onEditAgentRun,
  onForkAgentRun,
  onPromptEditedAgentRun,
  onStartEditedRun,
  sessionContext,
  onWithdrawAgentRun,
}: {
  activeSessionId: string;
  isCompacting: boolean;
  liveRun: LiveAgentRun | null;
  onClearLiveRun: () => void;
  onInvalidateLiveRun: () => void;
  onEditAgentRun: (agentRunIndex: number, text: string) => Promise<void>;
  onForkAgentRun: (agentRunIndex: number) => Promise<void>;
  onPromptEditedAgentRun: (sessionId: string, text: string) => Promise<void>;
  onStartEditedRun: (afterUserMessageCount: number) => void;
  sessionContext: ChatSessionContext | null;
  onWithdrawAgentRun: (agentRunIndex: number) => Promise<void>;
}) {
  const scrollAreaRef = useRef<HTMLDivElement | null>(null);
  const [pendingAgentRunIndex, setPendingAgentRunIndex] = useState<number | null>(null);
  const messages = BuildThreadMessageViews(sessionContext?.messages ?? []);
  const historySnapshots = liveRun?.history ?? [];
  const historyByUserCount = new Map<number, ThreadAgentRunSnapshot[]>();
  const trailingHistory: ThreadAgentRunSnapshot[] = [];
  const liveMessage = liveRun?.activeMessage;
  /// 压缩期间和本地等待态均不能改变当前活跃 leaf。
  const agentActionsAvailable = !isCompacting && (liveRun?.status !== 'running' || liveRun.agentStarted);
  const messageActionsDisabled = isCompacting || pendingAgentRunIndex !== null || activeSessionId === '';
  const activeBlockCount = liveMessage?.blocks.length ?? 0;
  const liveStepCount = liveRun?.steps.length ?? 0;
  const messageGroups = messages.reduce<ThreadMessageView[][]>((groups, message) => {
    if (message.kind === 'user' || groups.length === 0) {
      groups.push([message]);
      return groups;
    }

    groups[groups.length - 1].push(message);
    return groups;
  }, []);
  const lastAgentRunGroupIndex = messageGroups.reduce(
    (lastIndex, group, groupIndex) => (group[0]?.kind === 'user' ? groupIndex : lastIndex),
    -1
  );
  let userMessageCount = 0;

  useLayoutEffect(() => {
    const scrollArea = scrollAreaRef.current;

    if (scrollArea === null) {
      return undefined;
    }

    const frameId = window.requestAnimationFrame(() => {
      scrollArea.scrollTop = scrollArea.scrollHeight;
    });

    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [activeSessionId, messages.length, historySnapshots.length, activeBlockCount, liveStepCount]);

  historySnapshots.forEach((snapshot) => {
    if (typeof snapshot.afterUserMessageCount !== 'number') {
      trailingHistory.push(snapshot);
      return;
    }

    const snapshots = historyByUserCount.get(snapshot.afterUserMessageCount) ?? [];

    snapshots.push(snapshot);
    historyByUserCount.set(snapshot.afterUserMessageCount, snapshots);
  });

  /// 回撤指定 agent 区间，并同步清理本地流式展示。
  /// @param agentRunIndex 后端使用的用户消息倒序索引。
  async function HandleWithdrawAgentRun(agentRunIndex: number) {
    if (isCompacting || pendingAgentRunIndex !== null) {
      return;
    }

    setPendingAgentRunIndex(agentRunIndex);
    try {
      await onWithdrawAgentRun(agentRunIndex);
      onInvalidateLiveRun();
    } finally {
      setPendingAgentRunIndex(null);
    }
  }

  /// Fork 指定 agent 区间，并在成功后切换到新会话。
  /// @param agentRunIndex 后端使用的用户消息倒序索引。
  async function HandleForkAgentRun(agentRunIndex: number) {
    if (isCompacting || pendingAgentRunIndex !== null) {
      return;
    }

    setPendingAgentRunIndex(agentRunIndex);
    try {
      await onForkAgentRun(agentRunIndex);
    } finally {
      setPendingAgentRunIndex(null);
    }
  }

  /// 编辑指定 agent 区间的首条用户消息，并显示本地等待状态。
  /// @param agentRunIndex 后端使用的用户消息倒序索引。
  /// @param text 修改后的用户输入。
  async function HandleEditAgentRun(agentRunIndex: number, text: string) {
    if (isCompacting || pendingAgentRunIndex !== null) {
      return;
    }

    setPendingAgentRunIndex(agentRunIndex);
    try {
      await onEditAgentRun(agentRunIndex, text);
      const afterUserMessageCount = CountUserMessages(sessionContext?.messages ?? []) - agentRunIndex;

      onStartEditedRun(afterUserMessageCount);
      await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
      void onPromptEditedAgentRun(activeSessionId, text).catch((error) => {
        onClearLiveRun();
        console.error('编辑后发送消息失败', error);
      });
    } catch (error) {
      onClearLiveRun();
      throw error;
    } finally {
      setPendingAgentRunIndex(null);
    }
  }

  return (
    <section className="MessageViewport" aria-label="Thread messages">
      <ScrollArea ref={scrollAreaRef} ariaLabel="Thread message list" className="MessageScrollContent">
        {messageGroups.map((group, groupIndex) => {
          const [firstMessage, ...followingMessages] = group;

          if (firstMessage.kind !== 'user') {
            return group.map((message) => (
              <ThreadMessageItem
                agentActionsAvailable={agentActionsAvailable}
                disabled={messageActionsDisabled}
                key={message.id}
                message={message}
                onEditAgentRun={HandleEditAgentRun}
                onForkAgentRun={(agentRunIndex) => void HandleForkAgentRun(agentRunIndex)}
                onWithdrawAgentRun={(agentRunIndex) => void HandleWithdrawAgentRun(agentRunIndex)}
              />
            ));
          }

          userMessageCount += 1;
          const isCurrentAgentRun = groupIndex === lastAgentRunGroupIndex;

          return (
            <article className="AgentRunRegion" key={firstMessage.id}>
              <ThreadMessageItem
                agentActionsAvailable={agentActionsAvailable}
                disabled={messageActionsDisabled}
                message={firstMessage}
                onEditAgentRun={HandleEditAgentRun}
                onForkAgentRun={(agentRunIndex) => void HandleForkAgentRun(agentRunIndex)}
                onWithdrawAgentRun={(agentRunIndex) => void HandleWithdrawAgentRun(agentRunIndex)}
              />
              {(historyByUserCount.get(userMessageCount) ?? []).map((snapshot) => (
                <AgentRunSnapshot key={snapshot.id} snapshot={snapshot} />
              ))}
              {followingMessages.map((message) => (
                <ThreadMessageItem
                  agentActionsAvailable={agentActionsAvailable}
                  disabled={messageActionsDisabled}
                  key={message.id}
                  message={message}
                  onEditAgentRun={HandleEditAgentRun}
                  onForkAgentRun={(agentRunIndex) => void HandleForkAgentRun(agentRunIndex)}
                  onWithdrawAgentRun={(agentRunIndex) => void HandleWithdrawAgentRun(agentRunIndex)}
                />
              ))}
              {isCurrentAgentRun && liveRun?.steps.length ? <AgentStepsDisclosure steps={liveRun.steps} /> : null}
              {isCurrentAgentRun && liveMessage && liveMessage.blocks.length > 0 ? (
                <AssistantMessage message={liveMessage} />
              ) : null}
              {isCurrentAgentRun && isCompacting ? <ConversationCompaction /> : null}
              {isCurrentAgentRun && liveRun?.waiting && !isCompacting ? <WaitingDots /> : null}
            </article>
          );
        })}

        {trailingHistory.map((snapshot) => (
          <AgentRunSnapshot key={snapshot.id} snapshot={snapshot} />
        ))}
        {lastAgentRunGroupIndex < 0 && liveRun?.steps.length ? <AgentStepsDisclosure steps={liveRun.steps} /> : null}
        {lastAgentRunGroupIndex < 0 && liveMessage && liveMessage.blocks.length > 0 ? (
          <AssistantMessage message={liveMessage} />
        ) : null}
        {lastAgentRunGroupIndex < 0 && isCompacting ? <ConversationCompaction /> : null}
        {lastAgentRunGroupIndex < 0 && liveRun?.waiting && !isCompacting ? <WaitingDots /> : null}

        <div className="MessageSpacer" aria-hidden="true" />
      </ScrollArea>
    </section>
  );
}

/// 获取当前审批权限对应的下拉值。
/// @param preference 当前模型偏好。
function ResolveApprovalValue(preference: ThreadModelPreference | null): ApprovalValue {
  return preference?.approval === 1 ? BypassApprovalValue : DefaultApprovalValue;
}

/// 渲染工具审批权限选择器。
/// @param props.preference 当前模型偏好。
/// @param props.onChangeApproval 切换审批权限回调。
function ThreadApprovalSelect({
  preference,
  onChangeApproval,
}: {
  preference: ThreadModelPreference | null;
  onChangeApproval: (approval: number) => Promise<void>;
}) {
  return (
    <SelectField<ApprovalValue>
      ariaLabel={I18n.thread.approvalAriaLabel}
      borderRadius={999}
      className="ThreadApprovalSelect"
      fontSize={12}
      height={28}
      onChange={(value) => void onChangeApproval(Number(value))}
      options={[
        { label: I18n.thread.approvalDefault, value: DefaultApprovalValue },
        { label: I18n.thread.approvalBypass, value: BypassApprovalValue },
      ]}
      title={I18n.thread.approvalTooltip}
      value={ResolveApprovalValue(preference)}
      width={88}
    />
  );
}

/// 渲染顶部模型选择器。
/// @param props.modelOptions 后端返回的模型选项。
/// @param props.modelOptionsError 模型选项加载错误。
/// @param props.modelOptionsLoading 模型选项是否加载中。
/// @param props.modelSelection 当前模型选择状态。
/// @param props.onChangeModel 模型选择变化回调。
function ThreadModelSelect({
  modelOptions,
  modelOptionsError,
  modelOptionsLoading,
  modelSelection,
  onChangeModel,
}: {
  modelOptions: ThreadModelOptions | null;
  modelOptionsError: string;
  modelOptionsLoading: boolean;
  modelSelection: ThreadModelSelection;
  onChangeModel: (modelKey: string) => void;
}) {
  const providerModelIdsMap = modelOptions?.providerModelIdsMap ?? {};
  const modelSelectItems = BuildThreadModelSelectItems(providerModelIdsMap);
  const modelValue = ResolveThreadModelValue(providerModelIdsMap, modelSelection.modelKey);
  const fallbackLabel = modelOptionsLoading ? ThreadModelLoadingLabel() : ThreadModelEmptyLabel();
  const options = modelSelectItems.length > 0
    ? modelSelectItems
    : [{ label: fallbackLabel, value: EmptyThreadModelValue }];
  const value = modelValue || EmptyThreadModelValue;

  return (
    <div className="ThreadHeaderControls">
      <SelectField
        ariaLabel={I18n.thread.modelListAria}
        backgroundColor="var(--sidebar)"
        borderRadius={6}
        className="ThreadModelSelect"
        fontSize={12}
        height={30}
        onChange={onChangeModel}
        optionAlignment="center"
        options={options}
        value={value}
        width="min(440px, 100%)"
      />
      {modelOptionsLoading || modelOptionsError ? (
        <span
          className={modelOptionsError ? 'ThreadHeaderStatus ThreadHeaderStatusError' : 'ThreadHeaderStatus'}
          role="status"
        >
          {modelOptionsError ? ThreadModelErrorLabel() : ThreadModelLoadingLabel()}
        </span>
      ) : null}
    </div>
  );
}

/// 渲染底部思考档位选择器。
/// @param props.modelThinkingLevels 后端返回的模型思考档位列表。
/// @param props.modelSelection 当前模型选择状态。
/// @param props.onChangeThinkingLevel 思考档位选择变化回调。
function ThreadThinkingLevelSelect({
  modelThinkingLevels,
  modelSelection,
  onChangeThinkingLevel,
}: {
  modelThinkingLevels: string[];
  modelSelection: ThreadModelSelection;
  onChangeThinkingLevel: (thinkingLevel: string) => void;
}) {
  const options = BuildThreadThinkingLevelSelectOptions(modelThinkingLevels);
  const value = ResolveThreadThinkingLevelValue(modelThinkingLevels, modelSelection.thinkingLevel);

  return (
    <SelectField
      ariaLabel={I18n.thread.thinkingLevelAria}
      borderRadius={999}
      className="ThreadThinkingLevelSelect"
      fontSize={12}
      height={28}
      onChange={onChangeThinkingLevel}
      options={options}
      title={I18n.thread.thinkingTooltip}
      value={value}
      width={54}
    />
  );
}

/// 渲染工具启用开关和工具多选下拉。
/// @param props.toolNames 后端返回的工具名称。
/// @param props.toolPreference 当前工具偏好。
/// @param props.onChangeToolEnabled 切换工具启用状态回调。
/// @param props.onSaveTools 保存完整工具配置回调。
function ToolPicker({
  toolNames,
  toolPreference,
  onChangeToolEnabled,
  onSaveTools,
}: {
  toolNames: string[];
  toolPreference: ThreadModelPreference | null;
  onChangeToolEnabled: (enabled: boolean) => Promise<void>;
  onSaveTools: (tools: string[]) => Promise<void>;
}) {
  const [draftTools, setDraftTools] = useState<string[]>(toolPreference?.tools ?? ['0']);
  const [expanded, setExpanded] = useState(false);
  const [updatingEnabled, setUpdatingEnabled] = useState(false);
  const [savingTools, setSavingTools] = useState(false);
  const toolEnabled = IsToolEnabled(toolPreference);
  const visibleToolNames = toolNames.filter((tool) => tool !== SearchToolName);

  useEffect(() => {
    if (!expanded) {
      setDraftTools(toolPreference?.tools ?? ['0']);
    }
  }, [expanded, toolPreference]);

  /// 切换工具能力启用状态。
  async function ToggleToolEnabled() {
    if (updatingEnabled) {
      return;
    }

    setUpdatingEnabled(true);
    try {
      await onChangeToolEnabled(!toolEnabled);
    } finally {
      setUpdatingEnabled(false);
    }
  }

  /// 切换面板中单个工具选择状态。
  /// @param tool 工具名称。
  function ToggleToolSelected(tool: string) {
    setDraftTools((currentTools) => BuildToolsWithSelection(
      currentTools,
      tool,
      !currentTools.slice(1).includes(tool),
    ));
  }

  /// 展开或关闭工具选择面板，并在关闭时提交完整配置。
  async function ToggleToolOptions() {
    if (savingTools) {
      return;
    }

    if (!expanded) {
      setDraftTools(toolPreference?.tools ?? ['0']);
      setExpanded(true);
      return;
    }

    setExpanded(false);
    setSavingTools(true);
    try {
      await onSaveTools(draftTools);
    } finally {
      setSavingTools(false);
    }
  }

  return (
    <div className="ToolPicker">
      <button
        aria-checked={toolEnabled}
        aria-label={toolEnabled ? I18n.thread.toolDisableAria : I18n.thread.toolEnableAria}
        className="ToolToggle"
        disabled={updatingEnabled}
        onClick={() => void ToggleToolEnabled()}
        role="switch"
        title={I18n.thread.toolTooltip}
        type="button"
      >
        <IconGlyph name="wrench" size={15} />
      </button>
      <button
        aria-controls="tool-options"
        aria-expanded={expanded}
        aria-label={I18n.thread.toolSelectAria}
        className="ToolAddButton"
        disabled={savingTools}
        onClick={() => void ToggleToolOptions()}
        type="button"
      >
        <IconGlyph name="plus" size={15} />
      </button>
      {expanded ? (
        <div className="ToolOptions" id="tool-options">
          {visibleToolNames.map((tool) => {
            const selected = draftTools.slice(1).includes(tool);
            const inputId = `tool-option-${tool}`;

            return (
              <label className="ToolOption" htmlFor={inputId} key={tool}>
                <input
                  checked={selected}
                  disabled={savingTools}
                  id={inputId}
                  onChange={() => ToggleToolSelected(tool)}
                  type="checkbox"
                />
                <span>{tool}</span>
              </label>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

/// 更新 Composer 按会话隔离的忙碌状态。
/// @param current 当前忙碌状态映射。
/// @param sessionId 目标会话 id。
/// @param busy 目标会话是否忙碌。
function UpdateComposerBusyMap(current: ComposerBusyMap, sessionId: string, busy: boolean) {
  if ((current[sessionId] ?? false) === busy) {
    return current;
  }

  const next = { ...current };

  if (busy) {
    next[sessionId] = true;
  } else {
    delete next[sessionId];
  }

  return next;
}

/// 获取聊天消息中最后一条 assistant 消息的 token 使用量。
/// @param messages 当前聊天区消息数组。
function GetLastAssistantTotalTokens(messages: unknown[]) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];

    if (GetMessageRole(message) !== 'assistant' || !IsRecord(message) || !IsRecord(message.usage)) {
      continue;
    }

    const totalTokens = message.usage.totalTokens ?? message.usage.total_tokens;

    if (typeof totalTokens === 'number' && Number.isFinite(totalTokens) && totalTokens >= 0) {
      return totalTokens;
    }
  }

  return 0;
}

/// 按 k 为单位格式化 token 数。
/// @param tokens token 数。
function FormatTokenCount(tokens: number) {
  return `${Math.round((tokens / 1024) * 10) / 10}k`;
}

/// 渲染当前会话 token 使用量图标和说明。
/// @param props.totalTokens 当前聊天区最后一条 assistant 消息的 token 使用量。
/// @param props.tokenLimit 当前指定模型的最大 token 使用量。
function TokenUsageIndicator({ totalTokens, tokenLimit }: { totalTokens: number; tokenLimit: number | null }) {
  const progress = tokenLimit === null ? 0 : Math.min(totalTokens / tokenLimit, 1) * 100;
  const totalLabel = FormatTokenCount(totalTokens);
  const tokenLimitLabel = tokenLimit === null ? '--' : FormatTokenCount(tokenLimit);
  const description = I18n.thread.tokenUsageTooltip
    .replace('{total}', totalLabel)
    .replace('{limit}', tokenLimitLabel);

  return (
    <span aria-label={description} className="TokenUsageIndicator" role="img" tabIndex={0}>
      <svg aria-hidden="true" viewBox="0 0 24 24">
        <circle className="TokenUsageTrack" cx="12" cy="12" pathLength="100" r="8" />
        <circle
          className="TokenUsageValue"
          cx="12"
          cy="12"
          pathLength="100"
          r="8"
          strokeDasharray={`${progress} 100`}
        />
      </svg>
      <span className="TokenUsageTooltip" role="tooltip">{description}</span>
    </span>
  );
}

/// 渲染底部输入区。
/// @param props.activeSessionId 当前会话 id。
/// @param props.isCompacting 会话上下文是否正在压缩。
/// @param props.isRunning 当前会话是否运行中。
/// @param props.modelThinkingLevels 后端返回的模型思考档位列表。
/// @param props.modelSelection 当前模型选择状态。
/// @param props.tokenLimit 当前指定模型的最大 token 使用量。
/// @param props.totalTokens 当前聊天区最后一条 assistant 消息的 token 使用量。
/// @param props.onAbort 终止当前 Agent run 回调。
/// @param props.onChangeThinkingLevel 思考档位选择变化回调。
/// @param props.onSubmitPrompt 提交 prompt 回调。
/// @param props.toolNames 后端返回的工具名称。
/// @param props.toolPreference 当前工具偏好。
/// @param props.onChangeToolEnabled 切换工具启用状态回调。
/// @param props.onChangeToolSelected 切换指定工具选择状态回调。
/// @param props.onSaveTools 保存完整工具配置回调。
/// @param props.onChangeApproval 切换工具审批权限回调。
function Composer({
  activeSessionId,
  isCompacting,
  isRunning,
  modelThinkingLevels,
  modelSelection,
  tokenLimit,
  totalTokens,
  onAbort,
  onChangeThinkingLevel,
  onSubmitPrompt,
  toolNames,
  toolPreference,
  onChangeToolEnabled,
  onChangeToolSelected,
  onSaveTools,
  onChangeApproval,
}: {
  activeSessionId: string;
  isCompacting: boolean;
  isRunning: boolean;
  modelThinkingLevels: string[];
  modelSelection: ThreadModelSelection;
  tokenLimit: number | null;
  totalTokens: number;
  onAbort: () => Promise<void>;
  onChangeThinkingLevel: (thinkingLevel: string) => void;
  onSubmitPrompt: (submission: ChatPromptSubmission, images: PromptImage[]) => Promise<void>;
  toolNames: string[];
  toolPreference: ThreadModelPreference | null;
  onChangeToolEnabled: (enabled: boolean) => Promise<void>;
  onChangeToolSelected: (tool: string, selected: boolean) => Promise<void>;
  onSaveTools: (tools: string[]) => Promise<void>;
  onChangeApproval: (approval: number) => Promise<void>;
}) {
  const [prompt, setPrompt] = useState('');
  const [sentMessage, setSentMessage] = useState(I18n.thread.waitingInputStatus);
  const [abortingBySessionId, setAbortingBySessionId] = useState<ComposerBusyMap>({});
  const [networkUpdating, setNetworkUpdating] = useState(false);
  const [submittingBySessionId, setSubmittingBySessionId] = useState<ComposerBusyMap>({});
  const [attachedImages, setAttachedImages] = useState<ComposerImageAttachment[]>([]);
  const [imageDragging, setImageDragging] = useState(false);
  const [resourceGroups, setResourceGroups] = useState<ChatResourceGroups>([[], []]);
  const [resourceMenuOpen, setResourceMenuOpen] = useState(false);
  const [resourceMenuReady, setResourceMenuReady] = useState(false);
  const [resourceMenuIndex, setResourceMenuIndex] = useState(0);
  const [selectedResource, setSelectedResource] = useState<ChatResourceItem | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const imageDragDepthRef = useRef(0);
  const attachedImagesRef = useRef<ComposerImageAttachment[]>([]);
  const promptTextareaRef = useRef<HTMLTextAreaElement>(null);
  const resourceItemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const slashInputActiveRef = useRef(false);
  const activeAborting = abortingBySessionId[activeSessionId] ?? false;
  const activeSubmitting = submittingBySessionId[activeSessionId] ?? false;
  const searchEnabled = IsToolSelected(toolPreference, SearchToolName);
  const resourceQuery = selectedResource === null && prompt.startsWith('/')
    ? prompt.slice(1).trim().toLocaleLowerCase()
    : '';
  const visibleResourceGroups = resourceGroups.map((group) => group.filter((resource) => (
    resource.name.toLocaleLowerCase().includes(resourceQuery)
      || resource.description.toLocaleLowerCase().includes(resourceQuery)
  ))) as ChatResourceGroups;
  const visibleResources = visibleResourceGroups.flat();
  const chatResourceGroupLabels = [I18n.sidebar.templates, I18n.sidebar.skills] as const;

  useEffect(() => {
    if (activeSessionId === '') {
      return;
    }

    setAbortingBySessionId((current) => UpdateComposerBusyMap(current, '', false));
    setSubmittingBySessionId((current) => UpdateComposerBusyMap(current, '', false));
  }, [activeSessionId]);

  useEffect(() => () => {
    attachedImagesRef.current.forEach((attachment) => URL.revokeObjectURL(attachment.previewUrl));
  }, []);

  useEffect(() => {
    resourceItemRefs.current[resourceMenuIndex]?.scrollIntoView({ block: 'nearest' });
  }, [resourceMenuIndex, visibleResources.length]);

  /// 读取斜杠菜单中可调用的模板和 Skill。
  async function LoadResourceNames() {
    setResourceMenuReady(false);

    try {
      const groups = await invoke<[Omit<ChatResourceItem, 'kind'>[], Omit<ChatResourceItem, 'kind'>[]]>(
        ListResourcesNamesCommand,
        { input: { sessionId: activeSessionId } },
      );

      setResourceGroups([
        (groups[0] ?? []).map((resource) => ({ ...resource, kind: 'template' })),
        (groups[1] ?? []).map((resource) => ({ ...resource, kind: 'skill' })),
      ]);
    } catch (error) {
      console.error('读取聊天资源列表失败', error);
      ReportBackendError(I18n.thread.resourcesLoadError, error);
      setResourceGroups([[], []]);
    } finally {
      setResourceMenuReady(true);
    }
  }

  /// 打开斜杠菜单并读取当前资源。
  function OpenResourceMenu() {
    setResourceMenuOpen(true);
    setResourceMenuIndex(0);
    void LoadResourceNames();
  }

  /// 选择一个斜杠菜单资源。
  /// @param resource 用户选中的模板或 Skill。
  function SelectResource(resource: ChatResourceItem) {
    slashInputActiveRef.current = false;
    setSelectedResource(resource);
    setPrompt('');
    setResourceMenuOpen(false);
    window.requestAnimationFrame(() => promptTextareaRef.current?.focus());
  }

  /// 删除已选择的斜杠菜单资源，并恢复斜杠输入。
  function RemoveSelectedResource() {
    setSelectedResource(null);
    setPrompt('/');
    OpenResourceMenu();
    window.requestAnimationFrame(() => promptTextareaRef.current?.focus());
  }

  /// 鼠标进入资源菜单时隐藏键盘默认选中背景。
  function HideKeyboardResourceSelection() {
    setResourceMenuIndex(-1);
  }

  /// 清除全部待发送图片。
  function ClearAttachedImages() {
    attachedImagesRef.current.forEach((attachment) => URL.revokeObjectURL(attachment.previewUrl));
    attachedImagesRef.current = [];
    setAttachedImages([]);
  }

  /// 移除指定待发送图片。
  /// @param previewUrl 待移除图片的缩略图 URL。
  function RemoveAttachedImage(previewUrl: string) {
    const attachment = attachedImagesRef.current.find((item) => item.previewUrl === previewUrl);

    if (attachment === undefined) {
      return;
    }

    URL.revokeObjectURL(attachment.previewUrl);
    const nextAttachments = attachedImagesRef.current.filter((item) => item.previewUrl !== previewUrl);

    attachedImagesRef.current = nextAttachments;
    setAttachedImages(nextAttachments);
  }

  /// 将图片追加到输入区并生成内存缩略图。
  /// @param files 待发送图片文件列表。
  function AddAttachedImages(files: FileList | File[]) {
    const imageFiles = FindImageFiles(files);

    if (imageFiles.length !== files.length) {
      const error = new Error(I18n.thread.unsupportedImage);

      setSentMessage(I18n.thread.unsupportedImage);
      ReportBackendError(I18n.thread.addImageError, error);
    }

    if (imageFiles.length === 0) {
      return;
    }

    const remainingCount = MaxAttachedImageCount - attachedImagesRef.current.length;

    if (remainingCount <= 0) {
      setSentMessage(I18n.thread.imageMaxStatus.replace('{count}', String(MaxAttachedImageCount)));
      return;
    }

    const nextAttachments = imageFiles.slice(0, remainingCount).map((file) => ({
      file,
      previewUrl: URL.createObjectURL(file),
    }));
    const allAttachments = [...attachedImagesRef.current, ...nextAttachments];

    attachedImagesRef.current = allAttachments;
    setAttachedImages(allAttachments);
    setSentMessage(imageFiles.length > remainingCount
      ? I18n.thread.imageMaxStatus.replace('{count}', String(MaxAttachedImageCount))
      : I18n.thread.imagesAddedStatus.replace('{count}', String(nextAttachments.length)));
  }

  /// 从图片选择器读取目标图片。
  /// @param event 文件选择事件。
  function HandleImageInputChange(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.target.files ?? []);

    event.target.value = '';
    if (files.length === 0) {
      return;
    }

    AddAttachedImages(files);
  }

  /// 打开本地图片选择框。
  function OpenImagePicker() {
    fileInputRef.current?.click();
  }

  /// 处理图片拖入输入区。
  /// @param event 拖放进入事件。
  function HandleImageDragEnter(event: DragEvent<HTMLFormElement>) {
    if (!HasFiles(event.dataTransfer)) {
      return;
    }

    event.preventDefault();
    imageDragDepthRef.current += 1;
    setImageDragging(true);
  }

  /// 保持输入区作为可放置目标。
  /// @param event 拖放悬停事件。
  function HandleImageDragOver(event: DragEvent<HTMLFormElement>) {
    if (!HasFiles(event.dataTransfer)) {
      return;
    }

    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
  }

  /// 处理图片拖离输入区。
  /// @param event 拖放离开事件。
  function HandleImageDragLeave(event: DragEvent<HTMLFormElement>) {
    if (!HasFiles(event.dataTransfer)) {
      return;
    }

    imageDragDepthRef.current -= 1;
    if (imageDragDepthRef.current <= 0) {
      imageDragDepthRef.current = 0;
      setImageDragging(false);
    }
  }

  /// 处理拖放图片。
  /// @param event 图片拖放事件。
  function HandleImageDrop(event: DragEvent<HTMLFormElement>) {
    if (!HasFiles(event.dataTransfer)) {
      return;
    }

    event.preventDefault();
    imageDragDepthRef.current = 0;
    setImageDragging(false);

    AddAttachedImages(event.dataTransfer.files);
  }

  /// 处理从剪贴板粘贴的图片。
  /// @param event 粘贴事件。
  function HandleImagePaste(event: ClipboardEvent<HTMLFormElement>) {
    const clipboardFiles = Array.from(event.clipboardData.files);
    const files = clipboardFiles.length > 0 ? clipboardFiles : FindClipboardImageFiles(event.clipboardData.items);

    if (files.length === 0) {
      return;
    }

    event.preventDefault();
    AddAttachedImages(files);
  }

  /// 更新聊天输入，并在首字符为斜杠时打开资源菜单。
  /// @param event 输入框变化事件。
  function HandlePromptChange(event: ChangeEvent<HTMLTextAreaElement>) {
    const nextPrompt = event.target.value;

    setPrompt(nextPrompt);
    if (selectedResource !== null) {
      return;
    }

    if (nextPrompt.startsWith('/')) {
      if (!slashInputActiveRef.current) {
        OpenResourceMenu();
      } else {
        setResourceMenuOpen(true);
        setResourceMenuIndex(0);
      }
      slashInputActiveRef.current = true;
      return;
    }

    slashInputActiveRef.current = false;
    setResourceMenuOpen(false);
  }

  /// 提交当前输入内容。
  async function SubmitCurrentPrompt() {
    if (isRunning || isCompacting) {
      return;
    }

    const submission: ChatPromptSubmission = {
      text: prompt.trim(),
      resource: selectedResource,
    };
    if (!submission.text && attachedImages.length === 0 && submission.resource === null) {
      setSentMessage(I18n.thread.emptyPromptStatus);
      return;
    }

    if (submission.resource !== null && attachedImages.length > 0) {
      setSentMessage(I18n.thread.resourceImagesUnsupportedStatus);
      return;
    }

    if (activeSubmitting) {
      return;
    }

    const submitSessionId = activeSessionId;
    const images = attachedImages;

    setPrompt('');
    setSelectedResource(null);
    setResourceMenuOpen(false);
    ClearAttachedImages();
    setSubmittingBySessionId((current) => UpdateComposerBusyMap(current, submitSessionId, true));

    try {
      const encodedImages = await Promise.all(images.map((image) => EncodePromptImage(image.file)));

      await onSubmitPrompt(submission, encodedImages);
      setSentMessage(I18n.thread.sentStatus);
    } catch (error) {
      console.error('提交 prompt 失败', error);
      setSentMessage(I18n.thread.sendFailedStatus);
    } finally {
      setSubmittingBySessionId((current) => UpdateComposerBusyMap(current, submitSessionId, false));
    }
  }

  /// 处理表单提交。
  /// @param event 表单提交事件。
  function HandleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void SubmitCurrentPrompt();
  }

  /// 处理输入框键盘提交。
  /// @param event 键盘事件。
  function HandlePromptKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (resourceMenuOpen) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        if (visibleResources.length > 0) {
          const direction = event.key === 'ArrowDown' ? 1 : -1;
          setResourceMenuIndex((currentIndex) => {
            if (currentIndex < 0) {
              return direction > 0 ? 0 : visibleResources.length - 1;
            }

            return (currentIndex + direction + visibleResources.length) % visibleResources.length;
          });
        }
        return;
      }

      if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
        const resource = visibleResources[resourceMenuIndex];

        if (resource !== undefined) {
          event.preventDefault();
          SelectResource(resource);
        }
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        setResourceMenuOpen(false);
        return;
      }
    }

    const shouldRemoveResource = selectedResource !== null
      && ((event.key === 'Backspace' && prompt.length === 0)
        || (event.key === 'Delete'
          && event.currentTarget.selectionStart === 0
          && event.currentTarget.selectionEnd === 0));

    if (shouldRemoveResource) {
      event.preventDefault();
      RemoveSelectedResource();
      return;
    }

    if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) {
      return;
    }

    event.preventDefault();
    void SubmitCurrentPrompt();
  }

  /// 终止当前 Agent run。
  async function HandleAbort() {
    if (activeAborting) {
      return;
    }

    const abortSessionId = activeSessionId;

    setAbortingBySessionId((current) => UpdateComposerBusyMap(current, abortSessionId, true));
    setSentMessage(I18n.thread.stoppingStatus);

    try {
      await onAbort();
      setSentMessage(I18n.thread.abortRequestedStatus);
    } catch (error) {
      console.error('终止当前运行失败', error);
      setSentMessage(I18n.thread.abortFailedStatus);
    } finally {
      setAbortingBySessionId((current) => UpdateComposerBusyMap(current, abortSessionId, false));
    }
  }

  /// 切换联网工具选择状态。
  async function ToggleSearchTool() {
    if (networkUpdating) {
      return;
    }

    setNetworkUpdating(true);
    try {
      await onChangeToolSelected(SearchToolName, !searchEnabled);
    } finally {
      setNetworkUpdating(false);
    }
  }

  const sendButtonDisabled = isCompacting || (isRunning ? activeAborting : activeSubmitting);

  return (
    <form
      aria-label="Message composer"
      className={imageDragging ? 'Composer ComposerImageDragging' : 'Composer'}
      onDragEnter={HandleImageDragEnter}
      onDragLeave={HandleImageDragLeave}
      onDragOver={HandleImageDragOver}
      onDrop={HandleImageDrop}
      onPaste={HandleImagePaste}
      onSubmit={HandleSubmit}
    >
      <label className="SrOnly" htmlFor="agent-prompt">{I18n.thread.taskLabel}</label>
      <input
        accept="image/*"
        className="ComposerImageInput"
        multiple
        onChange={HandleImageInputChange}
        ref={fileInputRef}
        tabIndex={-1}
        type="file"
      />
      {attachedImages.length > 0 ? (
        <div className="ComposerImagePreviews">
          {attachedImages.map((attachment, index) => (
            <div className="ComposerImagePreview" key={attachment.previewUrl}>
              <img alt={I18n.thread.previewImageAlt.replace('{index}', String(index + 1))} src={attachment.previewUrl} />
              <button
                aria-label={I18n.thread.removeImageAria.replace('{index}', String(index + 1))}
                className="ComposerImageRemove"
                onClick={() => RemoveAttachedImage(attachment.previewUrl)}
                title={I18n.thread.removeImageTitle}
                type="button"
              >
                <IconGlyph name="x" size={10} />
              </button>
            </div>
          ))}
        </div>
      ) : null}
      {resourceMenuOpen ? (
        <div
          aria-label={I18n.thread.callableResourcesAria}
          className="SlashCommandMenu"
          onMouseEnter={HideKeyboardResourceSelection}
          role="listbox"
        >
          {resourceMenuReady && visibleResources.length === 0 ? (
            <span className="SlashCommandMenuStatus">{I18n.thread.noMatchingResources}</span>
          ) : null}
          {resourceMenuReady && visibleResourceGroups.map((group, groupIndex) => (
            group.length > 0 ? (
              <div className="SlashCommandGroup" key={chatResourceGroupLabels[groupIndex]}>
                <span className="SlashCommandGroupTitle">{chatResourceGroupLabels[groupIndex]}</span>
                {group.map((resource) => {
                  const resourceIndex = visibleResources.indexOf(resource);
                  const selected = resourceIndex === resourceMenuIndex;

                  return (
                    <button
                      aria-selected={selected}
                      className={selected ? 'SlashCommandItem SlashCommandItemSelected' : 'SlashCommandItem'}
                      key={`${resource.kind}-${resource.name}`}
                      onClick={() => SelectResource(resource)}
                      ref={(element) => {
                        resourceItemRefs.current[resourceIndex] = element;
                      }}
                      role="option"
                      type="button"
                    >
                      <IconGlyph name="boxes" size={18} />
                      <span className="SlashCommandItemName">{resource.name}</span>
                      <span className="SlashCommandItemDescription">{resource.description}</span>
                    </button>
                  );
                })}
              </div>
            ) : null
          ))}
        </div>
      ) : null}
      {selectedResource !== null ? (
        <span className="SlashCommandToken" title={selectedResource.description}>
          <IconGlyph name="boxes" size={12} />
          <span>/{selectedResource.name}</span>
        </span>
      ) : null}
      <textarea
        className="PromptTextarea"
        id="agent-prompt"
        onChange={HandlePromptChange}
        onKeyDown={HandlePromptKeyDown}
        placeholder={selectedResource === null ? I18n.thread.promptPlaceholder : I18n.thread.followupPlaceholder}
        ref={promptTextareaRef}
        rows={2}
        value={prompt}
      />

      <div className="ControlBar">
        <div className="LeftControls">
          <ToolPicker
            toolNames={toolNames}
            toolPreference={toolPreference}
            onChangeToolEnabled={onChangeToolEnabled}
            onSaveTools={onSaveTools}
          />
          <ThreadApprovalSelect preference={toolPreference} onChangeApproval={onChangeApproval} />
        </div>

        <div className="RightControls">
          <TokenUsageIndicator tokenLimit={tokenLimit} totalTokens={totalTokens} />
          <ThreadThinkingLevelSelect
            modelThinkingLevels={modelThinkingLevels}
            modelSelection={modelSelection}
            onChangeThinkingLevel={onChangeThinkingLevel}
          />
          <button
            aria-checked={searchEnabled}
            aria-label={searchEnabled ? I18n.thread.networkDisableAria : I18n.thread.networkEnableAria}
            className="NetworkToggle"
            disabled={networkUpdating}
            onClick={() => void ToggleSearchTool()}
            role="switch"
            title={I18n.thread.networkTooltip}
            type="button"
          >
            <IconGlyph name="globe" size={15} />
          </button>
          <button
            aria-label={I18n.thread.addImageTooltip}
            className="AttachButton"
            onClick={OpenImagePicker}
            title={I18n.thread.addImageTooltip}
            type="button"
          >
            <IconGlyph name="image" size={15} />
          </button>
          <button
            aria-label={isCompacting
              ? I18n.thread.compactingAria
              : isRunning ? I18n.thread.abortRunAria : I18n.thread.sendMessageAria}
            className={isRunning && !isCompacting ? 'SendButton SendButtonAbort' : 'SendButton'}
            disabled={sendButtonDisabled}
            onClick={isRunning && !isCompacting ? HandleAbort : undefined}
            type={isRunning || isCompacting ? 'button' : 'submit'}
          >
            <IconGlyph name={isRunning && !isCompacting ? 'square' : 'arrow-up'} size={15} />
          </button>
        </div>
      </div>

      <span className="SrOnly" role="status">{sentMessage}</span>
    </form>
  );
}

/// 渲染右侧对话框面板内容。
/// @param props.compactRatio 自动压缩的 token 使用百分比。
/// @param props.modelOptions 后端返回的模型选项。
/// @param props.modelOptionsError 模型选项加载错误。
/// @param props.modelOptionsLoading 模型选项是否加载中。
/// @param props.modelSelection 当前模型选择状态。
/// @param props.isRunning 当前会话是否运行中。
/// @param props.onAbort 终止当前 Agent run 回调。
/// @param props.onEditAgentRun 编辑并重新发送指定 agent 区间回调。
/// @param props.onForkAgentRun Fork 指定 agent 区间回调。
/// @param props.onPromptEditedAgentRun 发送编辑后消息回调。
/// @param props.onChangeThinkingLevel 思考档位选择变化回调。
/// @param props.onChangeModel 模型选择变化回调。
/// @param props.onRunningChange 指定会话运行状态变化回调。
/// @param props.onAppendUserMessage 追加本地用户消息回调。
/// @param props.onSubmitPrompt 提交 prompt 回调。
/// @param props.onWithdrawAgentRun 回撤指定 agent 区间回调。
/// @param props.sessionContext 当前会话持久化上下文。
/// @param props.toolNames 后端返回的工具名称。
/// @param props.toolPreference 当前工具偏好。
/// @param props.title 当前线程标题。
/// @param props.onChangeToolEnabled 切换工具启用状态回调。
/// @param props.onChangeToolSelected 切换指定工具选择状态回调。
/// @param props.onSaveTools 保存完整工具配置回调。
/// @param props.onChangeApproval 切换工具审批权限回调。
function ThreadPanel({
  activeSessionId,
  compactRatio,
  isRunning,
  modelOptions,
  modelOptionsError,
  modelOptionsLoading,
  modelSelection,
  onAbort,
  onEditAgentRun,
  onForkAgentRun,
  onPromptEditedAgentRun,
  onChangeThinkingLevel,
  onChangeModel,
  onRunningChange,
  onAppendUserMessage,
  onSubmitPrompt,
  onWithdrawAgentRun,
  sessionContext,
  toolNames,
  toolPreference,
  title,
  onChangeToolEnabled,
  onChangeToolSelected,
  onSaveTools,
  onChangeApproval,
}: ThreadPanelProps) {
  const modelThinkingLevels = modelOptions?.modelThinkingLevels ?? [];
  const tokenLimit = GetThreadModelTokenLimit(modelOptions?.providerModelTokensMap ?? {}, modelSelection.modelKey);
  const persistedTotalTokens = GetLastAssistantTotalTokens(sessionContext?.messages ?? []);
  const [liveRuns, setLiveRuns] = useState<LiveAgentRunMap>({});
  const [compactingSessionIds, setCompactingSessionIds] = useState<Set<string>>(new Set());
  const activeSessionIdRef = useRef(activeSessionId);
  const compactingSessionIdsRef = useRef(new Set<string>());
  const processedTurnEndSignatureRef = useRef<string | undefined>(undefined);
  const pendingWaitingSessionIdRef = useRef<string | null>(null);
  const preserveWaitingOnSessionChangeRef = useRef(false);
  /// 回撤后屏蔽旧 run 晚到事件，避免 reducer 重新创建运行态。
  const invalidatedHarnessSessionIdsRef = useRef(new Set<string>());
  /// 新建会话落地真实 sessionId 前，复用临时会话运行态避免按钮闪回发送态。
  const pendingRunTransfer = preserveWaitingOnSessionChangeRef.current
    && pendingWaitingSessionIdRef.current === ''
    && activeSessionId !== ''
    && liveRuns[activeSessionId] === undefined;
  const liveRun = liveRuns[activeSessionId] ?? (pendingRunTransfer ? liveRuns[''] ?? CreateLocalWaitingRun() : null);
  const liveRunIsRunning = liveRun?.status === 'running';
  const totalTokens = liveRun?.lastAssistantTotalTokens ?? persistedTotalTokens;
  const isCompacting = compactingSessionIds.has(activeSessionId);

  /// 更新会话运行态；回撤后拒绝任何旧状态写回运行中。
  /// @param sessionId 目标会话 id。
  /// @param isRunning 目标运行状态。
  const UpdateRunningState = useCallback((sessionId: string, isRunning: boolean) => {
    if (isRunning && invalidatedHarnessSessionIdsRef.current.has(sessionId)) {
      return;
    }

    onRunningChange(sessionId, isRunning);
  }, [onRunningChange]);

  /// 判断该会话的 Harness 事件是否属于回撤前的失效 run。
  /// @param sessionId Harness 事件所属会话 id。
  const ShouldIgnoreHarnessEvents = useCallback(
    (sessionId: string) => invalidatedHarnessSessionIdsRef.current.has(sessionId),
    []
  );

  useActiveHarnessEvents(setLiveRuns, ShouldIgnoreHarnessEvents);

  useEffect(() => {
    const turnEndSignature = liveRun?.lastTurnEndSignature;

    if (!turnEndSignature || processedTurnEndSignatureRef.current === turnEndSignature) {
      return;
    }

    processedTurnEndSignatureRef.current = turnEndSignature;
    void CompactAfterTurnEnd().catch((error) => {
      ReportBackendError('处理 turn_end 上下文压缩失败', error);
    });
  }, [activeSessionId, compactRatio, liveRun?.lastTurnEndSignature, tokenLimit, totalTokens]);

  useEffect(() => {
    const previousSessionId = activeSessionIdRef.current;
    activeSessionIdRef.current = activeSessionId;

    if (preserveWaitingOnSessionChangeRef.current) {
      const pendingSessionId = pendingWaitingSessionIdRef.current ?? previousSessionId;
      if (pendingSessionId !== '' || activeSessionId === '') {
        return;
      }

      setLiveRuns((current) => {
        const pendingRun = current[pendingSessionId] ?? null;
        const next = { ...current, [activeSessionId]: CreateLocalWaitingRun(pendingRun) };

        if (pendingSessionId !== activeSessionId) {
          delete next[pendingSessionId];
        }

        return next;
      });
      UpdateRunningState(pendingSessionId, false);
      UpdateRunningState(activeSessionId, true);
      pendingWaitingSessionIdRef.current = activeSessionId;
      preserveWaitingOnSessionChangeRef.current = false;
      return;
    }

    if (previousSessionId === activeSessionId || activeSessionId === '') {
      return;
    }

    /// 已重新从后端打开会话时，丢弃已落盘的前端展示快照，避免与持久化消息重复渲染。
    setLiveRuns((current) => {
      const currentRun = current[activeSessionId];

      if (!currentRun) {
        return current;
      }

      const next = { ...current };

      if (currentRun.status === 'running') {
        next[activeSessionId] = {
          ...currentRun,
          activeMessage: null,
          history: [],
          steps: [],
        };
      } else {
        delete next[activeSessionId];
      }

      return next;
    });
  }, [activeSessionId, UpdateRunningState]);

  useEffect(() => {
    Object.entries(liveRuns).forEach(([sessionId, run]) => {
      UpdateRunningState(sessionId, run.status === 'running');
    });
  }, [liveRuns, UpdateRunningState]);

  /// 提交 prompt 并立即展示本地等待状态。
  /// @param prompt 用户输入文本。
  /// @param images 用户输入图像。
  async function SubmitPromptWithWaiting(submission: ChatPromptSubmission, images: PromptImage[]) {
    const afterUserMessageCount = CountUserMessages(sessionContext?.messages ?? []);
    const shouldCompact = ShouldCompactBeforePrompt();
    const displayText = BuildChatSubmissionDisplayText(submission);

    if (shouldCompact) {
      onAppendUserMessage(displayText, images);
    }

    /// 新一轮本地提交开始后，接收该会话的新 Harness 事件。
    invalidatedHarnessSessionIdsRef.current.delete(activeSessionId);
    pendingWaitingSessionIdRef.current = activeSessionId;
    preserveWaitingOnSessionChangeRef.current = true;
    UpdateRunningState(activeSessionId, true);
    setLiveRuns((current) => ({
      ...current,
      [activeSessionId]: CreateLocalWaitingRun(current[activeSessionId] ?? null, afterUserMessageCount),
    }));

    try {
      await CompactBeforePrompt(shouldCompact);
      await onSubmitPrompt(submission, images, shouldCompact);
    } catch (error) {
      const currentSessionId = activeSessionIdRef.current;
      const pendingSessionId = pendingWaitingSessionIdRef.current;

      setLiveRuns((current) => {
        const next = { ...current };

        delete next[activeSessionId];
        delete next[currentSessionId];
        if (pendingSessionId !== null) {
          delete next[pendingSessionId];
        }

        return next;
      });
      UpdateRunningState(activeSessionId, false);
      UpdateRunningState(currentSessionId, false);
      if (pendingSessionId !== null) {
        UpdateRunningState(pendingSessionId, false);
      }
      throw error;
    } finally {
      preserveWaitingOnSessionChangeRef.current = false;
      pendingWaitingSessionIdRef.current = null;
    }
  }

  /// 清除当前会话残留的本地 Agent 展示状态。
  function ClearActiveLiveRun() {
    UpdateRunningState(activeSessionId, false);
    setLiveRuns((current) => {
      if (current[activeSessionId] === undefined) {
        return current;
      }

      const next = { ...current };
      delete next[activeSessionId];
      return next;
    });
  }

  /// 使当前会话回撤前的 Harness 事件失效，并清除本地展示状态。
  function InvalidateActiveLiveRun() {
    invalidatedHarnessSessionIdsRef.current.add(activeSessionId);
    ClearActiveLiveRun();
  }

  /// 为编辑后重新发送的消息创建本地等待状态。
  /// @param afterUserMessageCount 等待状态应跟随的用户消息数量。
  function StartEditedAgentRun(afterUserMessageCount: number) {
    UpdateRunningState(activeSessionId, true);
    setLiveRuns((current) => ({
      ...current,
      [activeSessionId]: CreateLocalWaitingRun(null, afterUserMessageCount),
    }));
  }

  /// 判断当前会话是否需要在发送前压缩历史上下文。
  function ShouldCompactBeforePrompt() {
    if (
      activeSessionId.length === 0
      || tokenLimit === null
      || !Number.isFinite(totalTokens)
      || !Number.isInteger(compactRatio)
      || compactRatio < 1
      || compactRatio > 100
    ) {
      return false;
    }

    return totalTokens / tokenLimit * 100 >= compactRatio;
  }

  /// 压缩指定会话的历史上下文，并保留既有消息展示。
  /// @param sessionId 需要压缩的会话 id。
  async function CompactSession(sessionId: string) {
    if (compactingSessionIdsRef.current.has(sessionId)) {
      return;
    }

    compactingSessionIdsRef.current.add(sessionId);
    setCompactingSessionIds((current) => new Set(current).add(sessionId));

    try {
      /// 先让压缩状态进入聊天区，再发起耗时的后端调用。
      await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
      await invoke(CompactChatCommand, {
        input: {
          sessionId,
          /// Rust Option 的 None 对应 Tauri JSON 的 null。
          customInstructions: null,
        },
      });
    } finally {
      compactingSessionIdsRef.current.delete(sessionId);
      setCompactingSessionIds((current) => {
        const next = new Set(current);
        next.delete(sessionId);
        return next;
      });
    }
  }

  /// 在发送 prompt 前按已完成的 token 占用情况压缩当前会话。
  /// @param shouldCompact 当前发送前是否需要压缩。
  async function CompactBeforePrompt(shouldCompact: boolean) {
    if (shouldCompact) {
      await CompactSession(activeSessionId);
    }
  }

  /// 收到 TurnEnd 后按当前 token 使用比例压缩会话。
  async function CompactAfterTurnEnd() {
    if (ShouldCompactBeforePrompt()) {
      await CompactSession(activeSessionId);
    }
  }

  return (
    <>
      <header className="ThreadHeader">
        <h1 className="SrOnly" id="thread-title">{title}</h1>
        <ThreadModelSelect
          modelOptions={modelOptions}
          modelOptionsError={modelOptionsError}
          modelOptionsLoading={modelOptionsLoading}
          modelSelection={modelSelection}
          onChangeModel={onChangeModel}
        />
      </header>

      <div className="ThreadBody">
        <MessageViewport
          activeSessionId={activeSessionId}
          isCompacting={isCompacting}
          liveRun={liveRun}
          onClearLiveRun={ClearActiveLiveRun}
          onInvalidateLiveRun={InvalidateActiveLiveRun}
          onEditAgentRun={onEditAgentRun}
          onForkAgentRun={onForkAgentRun}
          onPromptEditedAgentRun={onPromptEditedAgentRun}
          onStartEditedRun={StartEditedAgentRun}
          onWithdrawAgentRun={onWithdrawAgentRun}
          sessionContext={sessionContext}
        />
        <div className="ComposerWrap">
          <ToolApprovalStrip
            approvals={liveRun?.pendingToolApprovals ?? []}
            sessionId={activeSessionId}
          />
          <Composer
            activeSessionId={activeSessionId}
            isCompacting={isCompacting}
            isRunning={isRunning || liveRunIsRunning}
            modelThinkingLevels={modelThinkingLevels}
            modelSelection={modelSelection}
            tokenLimit={tokenLimit}
            totalTokens={totalTokens}
            onAbort={onAbort}
            onChangeThinkingLevel={onChangeThinkingLevel}
            onSubmitPrompt={SubmitPromptWithWaiting}
            toolNames={toolNames}
            toolPreference={toolPreference}
            onChangeToolEnabled={onChangeToolEnabled}
            onChangeToolSelected={onChangeToolSelected}
            onSaveTools={onSaveTools}
            onChangeApproval={onChangeApproval}
          />
        </div>
      </div>
    </>
  );
}

export default ThreadPanel;
