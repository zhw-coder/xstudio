import { I18n } from '../i18n';

export interface BackendErrorNotice {
  detail: string;
  id: number;
  summary: string;
}

/// 全局后端错误提示事件名。
export const BackendErrorEventName = 'xstudio://backend-error';

/// Toast 摘要最大长度，避免后端长错误撑开提示布局。
const BackendErrorSummaryMaxLength = 160;

/// 判断值是否为普通对象。
/// @param value 待判断值。
function IsRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/// 将未知错误安全转成详情文本。
/// @param error 后端或前端捕获的错误对象。
export function GetBackendErrorDetail(error: unknown) {
  if (error instanceof Error) {
    return error.stack || error.message;
  }

  if (typeof error === 'string') {
    return error;
  }

  try {
    const json = JSON.stringify(error, null, 2);

    if (typeof json === 'string') {
      return json;
    }
  } catch (stringifyError) {
    console.error('格式化错误详情失败', stringifyError);
  }

  return String(error);
}

/// 从未知错误读取优先展示的消息。
/// @param error 后端或前端捕获的错误对象。
function GetErrorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === 'string') {
    return error;
  }

  if (IsRecord(error) && typeof error.message === 'string') {
    return error.message;
  }

  return '';
}

/// 截断错误摘要，保留完整详情在展开区域。
/// @param text 原始错误摘要。
function TruncateBackendErrorSummary(text: string) {
  if (text.length <= BackendErrorSummaryMaxLength) {
    return text;
  }

  return `${text.slice(0, BackendErrorSummaryMaxLength - 3)}...`;
}

/// 获取适合 toast 首行展示的错误摘要。
/// @param error 后端或前端捕获的错误对象。
export function GetBackendErrorSummary(error: unknown) {
  const message = GetErrorMessage(error).trim();
  const detail = message.length > 0 ? message : GetBackendErrorDetail(error);
  const firstLine = detail
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0);

  return TruncateBackendErrorSummary(firstLine ?? I18n.errors.fallback);
}

/// 构造全局后端错误提示数据。
/// @param error 后端或前端捕获的错误对象。
export function BuildBackendErrorNotice(error: unknown): BackendErrorNotice {
  return {
    detail: GetBackendErrorDetail(error),
    id: Date.now(),
    summary: GetBackendErrorSummary(error),
  };
}

/// 打印错误对象和可用堆栈。
/// @param message 错误上下文。
/// @param error 后端或前端捕获的错误对象。
export function LogErrorWithStack(message: string, error: unknown) {
  console.error(message, error);

  if (error instanceof Error && error.stack) {
    console.error(error.stack);
  }
}

/// 派发全局后端错误提示事件。
/// @param error 后端或前端捕获的错误对象。
export function DispatchBackendError(error: unknown) {
  const notice = BuildBackendErrorNotice(error);

  window.dispatchEvent(new CustomEvent<BackendErrorNotice>(BackendErrorEventName, { detail: notice }));
  return notice.summary;
}

/// 记录错误并派发全局后端错误提示。
/// @param message 错误上下文。
/// @param error 后端或前端捕获的错误对象。
export function ReportBackendError(message: string, error: unknown) {
  LogErrorWithStack(message, error);
  return DispatchBackendError(error);
}
