import type { SelectFieldOption } from '../../components';
import { I18n } from '../../i18n';
import { GetBackendErrorSummary, LogErrorWithStack, ReportBackendError } from '../../utils/backendError';
import { DefaultModelCost, EmptyApiValue, GetDefaultProviderName, ModelInputOptions } from './constants';
import type {
  AllProviderModelsOutput,
  JsonObject,
  ModelItem,
  ModelJson,
  ModelParamEntry,
  ModelParamsDraft,
  ModelRecord,
  ProviderItem,
  ProviderModels,
} from './types';

/// 记录模型设置异常和堆栈。
/// @param message 异常上下文。
/// @param error 异常对象。
export function LogModelSettingsError(message: string, error: unknown) {
  LogErrorWithStack(message, error);
}

/// 记录模型设置后端异常并派发全局提示。
/// @param message 异常上下文。
/// @param error 异常对象。
export function ReportModelSettingsBackendError(message: string, error: unknown) {
  return ReportBackendError(message, error);
}

/// 获取可显示的错误消息。
/// @param error 异常对象。
export function GetErrorMessage(error: unknown) {
  return GetBackendErrorSummary(error) || I18n.errors.fallback;
}

/// 获取默认 API 标识。
/// @param apiProviderApis 后端返回的 API 标识列表。
export function GetDefaultApi(apiProviderApis: string[]) {
  return apiProviderApis[0] ?? EmptyApiValue;
}

/// 归一化 API 标识，不在数据集内时使用数据集第一项。
/// @param api 当前 API 标识。
/// @param apiProviderApis 后端返回的 API 标识列表。
export function NormalizeApi(api: string, apiProviderApis: string[]) {
  if (apiProviderApis.includes(api)) {
    return api;
  }

  return GetDefaultApi(apiProviderApis) || api;
}

/// 构造 API 下拉选项。
/// @param apiProviderApis 后端返回的 API 标识列表。
export function BuildApiSelectOptions(apiProviderApis: string[]): SelectFieldOption<string>[] {
  return apiProviderApis.map((api) => ({
    label: api,
    value: api,
  }));
}

/// 判断值是否为普通 JSON 对象。
/// @param value 待判断值。
export function IsJsonObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/// 解析 ModelRecord.modelJson。
/// @param record 模型持久化记录。
export function ParseModelJson(record: ModelRecord): ModelJson {
  try {
    const model = JSON.parse(record.modelJson);

    if (IsJsonObject(model)) {
      return model as ModelJson;
    }

    throw new Error('ModelRecord.modelJson 不是对象');
  } catch (error) {
    LogModelSettingsError(`解析模型 JSON 失败: ${record.providerName}/${record.modelId}`, error);
    return { api: EmptyApiValue, id: record.modelId };
  }
}

/// 从模型 JSON 中读取 API 标识。
/// @param model 模型 JSON 对象。
function GetModelApi(model: ModelJson) {
  return typeof model.api === 'string' ? model.api : EmptyApiValue;
}

/// 从多个键中读取第一个 JSON 对象。
/// @param source 数据源对象。
/// @param keys 备选键。
function GetObjectValue(source: JsonObject, keys: string[]) {
  for (const key of keys) {
    const value = source[key];

    if (IsJsonObject(value)) {
      return value;
    }
  }

  return {};
}

/// 从多个键中读取第一个数字或数字字符串。
/// @param source 数据源对象。
/// @param keys 备选键。
function GetNumericTextValue(source: JsonObject, keys: string[]) {
  for (const key of keys) {
    const value = source[key];

    if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
      return String(value);
    }

    if (typeof value === 'string' && value.trim().length > 0) {
      return value;
    }
  }

  return '';
}

/// 解析用户输入的非负数字。
/// @param value 输入文本。
function ParseOptionalNumber(value: string) {
  const trimmedValue = value.trim();

  if (trimmedValue.length === 0) {
    return undefined;
  }

  const numericValue = Number(trimmedValue);

  return Number.isFinite(numericValue) && numericValue >= 0 ? numericValue : undefined;
}

/// 格式化 JSON 值为只读展示文本。
/// @param value JSON 值。
export function FormatJsonDisplay(value: unknown) {
  if (IsJsonObject(value) && Object.keys(value).length === 0) {
    return '';
  }

  return JSON.stringify(value);
}

/// 格式化 compat 为单行输入文本。
/// @param value Provider 兼容性配置。
function FormatCompatInputValue(value: unknown) {
  if (typeof value === 'string') {
    return value;
  }

  if (value === undefined || value === null) {
    return '';
  }

  return JSON.stringify(value);
}

