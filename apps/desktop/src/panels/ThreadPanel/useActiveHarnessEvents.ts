import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { AddPendingToolApproval, ReduceHarnessEvent } from './harnessEventReducer';
import type { ChatAgentHarnessEventPayload, ChatToolApprovalRequestedPayload, LiveAgentRunMap } from './types';

/// AgentHarness 前端事件名。
const ChatAgentHarnessEventName = 'chat://agent-harness-event';
/// 工具审批请求事件名。
const ChatToolApprovalRequestedEventName = 'chat://tool-approval-requested';

/// 监听全部会话的 AgentHarness 事件。
/// @param onChangeLiveRuns 更新按会话隔离的 live run 状态。
/// @param shouldIgnoreSession 判断会话事件是否已失效。
export function useActiveHarnessEvents(
  onChangeLiveRuns: (updater: (current: LiveAgentRunMap) => LiveAgentRunMap) => void,
  shouldIgnoreSession: (sessionId: string) => boolean
) {
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let unlistenApproval: (() => void) | undefined;

    listen<ChatAgentHarnessEventPayload>(ChatAgentHarnessEventName, (event) => {
      const payload = event.payload;

      if (shouldIgnoreSession(payload.sessionId)) {
        return;
      }

      onChangeLiveRuns((current) => {
        const nextRun = ReduceHarnessEvent(current[payload.sessionId] ?? null, payload);

        if (nextRun === null) {
          return current;
        }

        return {
          ...current,
          [payload.sessionId]: nextRun,
        };
      });
      })
      .then((dispose) => {
        if (disposed) {
          dispose();
        } else {
          unlisten = dispose;
        }
      })
      .catch((error) => {
        console.error('监听 AgentHarness 事件失败', error);
      });

    listen<ChatToolApprovalRequestedPayload>(ChatToolApprovalRequestedEventName, (event) => {
      const approval = event.payload;

      if (shouldIgnoreSession(approval.sessionId)) {
        return;
      }

      onChangeLiveRuns((current) => ({
        ...current,
        [approval.sessionId]: AddPendingToolApproval(current[approval.sessionId] ?? null, approval),
      }));
    })
      .then((dispose) => {
        if (disposed) {
          dispose();
        } else {
          unlistenApproval = dispose;
        }
      })
      .catch((error) => {
        console.error('监听工具审批请求失败', error);
      });

    return () => {
      disposed = true;
      unlisten?.();
      unlistenApproval?.();
    };
  }, [onChangeLiveRuns, shouldIgnoreSession]);
}
