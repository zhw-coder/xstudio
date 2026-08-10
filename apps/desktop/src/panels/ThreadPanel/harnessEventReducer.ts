import type {
  ChatAgentHarnessEventPayload,
  LiveAgentRun,
  PendingToolApproval,
  ThreadAgentRunSnapshot,
  ThreadAssistantMessage,
  ThreadContentBlock,
  ThreadContentBlockKind,
} from './types';
import { I18n } from '../../i18n';

interface HarnessEventEnvelope {
  kind: 'agent' | 'harness' | 'unknown';
  event: Record<string, unknown> | null;
}

/// 可折叠内容块类型。
const CollapsibleBlockKinds = new Set<ThreadContentBlockKind>(['thinking', 'toolCall', 'toolResult']);

/// 判断输入是否为普通对象。
/// @param value 待判断值。
export function IsRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/// 稳定序列化 JSON，失败时保留错误上下文。
/// @param value 待序列化值。
function StringifyJson(value: unknown) {
  try {
    return JSON.stringify(value, null, 2) ?? '';
  } catch (error) {
    console.error('序列化消息内容失败', error);
    return String(value);
  }
}

/// 读取对象字符串字段。
/// @param record 源对象。
/// @param keys 候选字段名。
function ReadStringField(record: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const value = record[key];

    if (typeof value === 'string') {
      return value;
    }
  }

  return '';
}

/// 读取对象数组字段。
/// @param record 源对象。
/// @param keys 候选字段名。
function ReadArrayField(record: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const value = record[key];

    if (Array.isArray(value)) {
      return value;
    }
  }

  return [];
}

/// 获取消息 role。
/// @param message 消息对象。
export function GetMessageRole(message: unknown) {
  if (!IsRecord(message)) {
    return '';
  }

  return typeof message.role === 'string' ? message.role : '';
}

/// 获取助手消息的 token 使用量。
/// @param message 原始助手消息。
function GetAssistantTotalTokens(message: unknown) {
  if (!IsRecord(message) || !IsRecord(message.usage)) {
    return undefined;
  }

  const totalTokens = message.usage.totalTokens ?? message.usage.total_tokens;

  return typeof totalTokens === 'number' && Number.isFinite(totalTokens) && totalTokens >= 0
    ? totalTokens
    : undefined;
}

/// 从任意消息结构中提取可展示文本。
/// @param value 待提取值。
export function ExtractMessageText(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map(ExtractMessageText).filter(Boolean).join('\n');
  }

  if (!IsRecord(value)) {
    return '';
  }

  if (typeof value.text === 'string') {
    return value.text;
  }

  if (typeof value.thinking === 'string') {
    return value.thinking;
  }

  if ('content' in value) {
    return ExtractMessageText(value.content);
  }

  if ('payload' in value) {
    return ExtractMessageText(value.payload);
  }

  if ('message' in value) {
    return ExtractMessageText(value.message);
  }

  return '';
}

/// 归一化 AgentHarness 事件外层结构。
/// @param value AgentHarness 事件值。
function GetHarnessEventEnvelope(value: unknown): HarnessEventEnvelope {
  if (!IsRecord(value)) {
    return { kind: 'unknown', event: null };
  }

  if (IsRecord(value.event)) {
    const kind = value.kind === 'Agent' ? 'agent' : value.kind === 'Harness' ? 'harness' : 'unknown';

    return { kind, event: value.event };
  }

  return { kind: 'agent', event: value };
}

/// 获取 Agent/Harness 事件类型。
/// @param event 事件对象。
function GetEventType(event: Record<string, unknown> | null) {
  return typeof event?.type === 'string' ? event.type : '';
}

/// 判断事件类型是否匹配。
/// @param eventType 实际事件类型。
/// @param snakeCase snake_case 类型。
/// @param pascalCase PascalCase 类型。
function IsEventType(eventType: string, snakeCase: string, pascalCase: string) {
  return eventType === snakeCase || eventType === pascalCase;
}