/// 格式化键值编辑框中的 JSON 值。
/// @param value JSON 值。
function FormatParamEntryValue(value: unknown) {
  if (typeof value === 'string') {
    return value;
  }

  return JSON.stringify(value);
}

/// 格式化 JSON 键值编辑框中的值。
/// @param value JSON 值。
export function FormatJsonParamEntryValue(value: unknown) {
  return JSON.stringify(value);
}

/// 判断输入是否需要按 JSON 解析。
/// @param value 输入文本。
function ShouldParseJsonEntryValue(value: string) {
  return value.startsWith('{')
    || value.startsWith('[')
    || value.startsWith('"')
    || value === 'true'
    || value === 'false'
    || value === 'null'
    || /^-?\d/.test(value);
}

/// 解析键值编辑框中的 JSON 值。
/// @param value 输入文本。
export function ParseParamEntryValue(value: string) {
  const trimmedValue = value.trim();

  if (trimmedValue.length === 0) {
    return undefined;
  }

  if (!ShouldParseJsonEntryValue(trimmedValue)) {
    return value;
  }

  try {
    return JSON.parse(trimmedValue);
  } catch (error) {
    LogModelSettingsError(`解析参数值失败: ${value}`, error);
    return value;
  }
}

/// 将对象转换为键值编辑行。
/// @param source JSON 对象。
/// @param prefix 行 ID 前缀。
/// @param jsonValue 是否按 JSON 文本展示值。
export function BuildParamEntries(source: JsonObject, prefix: string, jsonValue = false): ModelParamEntry[] {
  return Object.entries(source).map(([key, value], index) => ({
    id: `${prefix}:${index}:${key}`,
    key,
    value: jsonValue ? FormatJsonParamEntryValue(value) : FormatParamEntryValue(value),
  }));
}

/// 将键值编辑行转换为 JSON 对象。
/// @param entries 键值编辑行。
export function BuildObjectFromParamEntries(entries: ModelParamEntry[]) {
  return entries.reduce<JsonObject>((output, entry) => {
    const key = entry.key.trim();
    const value = ParseParamEntryValue(entry.value);

    if (key.length > 0 && value !== undefined) {
      output[key] = value;
    }

    return output;
  }, {});
}

/// 将键值编辑行转换为字符串对象。
/// @param entries 键值编辑行。
function BuildStringObjectFromParamEntries(entries: ModelParamEntry[]) {
  return entries.reduce<Record<string, string>>((output, entry) => {
    const key = entry.key.trim();
    const value = entry.value.trim();

    if (key.length > 0 && value.length > 0) {
      output[key] = value;
    }

    return output;
  }, {});
}

/// 构造模型参数编辑草稿。
/// @param model 模型 JSON。
export function BuildModelParamsDraft(model: ModelJson): ModelParamsDraft {
  const rawInput = Array.isArray(model.input)
    ? model.input.filter((value): value is string => typeof value === 'string')
    : [];
  const inputValues = ModelInputOptions.filter((value) => rawInput.includes(value));
  const extraInputValues = rawInput.filter((value) => !ModelInputOptions.includes(value as never));
  const headers = GetObjectValue(model, ['headers']);
  return {
    compat: FormatCompatInputValue(model.compat),
    contextWindow: GetNumericTextValue(model, ['contextWindow', 'context_window']),
    cost: { ...DefaultModelCost, ...GetObjectValue(model, ['cost']) },
    extraInputValues,
    headers: BuildParamEntries(headers, 'headers'),
    inputValues,
    maxTokens: GetNumericTextValue(model, ['maxTokens', 'max_tokens']),
    reasoning: model.reasoning === true,
    thinkingLevelMap: GetObjectValue(model, ['thinkingLevelMap', 'thinking_level_map']),
  };
}

/// 将模型参数草稿写回模型 JSON。
/// @param model 原模型 JSON。
/// @param draft 模型参数草稿。
export function ApplyModelParamsDraft(model: ModelJson, draft: ModelParamsDraft): ModelJson {
  const nextModel: ModelJson = {
    ...model,
    cost: draft.cost,
    headers: BuildStringObjectFromParamEntries(draft.headers),
    input: [...draft.inputValues, ...draft.extraInputValues],
    reasoning: draft.reasoning,
    thinkingLevelMap: draft.thinkingLevelMap,
  };
  const contextWindow = ParseOptionalNumber(draft.contextWindow);
  const maxTokens = ParseOptionalNumber(draft.maxTokens);
  const compat = draft.compat;

  delete nextModel.context_window;
  delete nextModel.max_tokens;
  delete nextModel.thinking_level_map;

  nextModel.contextWindow = contextWindow ?? 0;
  nextModel.maxTokens = maxTokens ?? 0;

  if (compat.trim().length === 0) {
    delete nextModel.compat;
  } else {
    nextModel.compat = compat;
  }

  return nextModel;
}

