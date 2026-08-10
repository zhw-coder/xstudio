import type { IconName, SelectFieldItem, SelectFieldOption } from '../../components';
import type { ThreadModelOptions, ThreadModelSelection } from './types';

/// 底部 API 下拉默认值。
export const DefaultThreadThinkingLevelValue = 'high';

/// 空模型选择值。
export const EmptyThreadModelValue = '';

/// Provider 和模型 ID 组合值分隔符。
const ThreadModelValueSeparator = '::';

/// 思考档位图标颜色变量。
const ThreadThinkingLevelIconColors = {
  high: 'var(--thinking-level-high)',
  low: 'var(--thinking-level-low)',
  medium: 'var(--thinking-level-medium)',
  minimal: 'var(--thinking-level-minimal)',
  off: 'var(--thinking-level-off)',
  xhigh: 'var(--thinking-level-xhigh)',
} as const;

/// 思考档位统一使用 high 档位图标。
const ThreadThinkingLevelIcon: IconName = 'brain';

interface ThreadThinkingLevelIconConfig {
  icon: IconName;
  iconColor: string;
}

/// 获取思考档位图标配置。
/// @param level 思考档位。
function ResolveThreadThinkingLevelIconConfig(level: string): ThreadThinkingLevelIconConfig {
  const normalizedLevel = level.trim().toLowerCase();

  if (normalizedLevel === 'off' || normalizedLevel === 'none') {
    return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.off };
  }

  if (normalizedLevel === 'minimal') {
    return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.minimal };
  }

  if (normalizedLevel === 'low') {
    return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.low };
  }

  if (normalizedLevel === 'medium' || normalizedLevel === 'normal') {
    return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.medium };
  }

  if (normalizedLevel === 'high') {
    return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.high };
  }

  if (normalizedLevel === 'xhigh' || normalizedLevel === 'extra-high' || normalizedLevel === 'max') {
    return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.xhigh };
  }

  return { icon: ThreadThinkingLevelIcon, iconColor: ThreadThinkingLevelIconColors.high };
}

/// 构造模型下拉框的唯一值。
/// @param providerName Provider 名称。
/// @param modelId 模型 ID。
export function BuildThreadModelValue(providerName: string, modelId: string) {
  return `${encodeURIComponent(providerName)}${ThreadModelValueSeparator}${encodeURIComponent(modelId)}`;
}

/// 解析模型下拉框的唯一值。
/// @param modelKey Provider 和模型 ID 组合值。
export function ParseThreadModelValue(modelKey: string) {
  const [providerName, modelId, ...rest] = modelKey.split(ThreadModelValueSeparator);

  if (!providerName || !modelId || rest.length > 0) {
    return null;
  }

  return {
    providerName: decodeURIComponent(providerName),
    modelId: decodeURIComponent(modelId),
  };
}

/// 获取当前模型的最大上下文 token 数。
/// @param providerModelTokensMap Provider:ModelId 到最大 token 数的映射。
/// @param modelKey 当前模型组合值。
export function GetThreadModelTokenLimit(providerModelTokensMap: Record<string, number>, modelKey: string) {
  const model = ParseThreadModelValue(modelKey);

  if (model === null) {
    return null;
  }

  const tokenLimit = providerModelTokensMap[`${model.providerName}:${model.modelId}`];

  return typeof tokenLimit === 'number' && Number.isFinite(tokenLimit) && tokenLimit > 0 ? tokenLimit : null;
}

/// 构造模型分组下拉选项。
/// @param providerModelIdsMap Provider 名称到模型 ID 列表的映射。
export function BuildThreadModelSelectItems(providerModelIdsMap: Record<string, string[]>): SelectFieldItem<string>[] {
  const items: SelectFieldItem<string>[] = [];

  Object.entries(providerModelIdsMap)
    .sort(([leftName], [rightName]) => leftName.localeCompare(rightName))
    .forEach(([providerName, modelIds]) => {
      const validModelIds = modelIds.filter((modelId) => modelId.length > 0);

      if (validModelIds.length === 0) {
        return;
      }

      items.push({ label: providerName, type: 'group' });
      validModelIds.forEach((modelId) => {
        items.push({
          label: modelId,
          value: BuildThreadModelValue(providerName, modelId),
        });
      });
    });

  return items;
}

/// 判断下拉项是否为可选择项。
/// @param item 下拉项。
function IsThreadSelectOption(item: SelectFieldItem<string>): item is SelectFieldOption<string> {
  return item.type !== 'group';
}

/// 获取第一条可用模型组合值。
/// @param providerModelIdsMap Provider 名称到模型 ID 列表的映射。
export function GetFirstThreadModelValue(providerModelIdsMap: Record<string, string[]>) {
  const firstOption = BuildThreadModelSelectItems(providerModelIdsMap).find(IsThreadSelectOption);

  return firstOption?.value ?? EmptyThreadModelValue;
}

/// 判断模型组合值是否仍存在于当前模型数据中。
/// @param providerModelIdsMap Provider 名称到模型 ID 列表的映射。
/// @param modelKey 当前模型组合值。
export function HasThreadModelValue(providerModelIdsMap: Record<string, string[]>, modelKey: string) {
  return BuildThreadModelSelectItems(providerModelIdsMap).some(
    (item) => IsThreadSelectOption(item) && item.value === modelKey
  );
}

/// 解析当前模型下拉框实际展示值。
/// @param providerModelIdsMap Provider 名称到模型 ID 列表的映射。
/// @param modelKey 当前模型组合值。
export function ResolveThreadModelValue(providerModelIdsMap: Record<string, string[]>, modelKey: string) {
  if (HasThreadModelValue(providerModelIdsMap, modelKey)) {
    return modelKey;
  }

  return GetFirstThreadModelValue(providerModelIdsMap);
}

/// 获取思考档位下拉框选中值。
/// @param modelThinkingLevels 后端返回的模型思考档位列表。
/// @param currentThinkingLevel 当前模型思考档位。
export function ResolveThreadThinkingLevelValue(modelThinkingLevels: string[], currentThinkingLevel: string) {
  if (modelThinkingLevels.length === 0) {
    return currentThinkingLevel || DefaultThreadThinkingLevelValue;
  }

  if (modelThinkingLevels.includes(currentThinkingLevel)) {
    return currentThinkingLevel;
  }

  return DefaultThreadThinkingLevelValue;
}

/// 构造思考档位下拉选项。
/// @param modelThinkingLevels 后端返回的模型思考档位列表。
export function BuildThreadThinkingLevelSelectOptions(modelThinkingLevels: string[]): SelectFieldOption<string>[] {
  const levels = modelThinkingLevels.length > 0 ? modelThinkingLevels : [DefaultThreadThinkingLevelValue];

  return levels.map((level) => {
    const iconConfig = ResolveThreadThinkingLevelIconConfig(level);

    return {
      ...iconConfig,
      label: level,
      value: level,
    };
  });
}

/// 归一化线程模型选择状态。
/// @param options 后端返回的线程模型选项。
/// @param selection 当前线程模型选择状态。
export function NormalizeThreadModelSelection(
  options: ThreadModelOptions,
  selection: ThreadModelSelection
): ThreadModelSelection {
  return {
    thinkingLevel: ResolveThreadThinkingLevelValue(options.modelThinkingLevels, selection.thinkingLevel),
    modelKey: ResolveThreadModelValue(options.providerModelIdsMap, selection.modelKey),
  };
}
