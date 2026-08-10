import { invoke } from '@tauri-apps/api/core';
import type { ChangeEvent, KeyboardEvent } from 'react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { IconGlyph, ScrollArea, SettingsPanelHeader } from '../../components';
import { I18n } from '../../i18n';
import { ReportBackendError } from '../../utils/backendError';

interface SearchEngineConfig {
  enabled: boolean;
  engine: string;
  parameters: unknown | null;
}

interface SearchEngineItem extends SearchEngineConfig {
  domain: string;
}

/// 解析搜索引擎默认参数，确保表单只处理对象。
/// @param parameters 后端返回的搜索引擎参数。
function ParseParameters(parameters: unknown | null) {
  if (typeof parameters === 'object' && parameters !== null && !Array.isArray(parameters)) {
    return parameters as Record<string, unknown>;
  }

  return {};
}

/// 比较两份 JSON 参数。
/// @param left 左侧参数。
/// @param right 右侧参数。
function IsSameParameters(left: Record<string, unknown>, right: Record<string, unknown>) {
  return JSON.stringify(left) === JSON.stringify(right);
}

/// 根据同类型样例创建数组新增项。
/// @param sample 数组中已有的同类型样例。
function GetEmptyArrayValue(sample: unknown) {
  if (typeof sample === 'number') {
    return 0;
  }

  if (typeof sample === 'boolean') {
    return false;
  }

  if (Array.isArray(sample)) {
    return [];
  }

  if (typeof sample === 'object' && sample !== null) {
    return {};
  }

  return '';
}

/// 渲染搜索参数数组编辑区，交互与模型 Headers 键值编辑区一致。
/// @param props.name 参数名称。
/// @param props.onChange 数组变更回调。
/// @param props.values 数组值。
function SearchParameterArrayEditor({
  name,
  onChange,
  values,
}: {
  name: string;
  onChange: (values: unknown[]) => void;
  values: unknown[];
}) {
  const sample = values[0];

  /// 新增数组项。
  function AddValue() {
    onChange([...values, GetEmptyArrayValue(sample)]);
  }

  /// 移除数组项。
  /// @param index 要移除的数组项索引。
  function RemoveValue(index: number) {
    if (values.length <= 1) {
      return;
    }

    onChange(values.filter((_, itemIndex) => itemIndex !== index));
  }

  /// 更新字符串数组项。
  /// @param index 要更新的数组项索引。
  /// @param event 输入事件。
  function ChangeTextValue(index: number, event: ChangeEvent<HTMLInputElement>) {
    onChange(values.map((item, itemIndex) => itemIndex === index ? event.target.value : item));
  }

  /// 更新数值数组项。
  /// @param index 要更新的数组项索引。
  /// @param event 输入事件。
  function ChangeNumberValue(index: number, event: ChangeEvent<HTMLInputElement>) {
    onChange(values.map((item, itemIndex) => itemIndex === index ? Number(event.target.value) : item));
  }

  return (
    <section className="SearchParameterField SearchParameterFieldArray" aria-label={name}>
      <span>{name}</span>
      <div className="SearchArrayEntryList">
        <div className="ScrollArea SearchArrayEntryScroll">
          {values.map((value, index) => (
            <div className="SearchArrayEntryRow" key={`${name}:${index}`}>
              {typeof value === 'number' ? (
                <input
                  aria-label={I18n.settings.searchParameterValueAria.replace('{name}', name)}
                  className="ModelParamInput"
                  onChange={(event) => ChangeNumberValue(index, event)}
                  type="number"
                  value={value}
                />
              ) : (
                <input
                  aria-label={I18n.settings.searchParameterValueAria.replace('{name}', name)}
                  className="ModelParamInput"
                  onChange={(event) => ChangeTextValue(index, event)}
                  spellCheck={false}
                  type="text"
                  value={typeof value === 'string' ? value : JSON.stringify(value)}
                />
              )}
              <button
                aria-label={I18n.settings.searchParameterRemoveAria.replace('{name}', name)}
                className="SearchArrayEntryButton"
                disabled={values.length <= 1}
                onClick={() => RemoveValue(index)}
                type="button"
              >
                <IconGlyph name="minus" size={14} />
              </button>
            </div>
          ))}
        </div>
        <button aria-label={I18n.settings.searchParameterAddAria.replace('{name}', name)} className="SearchArrayEntryButton" onClick={AddValue} type="button">
          <IconGlyph name="plus" size={14} />
        </button>
      </div>
    </section>
  );
}

