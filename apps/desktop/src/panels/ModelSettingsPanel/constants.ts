import { I18n } from '../../i18n';
import type { JsonObject, ModelInputValue } from './types';

/// 模型参数可编辑输入类型。
export const ModelInputOptions: ModelInputValue[] = ['text', 'image'];

/// 获取上下文窗口默认占位提示。
export function GetContextWindowPlaceholder() {
  return I18n.modelSettings.contextWindowPlaceholder;
}

/// 获取兼容性配置默认占位提示。
export function GetCompatPlaceholder() {
  return I18n.modelSettings.compatPlaceholder;
}

/// 获取最大输出 token 默认占位提示。
export function GetMaxTokensPlaceholder() {
  return I18n.modelSettings.maxTokensPlaceholder;
}

/// 获取思考档位映射默认占位提示。
export function GetThinkingLevelMapPlaceholder() {
  return I18n.modelSettings.thinkingLevelMapPlaceholder;
}

/// 计费默认字段，保持空 cost 可恢复为可编辑对象。
export const DefaultModelCost: JsonObject = {
  cacheRead: 0,
  cacheWrite: 0,
  input: 0,
  output: 0,
};

/// 获取新增模型商默认名称。
export function GetDefaultProviderName() {
  return I18n.modelSettings.defaultProviderName;
}

/// 未保存模型商 ID 前缀。
export const NewProviderIdPrefix = 'new-provider';

/// 空 API 值。
export const EmptyApiValue = '';
