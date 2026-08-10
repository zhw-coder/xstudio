import { invoke } from '@tauri-apps/api/core';
import type { ChangeEvent } from 'react';
import { useEffect, useState } from 'react';
import { IconGlyph } from '../../components';
import { I18n } from '../../i18n';
import type { JsonObject, ModelParamEntry } from './types';
import {
  FormatJsonParamEntryValue,
  ParseParamEntryValue,
  ReportModelSettingsBackendError,
} from './utils';

/// 渲染模型参数键值行。
/// @param props.entry 键值行。
/// @param props.keyPlaceholder key 占位提示。
/// @param props.onChange 更新行回调。
/// @param props.onRemove 删除行回调。
/// @param props.valuePlaceholder value 占位提示。
function ParamEntryRow({
  entry,
  keyPlaceholder,
  onChange,
  onRemove,
  valuePlaceholder,
}: {
  entry: ModelParamEntry;
  keyPlaceholder: string;
  onChange: (entry: ModelParamEntry) => void;
  onRemove: () => void;
  valuePlaceholder: string;
}) {
  /// 更新键名。
  /// @param event 输入事件。
  function HandleKeyChange(event: ChangeEvent<HTMLInputElement>) {
    onChange({ ...entry, key: event.target.value });
  }

  /// 更新键值。
  /// @param event 输入事件。
  function HandleValueChange(event: ChangeEvent<HTMLInputElement>) {
    onChange({ ...entry, value: event.target.value });
  }

  return (
    <div className="ModelParamEntryRow">
      <input
        aria-label={I18n.modelSettings.parameterKeyAria}
        className="ModelParamInput"
        onChange={HandleKeyChange}
        placeholder={keyPlaceholder}
        spellCheck={false}
        value={entry.key}
      />
      <input
        aria-label={I18n.modelSettings.parameterValueAria}
        className="ModelParamInput"
        onChange={HandleValueChange}
        placeholder={valuePlaceholder}
        spellCheck={false}
        value={entry.value}
      />
      <button aria-label={I18n.modelSettings.removeParameterAria} className="ModelParamIconButton" onClick={onRemove} type="button">
        <IconGlyph name="minus" size={14} />
      </button>
    </div>
  );
}

/// 渲染模型参数键值编辑区。
/// @param props.entries 键值行。
/// @param props.keyPlaceholder key 占位提示。
/// @param props.onChange 更新键值行列表回调。
/// @param props.valuePlaceholder value 占位提示。
export function ModelParamEntryList({
  entries,
  keyPlaceholder,
  onChange,
  valuePlaceholder,
}: {
  entries: ModelParamEntry[];
  keyPlaceholder: string;
  onChange: (entries: ModelParamEntry[]) => void;
  valuePlaceholder: string;
}) {
  /// 新增键值行。
  function AddEntry() {
    onChange([...entries, { id: `entry:${Date.now()}:${entries.length}`, key: '', value: '' }]);
  }

  /// 更新指定键值行。
  /// @param targetEntry 目标行。
  function UpdateEntry(targetEntry: ModelParamEntry) {
    onChange(entries.map((entry) => (entry.id === targetEntry.id ? targetEntry : entry)));
  }

  /// 移除指定键值行。
  /// @param entryId 行 ID。
  function RemoveEntry(entryId: string) {
    onChange(entries.filter((entry) => entry.id !== entryId));
  }

  return (
    <div className="ModelParamEntryList">
      <div className="ModelParamEntryScroll">
        {entries.length > 0 ? (
          entries.map((entry) => (
            <ParamEntryRow
              entry={entry}
              key={entry.id}
              keyPlaceholder={keyPlaceholder}
              valuePlaceholder={valuePlaceholder}
              onChange={UpdateEntry}
              onRemove={() => RemoveEntry(entry.id)}
            />
          ))
        ) : (
          <span className="ModelParamEmptyText">{I18n.modelSettings.noConfig}</span>
        )}
      </div>
      <button aria-label={I18n.modelSettings.addParameterAria} className="ModelParamIconButton" onClick={AddEntry} type="button">
        <IconGlyph name="plus" size={14} />
      </button>
    </div>
  );
}