/// 渲染搜索引擎参数输入项。
/// @param props.name 参数名称。
/// @param props.value 参数值。
/// @param props.onChange 参数值变更回调。
function SearchParameterField({
  name,
  onChange,
  value,
}: {
  name: string;
  onChange: (value: unknown) => void;
  value: unknown;
}) {
  const label = name;

  /// 处理布尔参数变更。
  /// @param event 输入事件。
  function HandleBooleanChange(event: ChangeEvent<HTMLInputElement>) {
    onChange(event.target.checked);
  }

  /// 处理数字参数变更。
  /// @param event 输入事件。
  function HandleNumberChange(event: ChangeEvent<HTMLInputElement>) {
    onChange(event.target.value === '' ? 0 : Number(event.target.value));
  }

  /// 处理字符串参数变更。
  /// @param event 输入事件。
  function HandleTextChange(event: ChangeEvent<HTMLInputElement>) {
    onChange(event.target.value);
  }

  if (typeof value === 'boolean') {
    return (
      <label className="SearchParameterField">
        <span>{label}</span>
        <input aria-label={label} checked={value} onChange={HandleBooleanChange} type="checkbox" />
      </label>
    );
  }

  if (typeof value === 'number') {
    return (
      <label className="SearchParameterField">
        <span>{label}</span>
        <input aria-label={label} className="ModelParamInput" onChange={HandleNumberChange} type="number" value={value} />
      </label>
    );
  }

  if (Array.isArray(value)) {
    return (
      <SearchParameterArrayEditor name={label} values={value} onChange={onChange} />
    );
  }

  return (
    <label className="SearchParameterField">
      <span>{label}</span>
      <input
        aria-label={label}
        className="ModelParamInput"
        onChange={HandleTextChange}
        spellCheck={false}
        type="text"
        value={String(value ?? '')}
      />
    </label>
  );
}

