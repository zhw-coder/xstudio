export interface ProviderRecord {
  api: string;
  apiKey: string;
  baseUrl: string;
  name: string;
}

export interface ModelRecord {
  modelId: string;
  modelJson: string;
  providerName: string;
  recordKey: string;
  status: boolean;
}

export interface ProviderModels {
  models: ModelRecord[];
  provider: ProviderRecord;
}

export interface AllProviderModelsOutput {
  apiProviderApis: string[];
  providerModelsMap: Record<string, ProviderModels>;
}

export interface ModelJson {
  api?: unknown;
  compat?: unknown;
  contextWindow?: unknown;
  context_window?: unknown;
  cost?: unknown;
  headers?: unknown;
  id?: unknown;
  input?: unknown;
  maxTokens?: unknown;
  max_tokens?: unknown;
  reasoning?: unknown;
  thinkingLevelMap?: unknown;
  thinking_level_map?: unknown;
  [key: string]: unknown;
}

export interface ProviderItem {
  id: string;
  models: ModelItem[];
  name: string;
  provider: ProviderRecord;
  savedModels: ModelItem[] | null;
  savedProvider: ProviderRecord | null;
  status: 'active' | 'idle';
}

export interface ModelItem {
  enabled: boolean;
  model: ModelJson;
  modelId: string;
  name: string;
  protocol: ModelProtocol;
  providerName: string;
  recordKey: string;
}

export type ModelProtocol = string;
export type ModelInputValue = 'text' | 'image';
export type ModelNestedEditor = 'cost' | 'thinkingLevelMap';
export type JsonObject = Record<string, unknown>;
export type ProviderEditSource = 'header' | 'rail';

export interface ModelParamEntry {
  id: string;
  key: string;
  value: string;
}

export interface ModelParamsDraft {
  compat: string;
  contextWindow: string;
  cost: JsonObject;
  extraInputValues: string[];
  headers: ModelParamEntry[];
  inputValues: ModelInputValue[];
  maxTokens: string;
  reasoning: boolean;
  thinkingLevelMap: JsonObject;
}
