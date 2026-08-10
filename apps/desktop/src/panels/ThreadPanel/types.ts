export interface ThreadModelOptions {
  preference: ThreadModelPreference;
  modelThinkingLevels: string[];
  providerModelIdsMap: Record<string, string[]>;
  providerModelTokensMap: Record<string, number>;
}

export interface ThreadModelPreference {
  preferenceKey: string;
  modelRecordSelection: string;
  modelThinkingLevel: string;
  tools: string[];
  /** 审批权限：0 表示默认审批，1 表示绕过审批。 */
  approval: number;
}

export interface ThreadModelSelection {
  thinkingLevel: string;
  modelKey: string;
}

export interface ChatSessionModelSelection {
  provider: string;
  modelId: string;
}

export interface ChatSessionContext {
  messages: unknown[];
  thinkingLevel: string;
  model?: ChatSessionModelSelection | null;
}

/// 发送到后端的图片内容。
export interface PromptImage {
  data: string;
  mimeType: string;
}

/// 斜杠菜单资源类型。
export type ChatResourceKind = 'template' | 'skill';

/// 可在聊天输入区显式调用的资源。
export interface ChatResourceItem {
  name: string;
  description: string;
  kind: ChatResourceKind;
}

/// 聊天输入区提交的内容。
export interface ChatPromptSubmission {
  text: string;
  resource: ChatResourceItem | null;
}

export interface ChatAgentHarnessEventPayload {
  sessionId: string;
  event: unknown;
  timestamp: number;
}

/// 后端请求客户端确认工具调用的事件载荷。
export interface ChatToolApprovalRequestedPayload {
  approvalId: string;
  sessionId: string;
  toolCallId: string;
  toolName: string;
  args: Record<string, unknown>;
  timestamp: number;
}

/// 等待客户端确认的工具调用。
export interface PendingToolApproval {
  approvalId: string;
  toolCallId: string;
  toolName: string;
  args: Record<string, unknown>;
}

export type ThreadContentBlockKind = 'text' | 'thinking' | 'toolCall' | 'image' | 'toolResult';

export interface ThreadContentBlock {
  id: string;
  kind: ThreadContentBlockKind;
  title: string;
  text: string;
  collapsed: boolean;
  detail?: string;
  imageData?: string;
  isError?: boolean;
  isLoading?: boolean;
  mimeType?: string;
}

export interface ThreadAssistantMessage {
  id: string;
  blocks: ThreadContentBlock[];
  totalTokens?: number;
}

export interface ThreadAgentStep {
  id: string;
  index: number;
  blocks: ThreadContentBlock[];
}

export interface ThreadAgentRunSnapshot {
  id: string;
  afterUserMessageCount?: number;
  assistantMessage: ThreadAssistantMessage | null;
  steps: ThreadAgentStep[];
}

export interface LiveAgentRun {
  agentStarted: boolean;
  status: 'running' | 'completed' | 'failed';
  lastEventSignature?: string;
  lastTurnEndSignature?: string;
  lastAssistantTotalTokens?: number;
  activeMessage: ThreadAssistantMessage | null;
  history: ThreadAgentRunSnapshot[];
  steps: ThreadAgentStep[];
  waiting: boolean;
  pendingToolApprovals: PendingToolApproval[];
  error?: string;
}

export type LiveAgentRunMap = Record<string, LiveAgentRun>;
