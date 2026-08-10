//! 底层无状态 agent loop：以 AgentMessage 为通用货币驱动 LLM 调用、流式事件转发、工具执行调度。

use std::sync::atomic::Ordering;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    agent::types::*,
    model::{api_registry::AssistantMessageEventSink, types::*},
};

/// Agent loop 错误。
#[derive(Debug, thiserror::Error)]
pub enum AgentLoopError {
    /// 无消息时不能继续。
    #[error("Cannot continue: no messages in context")]
    CannotContinueNoMessages,
    /// 不能从 assistant 消息继续。
    #[error("Cannot continue from message role: assistant")]
    CannotContinueFromAssistant,
    /// prepare_next_turn 钩子执行失败。
    #[error("prepare_next_turn failed: {0}")]
    PrepareNextTurn(String),
    /// Agent 事件下沉失败。
    #[error(transparent)]
    Event(#[from] AgentError),
}

/// 启动一次带新 prompt 消息的 agent loop。
pub async fn agent_loop(
    prompts: Vec<AgentMessage>,
    config: AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    stream_fn: &dyn StreamFn,
) -> Result<Vec<AgentMessage>, AgentLoopError> {
    let mut new_messages = Vec::with_capacity(prompts.len());

    event.emit(AgentEvent::AgentStart).await?;
    event.emit(AgentEvent::TurnStart).await?;
    for prompt in prompts {
        event.emit(AgentEvent::MessageStart { message: &prompt }).await?;
        event.emit(AgentEvent::MessageEnd { message: &prompt }).await?;
        new_messages.push(prompt);
    }

    run_loop(&mut new_messages, config, event, stream_fn).await?;
    Ok(new_messages)
}

/// 不追加新消息地从已有上下文继续 agent loop。
pub async fn agent_loop_continue(
    config: AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    stream_fn: &dyn StreamFn,
) -> Result<Vec<AgentMessage>, AgentLoopError> {
    if event.state.messages.is_empty() {
        return Err(AgentLoopError::CannotContinueNoMessages);
    }
    if matches!(event.state.messages.last(), Some(AgentMessage::Assistant(_))) {
        return Err(AgentLoopError::CannotContinueFromAssistant);
    }

    let mut new_messages = Vec::new();
    event.emit(AgentEvent::AgentStart).await?;
    event.emit(AgentEvent::TurnStart).await?;
    run_loop(&mut new_messages, config, event, stream_fn).await?;
    Ok(new_messages)
}

/// agentLoop 与 agentLoopContinue 共享的主循环。
async fn run_loop(
    new_messages: &mut Vec<AgentMessage>,
    config: AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    stream_fn: &dyn StreamFn,
) -> Result<(), AgentLoopError> {
    let mut first_turn = true;
    let mut pending_messages = drain_messages(&config.get_steering_messages).await?;

    loop {
        let mut has_more_tool_calls = true;
        while has_more_tool_calls || !pending_messages.is_empty() {
            if !first_turn {
                event.emit(AgentEvent::TurnStart).await?;
            } else {
                first_turn = false;
            }

            if is_aborted(&config) {
                event.emit(AgentEvent::AgentEnd { messages: new_messages }).await?;
                return Ok(());
            }

            for message in std::mem::take(&mut pending_messages) {
                event.emit(AgentEvent::MessageStart { message: &message }).await?;
                event.emit(AgentEvent::MessageEnd { message: &message }).await?;
                new_messages.push(message);
            }

            let message = stream_assistant_response(&config, event, stream_fn).await?;

            if matches!(message.stop_reason, StopReason::Error | StopReason::Aborted) || is_aborted(&config) {
                let message = AgentMessage::Assistant(message);
                event.emit(AgentEvent::TurnEnd { message: &message, tool_results: &[] }).await?;
                new_messages.push(message);
                event.emit(AgentEvent::AgentEnd { messages: new_messages }).await?;
                return Ok(());
            }

            let executed = execute_tool_calls(&message, &config, event).await?;
            has_more_tool_calls = !executed.terminate;
            let tool_results = executed.messages;
            let turn_message = AgentMessage::Assistant(message);
            event.emit(AgentEvent::TurnEnd { message: &turn_message, tool_results: &tool_results }).await?;

            new_messages.push(turn_message);
            let tool_start_idx = new_messages.len();
            let mut tool_results_ref = Vec::new();
            for result in tool_results {
                new_messages.push(AgentMessage::ToolResult(result));
            }
            for i in tool_start_idx..new_messages.len() {
                if let Some(AgentMessage::ToolResult(tool_msg)) = new_messages.get(i) {
                    tool_results_ref.push(tool_msg);
                }
            }

            let Some(AgentMessage::Assistant(message)) = new_messages.get(tool_start_idx - 1) else { unreachable!() };
            let mut turn_context = ShouldStopAfterTurnContext {
                message,
                tool_results: &tool_results_ref,
                context: event.state,
                model: config.model,
                stream_options: config.stream_options,
                new_messages,
            };
            if let Some(prepare_next_turn) = &config.prepare_next_turn {
                prepare_next_turn.execute(&mut turn_context, config.env).await?;
            }
            if let Some(should_stop) = &config.should_stop_after_turn {
                if should_stop.execute(&mut turn_context).await? {
                    event.emit(AgentEvent::AgentEnd { messages: new_messages }).await?;
                    return Ok(());
                }
            }
            pending_messages = drain_messages(&config.get_steering_messages).await?;
        }

        let follow_up_messages = drain_messages(&config.get_follow_up_messages).await?;
        if follow_up_messages.is_empty() {
            break;
        }
        pending_messages = follow_up_messages;
    }

    event.emit(AgentEvent::AgentEnd { messages: new_messages }).await?;
    Ok(())
}

/// 调用 LLM 流式生成一条 assistant 响应。
pub async fn stream_assistant_response(
    config: &AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    stream_fn: &dyn StreamFn,
) -> Result<AssistantMessage, AgentLoopError> {
    if let Some(transform_context) = &config.transform_context {
        transform_context.execute(&mut event.state.messages, config.env).await?;
    };
    let llm_context = to_llm_context(config.env, event.state);

    let mut sink = AssistantResponseSink { addedpartial: false, event, config };
    let final_message =
        stream_fn.stream(config.model, llm_context, &config.stream_options, &config.provider_auth, &mut sink).await?;

    let message = AgentMessage::Assistant(final_message);
    if sink.addedpartial {
        event.emit(AgentEvent::MessageEnd { message: &message }).await?;
    } else {
        event.emit(AgentEvent::MessageStart { message: &message }).await?;
        event.emit(AgentEvent::MessageEnd { message: &message }).await?;
    }

    let AgentMessage::Assistant(final_message) = message else { unreachable!() };
    Ok(final_message)
}

#[async_trait]
impl AssistantMessageEventSink for AssistantResponseSink<'_, '_> {
    /// 消费一个 AssistantMessage 流事件。
    async fn emit(&mut self, event: AssistantMessageEvent) -> Result<AssistantMessage, StreamError> {
        Ok(match event {
            AssistantMessageEvent::Start { mut partial } => {
                if is_aborted(self.config) {
                    partial.stop_reason = StopReason::Aborted;
                    partial.error_message = Some("Agent run was aborted".to_string());
                    return Ok(partial);
                }
                let message = AgentMessage::Assistant(partial);
                self.event
                    .emit(AgentEvent::MessageStart { message: &message })
                    .await
                    .map_err(|error| StreamError::Callback(error.to_string()))?;
                let AgentMessage::Assistant(partial) = message else { unreachable!() };
                self.addedpartial = true;
                partial
            }
            AssistantMessageEvent::TextStart { mut partial, .. }
            | AssistantMessageEvent::TextDelta { mut partial, .. }
            | AssistantMessageEvent::TextEnd { mut partial, .. }
            | AssistantMessageEvent::ThinkingStart { mut partial, .. }
            | AssistantMessageEvent::ThinkingDelta { mut partial, .. }
            | AssistantMessageEvent::ThinkingEnd { mut partial, .. }
            | AssistantMessageEvent::ToolCallStart { mut partial, .. }
            | AssistantMessageEvent::ToolCallDelta { mut partial, .. }
            | AssistantMessageEvent::ToolCallEnd { mut partial, .. } => {
                if is_aborted(self.config) {
                    partial.stop_reason = StopReason::Aborted;
                    partial.error_message = Some("Agent run was aborted".to_string());
                    return Ok(partial);
                }
                let message = AgentMessage::Assistant(partial);
                self.event
                    .emit(AgentEvent::MessageUpdate { message: &message })
                    .await
                    .map_err(|error| StreamError::Callback(error.to_string()))?;
                let AgentMessage::Assistant(partial) = message else { unreachable!() };
                partial
            }
            AssistantMessageEvent::Done { message, .. } => message,
        })
    }
}

/// 执行 assistant 消息中的 tool calls。
pub async fn execute_tool_calls(
    message: &AssistantMessage,
    config: &AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
) -> Result<ExecutedToolCallBatch, AgentLoopError> {
    let calls = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolCall(tool_call) => Some(tool_call),
            _ => None,
        })
        .collect::<Vec<_>>();
    let results = match config.tool_execution {
        ToolExecutionMode::Parallel => execute_many_tool_calls(message, config, event, calls).await?,
        ToolExecutionMode::Sequential => {
            let mut results = Vec::with_capacity(calls.len());
            for tool_call in calls {
                results.push(execute_one_tool_call(message, config, event, tool_call).await?);
            }
            results
        }
    };
    let terminate_flags = results.iter().map(|(_, terminate)| *terminate).collect::<Vec<_>>();
    let messages = results.into_iter().map(|(message, _)| message).collect::<Vec<_>>();
    Ok(ExecutedToolCallBatch {
        terminate: terminate_flags.is_empty() || terminate_flags.iter().all(|value| *value),
        messages,
    })
}