/// 获取消息内容块类型。
/// @param block 内容块。
function GetContentBlockKind(block: Record<string, unknown>): ThreadContentBlockKind {
  if (block.type === 'thinking' || block.type === 'ThinkingContent') {
    return 'thinking';
  }

  if (block.type === 'toolCall' || block.type === 'ToolCall') {
    return 'toolCall';
  }

  if (block.type === 'image' || block.type === 'ImageContent') {
    return 'image';
  }

  return 'text';
}

/// 判断内容块是否为正文文本块。
/// @param block 待判断的内容块。
function IsTextContentBlock(block: unknown) {
  return IsRecord(block) && (block.type === 'text' || block.type === 'TextContent');
}

/// 将正文文本块稳定地排到助手消息末尾展示。
/// @param content 原始助手消息内容块。
function OrderAssistantContentForDisplay(content: unknown[]) {
  return [
    ...content.filter((block) => !IsTextContentBlock(block)),
    ...content.filter(IsTextContentBlock),
  ];
}

/// 判断内容块是否可折叠。
/// @param kind 内容块类型。
function IsCollapsibleBlock(kind: ThreadContentBlockKind) {
  return CollapsibleBlockKinds.has(kind);
}

/// 计算内容块折叠状态。
/// @param kind 内容块类型。
/// @param index 内容块位置。
/// @param lastIndex 最后一个内容块位置。
/// @param collapseProcessBlocks 是否强制折叠过程类内容。
/// @param previousBlock 上一次同位置内容块。
function ResolveBlockCollapsed(
  kind: ThreadContentBlockKind,
  index: number,
  lastIndex: number,
  collapseProcessBlocks: boolean,
  previousBlock: ThreadContentBlock | undefined
) {
  if (!IsCollapsibleBlock(kind)) {
    return false;
  }

  if (collapseProcessBlocks || index < lastIndex) {
    return true;
  }

  return previousBlock?.collapsed ?? false;
}

/// 构造正文文本块。
/// @param rawBlock 原始内容块。
/// @param id 内容块 id。
function BuildTextBlock(rawBlock: Record<string, unknown>, id: string): ThreadContentBlock {
  return {
    id,
    kind: 'text',
    title: I18n.thread.assistantContent,
    text: ReadStringField(rawBlock, ['text']),
    collapsed: false,
  };
}

/// 构造思考块。
/// @param rawBlock 原始内容块。
/// @param id 内容块 id。
/// @param collapsed 是否折叠。
function BuildThinkingBlock(rawBlock: Record<string, unknown>, id: string, collapsed: boolean): ThreadContentBlock {
  return {
    id,
    kind: 'thinking',
    title: I18n.thread.thinkingTooltip,
    text: ReadStringField(rawBlock, ['thinking', 'text']),
    collapsed,
  };
}

/// 构造工具调用块。
/// @param rawBlock 原始内容块。
/// @param id 内容块 id。
/// @param collapsed 是否折叠。
function BuildToolCallBlock(rawBlock: Record<string, unknown>, id: string, collapsed: boolean): ThreadContentBlock {
  const toolCallId = ReadStringField(rawBlock, ['id']);
  const name = ReadStringField(rawBlock, ['name']);
  const argumentsValue = IsRecord(rawBlock.arguments) ? rawBlock.arguments : {};
  const detail = StringifyJson({
    id: ReadStringField(rawBlock, ['id']),
    name,
    arguments: argumentsValue,
  });

  return {
    id: toolCallId || id,
    kind: 'toolCall',
    title: name || I18n.thread.toolCall,
    text: detail,
    detail,
    collapsed,
  };
}

/// 构造图像块。
/// @param rawBlock 原始内容块。
/// @param id 内容块 id。
function BuildImageBlock(rawBlock: Record<string, unknown>, id: string): ThreadContentBlock {
  const data = ReadStringField(rawBlock, ['data']);
  const mimeType = ReadStringField(rawBlock, ['mimeType', 'mime_type']);

  return {
    id,
    kind: 'image',
    title: I18n.thread.assistantImage,
    text: '',
    collapsed: false,
    imageData: data,
    isLoading: data.length === 0,
    mimeType,
  };
}