/// 渲染搜索设置面板。
function SearchSettingsPanel() {
  const [engines, setEngines] = useState<SearchEngineItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState('');
  const [selectedEngineName, setSelectedEngineName] = useState<string | null>(null);
  const [parametersDraft, setParametersDraft] = useState<Record<string, unknown>>({});
  const [savedEngineEnabledByName, setSavedEngineEnabledByName] = useState<Record<string, boolean>>({});
  const [searchListHeight, setSearchListHeight] = useState<number | null>(null);
  const searchSettingsContentRef = useRef<HTMLDivElement>(null);
  const selectedEngine = engines.find((engine) => engine.engine === selectedEngineName) ?? null;
  const parametersDirty = selectedEngine !== null && !IsSameParameters(parametersDraft, ParseParameters(selectedEngine.parameters));
  const enabledDirty = engines.some((engine) => engine.enabled !== (savedEngineEnabledByName[engine.engine] ?? false));
  const enginesByDomain = useMemo(() => engines.reduce<Record<string, SearchEngineItem[]>>((groups, engine) => {
    (groups[engine.domain] ??= []).push(engine);
    return groups;
  }, {}), [engines]);

  /// 加载工具库支持的搜索引擎及已保存配置。
  async function LoadSearchEngines() {
    setLoading(true);
    setSaveError('');

    try {
      const [engineGroups, configs] = await Promise.all([
        invoke<string[][]>('list_search_engines'),
        invoke<SearchEngineConfig[]>('list_search_configs'),
      ]);
      const configMap = new Map(configs.map((config) => [config.engine, config]));
      const nextEngines = engineGroups.flatMap(([domain, ...engineNames]) => engineNames.map((engine) => ({
        domain,
        enabled: configMap.get(engine)?.enabled ?? false,
        engine,
        parameters: configMap.get(engine)?.parameters ?? null,
      })));

      setEngines(nextEngines);
      setSavedEngineEnabledByName(nextEngines.reduce<Record<string, boolean>>((output, engine) => {
        output[engine.engine] = engine.enabled;
        return output;
      }, {}));
      setSelectedEngineName((current) => nextEngines.some((engine) => engine.engine === current) ? current : null);
    } catch (error) {
      setSaveError(ReportBackendError('加载搜索引擎配置失败', error));
    } finally {
      setLoading(false);
    }
  }

  /// 获取选中搜索引擎的已保存或默认参数。
  /// @param engine 要选择的搜索引擎。
  async function SelectEngine(engine: SearchEngineItem) {
    setSelectedEngineName(engine.engine);
    setSaveError('');

    try {
      const config = engine.parameters === null
        ? await invoke<SearchEngineConfig>('get_search_engine', { engine: engine.engine })
        : engine;
      const nextParameters = ParseParameters(config.parameters);

      setParametersDraft(nextParameters);
      setEngines((items) => items.map((item) => item.engine === engine.engine
        ? { ...item, parameters: config.parameters }
        : item));
    } catch (error) {
      setSaveError(ReportBackendError(`加载搜索引擎参数失败: ${engine.engine}`, error));
    }
  }

  /// 保存指定搜索引擎配置。
  /// @param engine 要保存的搜索引擎。
  /// @param enabled 目标启用状态。
  /// @param parameters 目标参数。
  async function SaveEngine(engine: SearchEngineItem, enabled: boolean, parameters: unknown | null) {
    const resolved = parameters === null
      ? await invoke<SearchEngineConfig>('get_search_engine', { engine: engine.engine })
      : { ...engine, parameters };

    return invoke<SearchEngineConfig>('save_search_config', {
      input: { enabled, engine: engine.engine, parameters: resolved.parameters },
    });
  }

  /// 按领域切换唯一启用的搜索引擎草稿。
  /// @param engine 要启用的搜索引擎。
  function ToggleEngineEnabled(engine: SearchEngineItem) {
    if (saving) {
      return;
    }

    setSaveError('');
    setEngines((items) => items.map((item) => {
      if (item.domain !== engine.domain) {
        return item;
      }

      if (item.engine === engine.engine) {
        return { ...item, enabled: !item.enabled };
      }

      return item.enabled ? { ...item, enabled: false } : item;
    }));
  }

  /// 保存搜索引擎启用状态和当前参数草稿。
  async function SaveChanges() {
    if ((!enabledDirty && !parametersDirty) || saving) {
      return;
    }

    setSaving(true);
    setSaveError('');

    try {
      const enginesToSave = engines.filter((engine) => (
        engine.enabled !== (savedEngineEnabledByName[engine.engine] ?? false)
        || (engine.engine === selectedEngineName && parametersDirty)
      ));
      const savedConfigs = await Promise.all(enginesToSave.map(async (engine) => SaveEngine(
        engine,
        engine.enabled,
        engine.engine === selectedEngineName ? parametersDraft : engine.parameters,
      )));
      const savedMap = new Map(savedConfigs.map((config) => [config.engine, config]));

      setEngines((items) => items.map((item) => savedMap.has(item.engine)
        ? { ...item, ...savedMap.get(item.engine)! }
        : item));
      setSavedEngineEnabledByName((current) => savedConfigs.reduce<Record<string, boolean>>((output, config) => {
        output[config.engine] = config.enabled;
        return output;
      }, { ...current }));
    } catch (error) {
      setSaveError(ReportBackendError('保存搜索引擎配置失败', error));
    } finally {
      setSaving(false);
    }
  }

  /// 处理搜索引擎行键盘选择。
  /// @param event 键盘事件。
  /// @param engine 要选择的搜索引擎。
  function HandleEngineRowKeyDown(event: KeyboardEvent<HTMLDivElement>, engine: SearchEngineItem) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      void SelectEngine(engine);
    }
  }

  useEffect(() => {
    void LoadSearchEngines();
  }, []);

  useEffect(() => {
    const content = searchSettingsContentRef.current;

    if (content === null) {
      return;
    }

    /// 更新搜索列表区域高度，确保为可用高度的整除三分之二。
    function UpdateSearchListHeight() {
      const contentHeight = searchSettingsContentRef.current?.clientHeight ?? 0;

      setSearchListHeight(Math.max(0, Math.floor(((contentHeight - 14) * 2) / 3)));
    }

    const observer = new ResizeObserver(UpdateSearchListHeight);

    UpdateSearchListHeight();
    observer.observe(content);

    return () => {
      observer.disconnect();
    };
  }, []);

  return (
    <section aria-busy={loading || saving} className="SearchSettingsPanel" aria-labelledby="search-settings-title">
      <SettingsPanelHeader
        description={I18n.settings.searchDescription}
        saveDisabled={(!enabledDirty && !parametersDirty) || saving}
        saveError={saveError}
        title={I18n.settings.searchTitle}
        titleId="search-settings-title"
        onSave={() => void SaveChanges()}
      />

      <div className="SearchSettingsContent" ref={searchSettingsContentRef}>
        <section className="SearchParametersPanel" aria-labelledby="search-parameters-title">
          {selectedEngine === null ? (
            <span className="SearchParametersEmpty">{loading ? I18n.common.loading : I18n.settings.searchSelectEngine}</span>
          ) : (
            <>
              <h2 id="search-parameters-title">{selectedEngine.engine}</h2>
              <div className="SearchParametersForm">
                {Object.entries(parametersDraft).map(([name, value]) => (
                  <SearchParameterField
                    key={name}
                    name={name}
                    value={value}
                    onChange={(nextValue) => setParametersDraft((parameters) => ({ ...parameters, [name]: nextValue }))}
                  />
                ))}
              </div>
            </>
          )}
        </section>

        <section
          className="SearchEngineListSection"
          aria-labelledby="search-engine-list-title"
          style={searchListHeight === null ? undefined : { flexBasis: `${searchListHeight}px`, height: `${searchListHeight}px` }}
        >
          <div className="SearchEngineList" role="table">
            <div className="SearchEngineListHeader" role="row">
              <span id="search-engine-list-title" role="columnheader">{I18n.settings.searchEngineName}</span>
              <span role="columnheader">{I18n.settings.searchDomain}</span>
              <span role="columnheader">{I18n.settings.searchEnabled}</span>
            </div>
            <div className="SearchEngineRowsWrap" role="rowgroup">
              <ScrollArea ariaLabel={I18n.settings.searchEngineListAria} className="SearchEngineRows">
                {Object.entries(enginesByDomain).flatMap(([domain, items]) => items.map((engine) => (
                <div
                  aria-selected={selectedEngineName === engine.engine}
                  className={selectedEngineName === engine.engine ? 'SearchEngineRow SearchEngineRowSelected' : 'SearchEngineRow'}
                  key={engine.engine}
                  onClick={() => void SelectEngine(engine)}
                  onKeyDown={(event) => HandleEngineRowKeyDown(event, engine)}
                  role="row"
                  tabIndex={0}
                >
                  <span role="cell">{engine.engine}</span>
                  <span role="cell">{domain}</span>
                  <span className="SearchEngineEnabledCell" role="cell">
                    <input
                      aria-label={`${I18n.settings.searchEnabled} ${engine.engine}`}
                      checked={engine.enabled}
                      disabled={saving}
                      onChange={() => void ToggleEngineEnabled(engine)}
                      onClick={(event) => event.stopPropagation()}
                      type="checkbox"
                    />
                  </span>
                </div>
                )))}
              </ScrollArea>
            </div>
          </div>
        </section>
      </div>
    </section>
  );
}

export default SearchSettingsPanel;