/// 并行执行允许并发的 tool calls；单工具顺序覆盖直接复用串行执行路径。
async fn execute_many_tool_calls(
    message: &AssistantMessage,
    config: &AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    calls: Vec<&ToolCall>,
) -> Result<Vec<(ToolResultMessage, bool)>, AgentLoopError> {
    let mut results = Vec::with_capacity(calls.len());
    let mut parallel_calls = Vec::new();
    let mut sequential_calls = Vec::new();

    for tool_call in calls {
        let Some(tool) = event.state.tools.iter().find(|tool| tool.definition().name == tool_call.name).cloned() else {
            let tool_result = error_tool_result(tool_call, "Tool not found");
            let message = AgentMessage::ToolResult(tool_result);
            event.emit(AgentEvent::MessageStart { message: &message }).await?;
            event.emit(AgentEvent::MessageEnd { message: &message }).await?;
            let AgentMessage::ToolResult(tool_result) = message else { unreachable!() };
            results.push((tool_result, false));
            continue;
        };
        let args = tool.prepare_arguments(Value::Object(tool_call.arguments.clone()));
        event
            .emit(AgentEvent::ToolExecutionStart {
                tool_call_id: &tool_call.id,
                tool_name: &tool_call.name,
                args: &args,
            })
            .await?;
        if let Some(before_tool_call) = &config.before_tool_call {
            let mut before_context =
                BeforeToolCallContext { assistant_message: message, tool_call, args: &args, context: event.state };
            if let Some(before_result) = before_tool_call.execute(&mut before_context).await? {
                if before_result.block.unwrap_or(false) {
                    let result = AgentToolResult {
                        content: vec![ContentBlock::Text(TextContent {
                            text: before_result.reason.unwrap_or_else(|| "Tool execution was blocked".to_string()),
                            text_signature: None,
                        })],
                        details: Value::Object(Default::default()),
                        terminate: None,
                    };
                    results.push(complete_tool_call(message, config, event, tool_call, args, result, true).await?);
                    continue;
                }
            }
        }

        if matches!(tool.execution_mode(), Some(ToolExecutionMode::Sequential)) {
            sequential_calls.push((tool_call, tool, args));
        } else {
            parallel_calls.push((tool_call, tool, args));
        }
    }

    let tasks = parallel_calls.into_iter().map(|(tool_call, tool, args)| async move {
        let result = tool.execute(config.env, &tool_call.id, &args, config.update_tool_call).await;
        (tool_call, args, result)
    });
    let sequential_task = async {
        let mut results = Vec::with_capacity(sequential_calls.len());
        for (tool_call, tool, args) in sequential_calls {
            let result = tool.execute(config.env, &tool_call.id, &args, config.update_tool_call).await;
            results.push((tool_call, args, result));
        }
        results
    };
    let (parallel_results, sequential_results) = futures::join!(futures::future::join_all(tasks), sequential_task);
    for (tool_call, args, result) in parallel_results.into_iter().chain(sequential_results) {
        let (result, is_error) = match result {
            Ok(result) => (result, false),
            Err(error) => (
                AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent { text: error.to_string(), text_signature: None })],
                    details: Value::Object(Default::default()),
                    terminate: None,
                },
                true,
            ),
        };
        results.push(complete_tool_call(message, config, event, tool_call, args, result, is_error).await?);
    }

    Ok(results)
}