/// 构造未知内容兜底文本块。
/// @param rawBlock 原始内容块。
/// @param id 内容块 id。
function BuildFallbackBlock(rawBlock: Record<string, unknown>, id: string): ThreadContentBlock {
  return {
    id,
    kind: 'text',
    title: I18n.thread.assistantContent,
    text: ExtractMessageText(rawBlock) || StringifyJson(rawBlock),
    collapsed: false,
  };
}

/// 构造单个助手内容块。
/// @param rawBlock 原始内容块。
/// @param messageId 消息 id。
/// @param index 内容块位置。
/// @param lastIndex 最后一个内容块位置。
/// @param collapseProcessBlocks 是否强制折叠过程类内容。
/// @param previousBlock 上一次同位置内容块。
function BuildAssistantContentBlock(
  rawBlock: unknown,
  messageId: string,
  index: number,
  lastIndex: number,
  collapseProcessBlocks: boolean,
  previousBlock: ThreadContentBlock | undefined
): ThreadContentBlock {
  if (!IsRecord(rawBlock)) {
    return {
      id: `${messageId}-text-${index}`,
      kind: 'text',
      title: I18n.thread.assistantContent,
      text: String(rawBlock ?? ''),
      collapsed: false,
    };
  }

  const kind = GetContentBlockKind(rawBlock);
  const id = `${messageId}-${kind}-${index}`;
  const collapsed = ResolveBlockCollapsed(kind, index, lastIndex, collapseProcessBlocks, previousBlock);

  if (kind === 'text') {
    return BuildTextBlock(rawBlock, id);
  }

  if (kind === 'thinking') {
    return BuildThinkingBlock(rawBlock, id, collapsed);
  }

  if (kind === 'toolCall') {
    return BuildToolCallBlock(rawBlock, id, collapsed);
  }

  if (kind === 'image') {
    return BuildImageBlock(rawBlock, id);
  }

  return BuildFallbackBlock(rawBlock, id);
}

/// 构造助手消息展示模型。
/// @param message 原始助手消息。
/// @param messageId 展示消息 id。
/// @param collapseProcessBlocks 是否强制折叠过程类内容。
/// @param previousMessage 上一次展示消息。
export function BuildAssistantMessageView(
  message: unknown,
  messageId: string,
  collapseProcessBlocks: boolean,
  previousMessage?: ThreadAssistantMessage | null
): ThreadAssistantMessage {
  const content = IsRecord(message) ? ReadArrayField(message, ['content']) : [];
  const orderedContent = OrderAssistantContentForDisplay(content);
  const lastIndex = Math.max(orderedContent.length - 1, 0);

  return {
    id: messageId,
    blocks: orderedContent.map((block, index) => (
      BuildAssistantContentBlock(
        block,
        messageId,
        index,
        lastIndex,
        collapseProcessBlocks,
        previousMessage?.blocks[index]
      )
    )),
    totalTokens: GetAssistantTotalTokens(message),
  };
}

/// 构造工具结果内容块。
/// @param toolResult 原始工具结果消息。
/// @param id 内容块 id。
/// @param collapsed 是否折叠。
export function BuildToolResultBlock(toolResult: unknown, id: string, collapsed: boolean): ThreadContentBlock {
  const record = IsRecord(toolResult) ? toolResult : {};
  const toolName = ReadStringField(record, ['toolName', 'tool_name']);
  const content = record.content;
  const details = record.details;
  const text = ExtractMessageText(content) || StringifyJson(content ?? details ?? record);
  const detail = StringifyJson({
    toolCallId: ReadStringField(record, ['toolCallId', 'tool_call_id']),
    toolName,
    content,
    details,
    isError: Boolean(record.isError ?? record.is_error),
  });

  return {
    id,
    kind: 'toolResult',
    title: toolName ? `${I18n.thread.toolResult}: ${toolName}` : I18n.thread.toolResult,
    text,
    detail,
    collapsed,
    isError: Boolean(record.isError ?? record.is_error),
  };
}

/// 折叠消息内过程类内容块。
/// @param message 展示消息。
function CollapseMessageProcessBlocks(message: ThreadAssistantMessage): ThreadAssistantMessage {
  return {
    ...message,
    blocks: message.blocks.map((block) => (
      IsCollapsibleBlock(block.kind)
        ? { ...block, collapsed: true }
        : block
    )),
  };
}