/// 渲染思考档位映射配置三级界面。
/// @param props.initialValue 当前映射对象。
/// @param props.onCancel 取消回调。
/// @param props.onConfirm 确认回调。
export function ThinkingLevelMapEditor({
  initialValue,
  onCancel,
  onConfirm,
}: {
  initialValue: JsonObject;
  onCancel: () => void;
  onConfirm: (value: JsonObject) => void;
}) {
  const [levels, setLevels] = useState<string[]>([]);
  const [levelValues, setLevelValues] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');

  useEffect(() => {
    let cancelled = false;

    /// 加载后端思考档位列表。
    async function LoadThinkingLevels() {
      setLoading(true);
      setErrorMessage('');

      try {
        const nextLevels = await invoke<string[]>('model_thinking_levels');

        if (cancelled) {
          return;
        }

        const nextLevelValues = nextLevels.reduce<Record<string, string>>((output, level) => {
          const value = initialValue[level];

          output[level] = typeof value === 'string' ? value : '';
          return output;
        }, {});

        setLevels(nextLevels);
        setLevelValues(nextLevelValues);
      } catch (error) {
        const message = ReportModelSettingsBackendError('加载思考档位失败', error);

        if (!cancelled) {
          setErrorMessage(message);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void LoadThinkingLevels();

    return () => {
      cancelled = true;
    };
  }, [initialValue]);

  /// 更新思考档位映射值。
  /// @param level 思考档位。
  /// @param value 映射值。
  function ChangeLevelValue(level: string, value: string) {
    setLevelValues((currentValues) => ({
      ...currentValues,
      [level]: value,
    }));
  }

  /// 确认思考档位映射。
  function ConfirmThinkingLevels() {
    const nextValue = levels.reduce<JsonObject>((output, level) => {
      const value = levelValues[level]?.trim() ?? '';

      if (value.length > 0) {
        output[level] = value;
      }

      return output;
    }, {});

    onConfirm(nextValue);
  }

  return (
    <section className="ModelParamEditor" aria-labelledby="thinking-level-editor-title">
      <div className="ModelParamEditorHeader">
        <h3 id="thinking-level-editor-title">{I18n.modelSettings.thinkingLevelTitle}</h3>
        <span>{loading ? I18n.common.loading : `${levels.length} ${I18n.common.itemUnit}`}</span>
      </div>
      {errorMessage.length > 0 ? <span className="ModelParamError" role="alert">{errorMessage}</span> : null}
      <div className="ModelParamLevelList">
        {levels.map((level) => (
          <label className="ModelParamLevelRow" key={level}>
            <span>{level}</span>
            <input
              className="ModelParamInput"
              onChange={(event) => ChangeLevelValue(level, event.target.value)}
              placeholder={I18n.modelSettings.mappingValuePlaceholder}
              spellCheck={false}
              value={levelValues[level] ?? ''}
            />
          </label>
        ))}
      </div>
      <div className="ModelParamActions">
        <button className="ModelParamSecondaryButton" onClick={onCancel} type="button">{I18n.common.cancel}</button>
        <button className="ModelParamPrimaryButton" onClick={ConfirmThinkingLevels} type="button">{I18n.common.confirm}</button>
      </div>
    </section>
  );
}

type CostFieldKey = 'input' | 'output' | 'cacheRead' | 'cacheWrite';

/// 计费编辑器固定字段，顺序与界面展示一致。
const CostFieldKeys: CostFieldKey[] = ['input', 'output', 'cacheRead', 'cacheWrite'];

/// 计费字段的 snake_case 兼容 key，用于回填旧数据或手写配置。
const CostSnakeCaseKeys: Record<CostFieldKey, string> = {
  cacheRead: 'cache_read',
  cacheWrite: 'cache_write',
  input: 'input',
  output: 'output',
};

/// 读取计费字段初始输入值。
/// @param source 当前计费对象。
/// @param key 计费字段 key。
function GetCostFieldInputValue(source: JsonObject, key: CostFieldKey) {
  const snakeKey = CostSnakeCaseKeys[key];
  const snakeValue = source[snakeKey];
  const value = snakeKey !== key && snakeValue !== undefined ? snakeValue : source[key];

  return value === undefined ? '0' : FormatJsonParamEntryValue(value);
}

/// 构造计费字段输入对象。
/// @param source 当前计费对象。
function BuildCostFieldInputs(source: JsonObject) {
  return CostFieldKeys.reduce<Record<CostFieldKey, string>>((output, key) => {
    output[key] = GetCostFieldInputValue(source, key);
    return output;
  }, {
    cacheRead: '0',
    cacheWrite: '0',
    input: '0',
    output: '0',
  });
}

/// 渲染计费字段行。
/// @param props.fieldKey 计费字段 key。
/// @param props.label 字段标签。
/// @param props.onChange 更新字段值回调。
/// @param props.value 字段输入值。
function CostFieldRow({
  fieldKey,
  label,
  onChange,
  value,
}: {
  fieldKey: CostFieldKey;
  label: string;
  onChange: (fieldKey: CostFieldKey, value: string) => void;
  value: string;
}) {
  /// 更新计费字段输入。
  /// @param event 输入事件。
  function HandleCostFieldChange(event: ChangeEvent<HTMLInputElement>) {
    onChange(fieldKey, event.target.value);
  }

  return (
    <label className="ModelParamCostRow">
      <span className="ModelParamCostKey" title={label}>{label}</span>
      <input
        aria-label={label}
        className="ModelParamInput"
        inputMode="decimal"
        onChange={HandleCostFieldChange}
        placeholder="0"
        spellCheck={false}
        value={value}
      />
    </label>
  );
}

/// 渲染计费配置三级界面。
/// @param props.initialValue 当前对象。
/// @param props.onCancel 取消回调。
/// @param props.onConfirm 确认回调。
/// @param props.title 标题。
export function CostMapEditor({
  initialValue,
  onCancel,
  onConfirm,
  title,
}: {
  initialValue: JsonObject;
  onCancel: () => void;
  onConfirm: (value: JsonObject) => void;
  title: string;
}) {
  const costLabels = I18n.modelSettings.modelCostLabels;
  const costFieldLabels: Record<CostFieldKey, string> = {
    cacheRead: costLabels.cacheRead,
    cacheWrite: costLabels.cacheWrite,
    input: costLabels.input,
    output: costLabels.output,
  };
  const [fieldValues, setFieldValues] = useState<Record<CostFieldKey, string>>(() => BuildCostFieldInputs(initialValue));

  /// 更新计费字段输入。
  /// @param fieldKey 计费字段 key。
  /// @param value 字段输入值。
  function ChangeCostField(fieldKey: CostFieldKey, value: string) {
    setFieldValues((currentValues) => ({
      ...currentValues,
      [fieldKey]: value,
    }));
  }

  /// 确认对象配置。
  function ConfirmCostMap() {
    const nextValue = CostFieldKeys.reduce<JsonObject>((output, key) => {
      const value = ParseParamEntryValue(fieldValues[key]);

      if (value !== undefined) {
        output[key] = value;
      }

      return output;
    }, {});

    onConfirm(nextValue);
  }

  return (
    <section className="ModelParamEditor" aria-labelledby="cost-map-editor-title">
      <div className="ModelParamEditorHeader">
        <h3 id="cost-map-editor-title">{title}</h3>
        <span>{CostFieldKeys.length} {I18n.common.itemUnit}</span>
      </div>
      <div className="ModelParamCostList">
        {CostFieldKeys.map((fieldKey) => (
          <CostFieldRow
            fieldKey={fieldKey}
            key={fieldKey}
            label={costFieldLabels[fieldKey]}
            value={fieldValues[fieldKey]}
            onChange={ChangeCostField}
          />
        ))}
      </div>
      <div className="ModelParamActions">
        <button className="ModelParamSecondaryButton" onClick={onCancel} type="button">{I18n.common.cancel}</button>
        <button className="ModelParamPrimaryButton" onClick={ConfirmCostMap} type="button">{I18n.common.confirm}</button>
      </div>
    </section>
  );
}