/// 完成 tool call 的后处理、事件发送及结果消息构造。
async fn complete_tool_call(
    message: &AssistantMessage,
    config: &AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    tool_call: &ToolCall,
    args: Value,
    mut result: AgentToolResult,
    mut is_error: bool,
) -> Result<(ToolResultMessage, bool), AgentLoopError> {
    if let Some(after_tool_call) = &config.after_tool_call {
        let mut after_context = AfterToolCallContext {
            assistant_message: message,
            tool_call,
            args: &args,
            result: &result,
            is_error: &is_error,
            context: event.state,
        };
        if let Some(after_result) = after_tool_call.execute(&mut after_context).await? {
            if let Some(content) = after_result.content {
                result.content = content;
            }
            if let Some(details) = after_result.details {
                result.details = details;
            }
            if let Some(terminate) = after_result.terminate {
                result.terminate = Some(terminate);
            }
            if let Some(next_is_error) = after_result.is_error {
                is_error = next_is_error;
            }
        }
    }
    let value = serde_json::to_value(&result).unwrap_or(Value::Null);
    event
        .emit(AgentEvent::ToolExecutionEnd {
            tool_call_id: &tool_call.id,
            tool_name: &tool_call.name,
            result: &value,
            is_error,
        })
        .await?;
    let tool_result = ToolResultMessage {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        content: result.content,
        details: Some(result.details),
        is_error,
        timestamp: now_millis(),
    };
    let message = AgentMessage::ToolResult(tool_result);
    event.emit(AgentEvent::MessageStart { message: &message }).await?;
    event.emit(AgentEvent::MessageEnd { message: &message }).await?;
    let AgentMessage::ToolResult(tool_result) = message else { unreachable!() };
    let terminate = result.terminate.unwrap_or(false);
    Ok((tool_result, terminate))
}