/// 折叠 step 内所有可折叠内容。
/// @param blocks 内容块列表。
function CollapseStepBlocks(blocks: ThreadContentBlock[]) {
  return blocks.map((block) => (
    IsCollapsibleBlock(block.kind)
      ? { ...block, collapsed: true }
      : block
  ));
}

/// 新建运行状态。
/// @param waiting 是否展示等待点。
function CreateRunningState(waiting: boolean): LiveAgentRun {
  return {
    agentStarted: false,
    status: 'running',
    lastEventSignature: '',
    lastTurnEndSignature: undefined,
    lastAssistantTotalTokens: undefined,
    activeMessage: null,
    history: [],
    steps: [],
    waiting,
    pendingToolApprovals: [],
    error: undefined,
  };
}

/// 构造用于去重的 Harness 事件标识。
/// @param payload AgentHarness 前端事件载荷。
function BuildEventSignature(payload: ChatAgentHarnessEventPayload) {
  return `${payload.timestamp}:${StringifyJson(payload.event)}`;
}

/// 标记已处理的 Harness 事件。
/// @param run 更新后的运行状态。
/// @param eventSignature 当前事件标识。
function MarkProcessedEvent(run: LiveAgentRun, eventSignature: string): LiveAgentRun {
  return {
    ...run,
    lastEventSignature: eventSignature,
  };
}

/// 合并当前轮次的工具结果，使用稳定块 id 覆盖重复事件。
/// @param activeMessage 当前活跃助手消息。
/// @param resultBlocks 当前 TurnEnd 生成的工具结果块。
function MergeToolResultBlocks(
  activeMessage: ThreadAssistantMessage,
  resultBlocks: ThreadContentBlock[]
) {
  const resultBlockIds = new Set(resultBlocks.map((block) => block.id));

  return {
    ...activeMessage,
    blocks: [
      ...activeMessage.blocks.filter((block) => !resultBlockIds.has(block.id)),
      ...resultBlocks,
    ],
  };
}

/// 将当前活跃轮次归档进 steps。
/// @param current 当前 live run。
function ArchiveActiveTurn(current: LiveAgentRun): LiveAgentRun {
  if (!current.activeMessage || current.activeMessage.blocks.length === 0) {
    return current;
  }

  const index = current.steps.length + 1;
  const blocks = CollapseStepBlocks(current.activeMessage.blocks);

  return {
    ...current,
    activeMessage: null,
    steps: [
      ...current.steps,
      {
        id: `agent-step-${index}`,
        index,
        blocks,
      },
    ],
  };
}

/// 判断当前 run 是否有可展示内容。
/// @param current 当前 live run。
function HasRunSnapshotContent(current: LiveAgentRun) {
  return current.steps.length > 0 || Boolean(current.activeMessage && current.activeMessage.blocks.length > 0);
}

/// 构造已完成 run 的展示快照。
/// @param current 当前 live run。
/// @param afterUserMessageCount 快照应跟随的用户消息数量。
function BuildRunSnapshot(
  current: LiveAgentRun,
  afterUserMessageCount?: number
): ThreadAgentRunSnapshot | null {
  if (!HasRunSnapshotContent(current)) {
    return null;
  }

  return {
    id: `live-run-${current.history.length + 1}`,
    afterUserMessageCount,
    assistantMessage: current.activeMessage ? CollapseMessageProcessBlocks(current.activeMessage) : null,
    steps: current.steps.map((step) => ({
      ...step,
      blocks: CollapseStepBlocks(step.blocks),
    })),
  };
}

/// 开始下一次 run，并保留上一轮已完成展示内容。
/// @param current 当前 live run。
/// @param waiting 是否展示等待点。
/// @param afterUserMessageCount 快照应跟随的用户消息数量。
function StartNextRun(
  current: LiveAgentRun | null,
  waiting: boolean,
  afterUserMessageCount?: number
): LiveAgentRun {
  if (current === null) {
    return CreateRunningState(waiting);
  }

  if (current.status === 'running') {
    return {
      ...current,
      status: 'running',
      waiting,
      error: undefined,
    };
  }

  const snapshot = BuildRunSnapshot(current, afterUserMessageCount);

  return {
    agentStarted: false,
    status: 'running',
    activeMessage: null,
    history: snapshot ? [...current.history, snapshot] : current.history,
    steps: [],
    waiting,
    pendingToolApprovals: [],
    error: undefined,
  };
}