/// 从后端模型记录构造界面模型行。
/// @param record 模型持久化记录。
/// @param apiProviderApis 后端返回的 API 标识列表。
export function BuildModelItem(record: ModelRecord, apiProviderApis: string[]): ModelItem {
  const model = ParseModelJson(record);
  const protocol = NormalizeApi(GetModelApi(model), apiProviderApis);

  return {
    enabled: record.status,
    model,
    modelId: record.modelId,
    name: record.modelId,
    protocol,
    providerName: record.providerName,
    recordKey: record.recordKey,
  };
}

/// 构造模型商状态。
/// @param models 模型行列表。
export function GetProviderStatus(models: ModelItem[]): ProviderItem['status'] {
  return models.length > 0 ? 'active' : 'idle';
}

/// 从后端 provider map 条目构造界面模型商。
/// @param providerName provider_models_map 的 key。
/// @param providerModels 后端 ProviderModels 数据。
/// @param apiProviderApis 后端返回的 API 标识列表。
function BuildProviderItem(
  providerName: string,
  providerModels: ProviderModels,
  apiProviderApis: string[],
): ProviderItem {
  const savedProvider = { ...providerModels.provider, name: providerName };
  const provider = {
    ...savedProvider,
    api: NormalizeApi(savedProvider.api, apiProviderApis),
  };
  const models = providerModels.models.map((record) => BuildModelItem(record, apiProviderApis));
  const savedModels = providerModels.models.map((record) => BuildModelItem(record, []));

  return {
    id: `provider:${providerName}`,
    models,
    name: providerName,
    provider,
    savedModels,
    savedProvider,
    status: GetProviderStatus(models),
  };
}

/// 从后端聚合输出构造模型商列表。
/// @param output 后端聚合输出。
export function BuildProviderItems(output: AllProviderModelsOutput): ProviderItem[] {
  return Object.entries(output.providerModelsMap)
    .sort(([leftName], [rightName]) => leftName.localeCompare(rightName))
    .map(([providerName, providerModels]) => BuildProviderItem(providerName, providerModels, output.apiProviderApis));
}

/// 构造不重复的模型商名称。
/// @param baseName 用户输入名称。
/// @param providers 当前模型商列表。
/// @param editingProviderId 当前编辑模型商 ID。
export function BuildUniqueProviderName(baseName: string, providers: ProviderItem[], editingProviderId: string) {
  const normalizedBaseName = baseName.trim() || GetDefaultProviderName();
  const usedNames = new Set(
    providers
      .filter((provider) => provider.id !== editingProviderId)
      .map((provider) => provider.provider.name),
  );
  let nextName = normalizedBaseName;
  let suffix = 1;

  while (usedNames.has(nextName)) {
    nextName = `${normalizedBaseName}(${suffix})`;
    suffix += 1;
  }

  return nextName;
}

/// 将界面模型行转换为后端 ModelRecord。
/// @param providerName 模型商名称。
/// @param model 模型行。
export function BuildModelRecord(providerName: string, model: ModelItem): ModelRecord {
  const modelJson = {
    ...model.model,
    api: model.protocol,
    id: typeof model.model.id === 'string' ? model.model.id : model.modelId,
  };

  return {
    modelId: model.modelId,
    modelJson: JSON.stringify(modelJson),
    providerName,
    recordKey: `${providerName}:${model.modelId}`,
    status: model.enabled,
  };
}

/// 判断模型列表是否被编辑。
/// @param models 当前模型列表。
/// @param savedModels 已保存模型列表。
function IsModelListDirty(models: ModelItem[], savedModels: ModelItem[] | null) {
  if (savedModels === null) {
    return models.length > 0;
  }

  if (models.length !== savedModels.length) {
    return true;
  }

  const savedModelMap = new Map(savedModels.map((model) => [model.modelId, model]));

  return models.some((model) => {
    const savedModel = savedModelMap.get(model.modelId);

    return savedModel === undefined
      || savedModel.enabled !== model.enabled
      || savedModel.protocol !== model.protocol
      || JSON.stringify(savedModel.model) !== JSON.stringify(model.model);
  });
}

/// 判断模型商是否被编辑。
/// @param provider 当前模型商。
export function IsProviderDirty(provider: ProviderItem) {
  if (provider.savedProvider === null) {
    return true;
  }

  return provider.savedProvider.name !== provider.provider.name
    || provider.savedProvider.api !== provider.provider.api
    || provider.savedProvider.baseUrl !== provider.provider.baseUrl
    || provider.savedProvider.apiKey !== provider.provider.apiKey
    || IsModelListDirty(provider.models, provider.savedModels);
}