/// 执行单个 tool call，并返回 toolResult 与 terminate 标志。
async fn execute_one_tool_call(
    message: &AssistantMessage,
    config: &AgentLoopConfig<'_>,
    event: &mut AgentRuntimeEventSink<'_, '_, '_>,
    tool_call: &ToolCall,
) -> Result<(ToolResultMessage, bool), AgentLoopError> {
    let Some(tool) = event.state.tools.iter().find(|tool| tool.definition().name == tool_call.name).cloned() else {
        let tool_result = error_tool_result(&tool_call, "Tool not found");
        let message = AgentMessage::ToolResult(tool_result);
        event.emit(AgentEvent::MessageStart { message: &message }).await?;
        event.emit(AgentEvent::MessageEnd { message: &message }).await?;
        let AgentMessage::ToolResult(tool_result) = message else { unreachable!() };
        return Ok((tool_result, false));
    };
    let args = tool.prepare_arguments(Value::Object(tool_call.arguments.clone()));
    event
        .emit(AgentEvent::ToolExecutionStart { tool_call_id: &tool_call.id, tool_name: &tool_call.name, args: &args })
        .await?;
    let mut executed_result = None;
    let mut is_error = false;
    if let Some(before_tool_call) = &config.before_tool_call {
        let mut before_context = BeforeToolCallContext {
            assistant_message: &message,
            tool_call: &tool_call,
            args: &args,
            context: event.state,
        };
        if let Some(before_result) = before_tool_call.execute(&mut before_context).await? {
            if before_result.block.unwrap_or(false) {
                executed_result = Some(AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent {
                        text: before_result.reason.unwrap_or_else(|| "Tool execution was blocked".to_string()),
                        text_signature: None,
                    })],
                    details: Value::Object(Default::default()),
                    terminate: None,
                });
                is_error = true;
            }
        }
    }
    if executed_result.is_none() {
        match tool.execute(config.env, &tool_call.id, &args, config.update_tool_call).await {
            Ok(result) => executed_result = Some(result),
            Err(error) => {
                executed_result = Some(AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent { text: error.to_string(), text_signature: None })],
                    details: Value::Object(Default::default()),
                    terminate: None,
                });
                is_error = true;
            }
        }
    }
    let mut result = executed_result.unwrap_or_else(|| AgentToolResult {
        content: vec![],
        details: Value::Object(Default::default()),
        terminate: None,
    });
    if let Some(after_tool_call) = &config.after_tool_call {
        let mut after_context = AfterToolCallContext {
            assistant_message: &message,
            tool_call: &tool_call,
            args: &args,
            result: &result,
            is_error: &is_error,
            context: event.state,
        };
        if let Some(after_result) = after_tool_call.execute(&mut after_context).await? {
            if let Some(content) = after_result.content {
                result.content = content;
            }
            if let Some(details) = after_result.details {
                result.details = details;
            }
            if let Some(terminate) = after_result.terminate {
                result.terminate = Some(terminate);
            }
            if let Some(next_is_error) = after_result.is_error {
                is_error = next_is_error;
            }
        }
    }
    let value = serde_json::to_value(&result).unwrap_or(Value::Null);
    event
        .emit(AgentEvent::ToolExecutionEnd {
            tool_call_id: &tool_call.id,
            tool_name: &tool_call.name,
            result: &value,
            is_error,
        })
        .await?;
    let tool_result = ToolResultMessage {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        content: result.content,
        details: Some(result.details),
        is_error,
        timestamp: now_millis(),
    };
    let message = AgentMessage::ToolResult(tool_result);
    event.emit(AgentEvent::MessageStart { message: &message }).await?;
    event.emit(AgentEvent::MessageEnd { message: &message }).await?;
    let AgentMessage::ToolResult(tool_result) = message else { unreachable!() };
    let terminate = result.terminate.unwrap_or(false);
    Ok((tool_result, terminate))
}

/// 一批 tool call 的执行结果。
pub struct ExecutedToolCallBatch {
    /// 生成的 toolResult 消息。
    pub messages: Vec<ToolResultMessage>,
    /// 是否应终止后续推断。
    pub terminate: bool,
}

/// drain 可选消息队列。
async fn drain_messages(hook: &Option<&MessageQueueDrainHook<'_>>) -> Result<Vec<AgentMessage>, AgentLoopError> {
    let mut input = ();
    Ok(match hook {
        Some(hook) => hook.execute(&mut input).await?,
        None => Vec::new(),
    })
}

/// 判断当前 run 是否已 abort。
fn is_aborted(config: &AgentLoopConfig) -> bool {
    config.abort_flag.as_ref().is_some_and(|flag| flag.load(Ordering::SeqCst))
}

/// 构造工具错误结果。
fn error_tool_result(tool_call: &ToolCall, reason: &str) -> ToolResultMessage {
    ToolResultMessage {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        content: vec![ContentBlock::Text(TextContent { text: reason.to_string(), text_signature: None })],
        details: None,
        is_error: true,
        timestamp: now_millis(),
    }
}