/// 将工具执行结束事件对应的待审批项移除。
/// @param run 当前运行态。
/// @param event 工具执行结束事件。
function ClearCompletedToolApproval(run: LiveAgentRun, event: Record<string, unknown>) {
  const toolCallId = ReadStringField(event, ['toolCallId', 'tool_call_id']);

  if (!toolCallId) {
    return run;
  }

  return {
    ...run,
    pendingToolApprovals: run.pendingToolApprovals.filter((approval) => approval.toolCallId !== toolCallId),
  };
}

/// 将客户端工具审批请求归并到当前 live run。
/// @param current 当前 live run 状态。
/// @param approval 待审批工具调用。
export function AddPendingToolApproval(
  current: LiveAgentRun | null,
  approval: PendingToolApproval
): LiveAgentRun {
  const run = EnsureRunningState(current);

  if (run.pendingToolApprovals.some((item) => item.approvalId === approval.approvalId)) {
    return run;
  }

  return {
    ...run,
    waiting: false,
    pendingToolApprovals: [...run.pendingToolApprovals, approval],
  };
}

/// 新建本地提交后的等待状态。
/// @param current 当前 live run。
/// @param afterUserMessageCount 已完成回复应跟随的用户消息数量。
export function CreateLocalWaitingRun(
  current: LiveAgentRun | null = null,
  afterUserMessageCount?: number
): LiveAgentRun {
  return StartNextRun(current, true, afterUserMessageCount);
}

/// 判断消息是否为助手消息。
/// @param message 原始消息。
function IsAssistantMessage(message: unknown) {
  return GetMessageRole(message) === 'assistant';
}

/// 读取 AgentEvent 中的 message 字段。
/// @param event AgentEvent 对象。
function ReadEventMessage(event: Record<string, unknown>) {
  return event.message;
}

/// 读取 TurnEnd 中的 tool_results 字段。
/// @param event AgentEvent 对象。
function ReadTurnToolResults(event: Record<string, unknown>) {
  return ReadArrayField(event, ['toolResults', 'tool_results']);
}

/// 读取 TurnEnd 消息中的图像内容块。
/// @param message TurnEnd 对应的 Agent 消息。
/// @param messageId 展示消息 id。
function BuildTurnEndImageBlocks(message: unknown, messageId: string) {
  if (!IsRecord(message)) {
    return [];
  }

  return ReadArrayField(message, ['content']).flatMap((contentBlock, index) => {
    if (!IsRecord(contentBlock) || GetContentBlockKind(contentBlock) !== 'image') {
      return [];
    }

    return [BuildImageBlock(contentBlock, `${messageId}-image-${index}`)];
  });
}

/// 确保 reducer 有一个可更新的运行态。
/// @param current 当前 live run。
function EnsureRunningState(current: LiveAgentRun | null): LiveAgentRun {
  return current?.status === 'running' ? current : StartNextRun(current, false);
}

/// 判断助手消息是否已有可展示内容块。
/// @param message 助手消息展示模型。
function HasVisibleAssistantBlocks(message: ThreadAssistantMessage) {
  return message.blocks.some((block) => (
    block.kind === 'image'
    || block.text.trim().length > 0
    || (block.detail?.trim().length ?? 0) > 0
  ));
}

/// 归并 AgentHarness 事件到当前 live run 状态。
/// @param current 当前 live run 状态。
/// @param payload AgentHarness 前端事件载荷。
export function ReduceHarnessEvent(
  current: LiveAgentRun | null,
  payload: ChatAgentHarnessEventPayload
): LiveAgentRun | null {
  const eventSignature = BuildEventSignature(payload);

  if (current?.lastEventSignature === eventSignature) {
    return current;
  }

  const envelope = GetHarnessEventEnvelope(payload.event);
  const event = envelope.event;
  const eventType = GetEventType(event);

  if (envelope.kind === 'harness' && IsEventType(eventType, 'settled', 'Settled')) {
    return MarkProcessedEvent({
      ...(current ?? CreateRunningState(false)),
      status: 'completed',
      waiting: false,
    }, eventSignature);
  }

  if (envelope.kind !== 'agent' || event === null) {
    return current;
  }

  if (IsEventType(eventType, 'agent_start', 'AgentStart')) {
    return MarkProcessedEvent({
      ...StartNextRun(current, current?.waiting ?? false),
      agentStarted: true,
    }, eventSignature);
  }

  if (IsEventType(eventType, 'message_start', 'MessageStart')) {
    const message = ReadEventMessage(event);
    if (!IsAssistantMessage(message)) {
      return current;
    }

    const archived = ArchiveActiveTurn(EnsureRunningState(current));
    const activeMessage = BuildAssistantMessageView(message, `live-assistant-${payload.timestamp}`, false);

    return MarkProcessedEvent({
      ...archived,
      status: 'running',
      activeMessage,
      waiting: !HasVisibleAssistantBlocks(activeMessage),
    }, eventSignature);
  }

  if (IsEventType(eventType, 'message_update', 'MessageUpdate')) {
    const message = ReadEventMessage(event);
    if (!IsAssistantMessage(message)) {
      return current;
    }

    const run = EnsureRunningState(current);
    const activeMessage = BuildAssistantMessageView(
      message,
      run.activeMessage?.id ?? `live-assistant-${payload.timestamp}`,
      false,
      run.activeMessage
    );

    return MarkProcessedEvent({
      ...run,
      status: 'running',
      activeMessage,
      waiting: !HasVisibleAssistantBlocks(activeMessage),
    }, eventSignature);
  }

  if (IsEventType(eventType, 'message_end', 'MessageEnd')) {
    const message = ReadEventMessage(event);
    if (!IsAssistantMessage(message)) {
      return current;
    }

    const run = EnsureRunningState(current);
    const activeMessage = BuildAssistantMessageView(
      message,
      run.activeMessage?.id ?? `live-assistant-${payload.timestamp}`,
      true,
      run.activeMessage
    );

    return MarkProcessedEvent({
      ...run,
      activeMessage,
      lastAssistantTotalTokens: activeMessage.totalTokens,
      waiting: true,
    }, eventSignature);
  }

  if (IsEventType(eventType, 'turn_end', 'TurnEnd')) {
    const run = EnsureRunningState(current);
    const toolResults = ReadTurnToolResults(event);
    const activeMessage = run.activeMessage ?? { id: `live-assistant-${payload.timestamp}`, blocks: [] };
    const imageBlocks = BuildTurnEndImageBlocks(ReadEventMessage(event), activeMessage.id);

    if (toolResults.length === 0 && imageBlocks.length === 0) {
      return MarkProcessedEvent({
        ...run,
        lastTurnEndSignature: eventSignature,
        waiting: true,
      }, eventSignature);
    }

    const resultBlocks = toolResults.map((toolResult, index) => (
      BuildToolResultBlock(toolResult, `${run.activeMessage?.id ?? 'live-assistant'}-tool-result-${index}`, false)
    ));

    return MarkProcessedEvent({
      ...run,
      activeMessage: MergeToolResultBlocks(activeMessage, [...resultBlocks, ...imageBlocks]),
      lastTurnEndSignature: eventSignature,
      waiting: true,
    }, eventSignature);
  }

  if (IsEventType(eventType, 'tool_execution_end', 'ToolExecutionEnd')) {
    return MarkProcessedEvent(ClearCompletedToolApproval(EnsureRunningState(current), event), eventSignature);
  }

  if (IsEventType(eventType, 'agent_end', 'AgentEnd')) {
    const run = current ?? CreateRunningState(false);

    return MarkProcessedEvent({
      ...run,
      status: 'completed',
      activeMessage: run.activeMessage ? CollapseMessageProcessBlocks(run.activeMessage) : null,
      pendingToolApprovals: [],
      waiting: false,
    }, eventSignature);
  }

  return current;
}
