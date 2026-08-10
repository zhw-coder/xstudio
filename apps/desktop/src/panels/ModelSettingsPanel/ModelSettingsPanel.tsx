import { invoke } from '@tauri-apps/api/core';
import type { ChangeEvent, KeyboardEvent, MouseEvent } from 'react';
import { useEffect, useRef, useState } from 'react';
import { IconGlyph, ScrollArea, SelectField, SettingsPanelHeader } from '../../components';
import type { SelectFieldOption } from '../../components';
import { I18n } from '../../i18n';
import { GetDefaultProviderName, NewProviderIdPrefix } from './constants';
import { ConfigField, ConfigSelectField, ModelToggle } from './ModelControls';
import ModelParamsEditor from './ModelParamsEditor';
import type {
  AllProviderModelsOutput,
  ModelItem,
  ModelProtocol,
  ModelRecord,
  ProviderEditSource,
  ProviderItem,
  ProviderRecord,
} from './types';
import {
  BuildApiSelectOptions,
  BuildModelItem,
  BuildModelRecord,
  BuildProviderItems,
  BuildUniqueProviderName,
  GetDefaultApi,
  GetProviderStatus,
  IsProviderDirty,
  ReportModelSettingsBackendError,
} from './utils';

interface ModelSettingsPanelProps {
  /// 模型配置保存或删除成功后的通知回调。
  onModelsChange: () => void;
}

/// 渲染模型商卡片。
/// @param props.active 当前模型商是否选中。
/// @param props.draftName 编辑中的模型商名称。
/// @param props.editing 当前模型商行是否正在编辑。
/// @param props.onCancelEdit 取消编辑回调。
/// @param props.onChangeDraftName 更新编辑草稿回调。
/// @param props.onCommitEdit 提交编辑回调。
/// @param props.provider 模型商数据。
/// @param props.onSelect 选中当前模型商回调。
/// @param props.onStartEdit 开始编辑当前模型商回调。
function ProviderCard({
  active,
  draftName,
  editing,
  onCancelEdit,
  onChangeDraftName,
  onCommitEdit,
  onSelect,
  onStartEdit,
  provider,
}: {
  active: boolean;
  draftName: string;
  editing: boolean;
  onCancelEdit: () => void;
  onChangeDraftName: (name: string) => void;
  onCommitEdit: () => void;
  onSelect: () => void;
  onStartEdit: () => void;
  provider: ProviderItem;
}) {
  const className = [
    'ModelProviderCard',
    active ? 'ModelProviderCardActive' : '',
    editing ? 'ModelProviderCardEditing' : '',
  ].filter(Boolean).join(' ');

  /// 处理模型商行名称输入。
  /// @param event 输入事件。
  function HandleProviderRowNameChange(event: ChangeEvent<HTMLInputElement>) {
    onChangeDraftName(event.target.value);
  }

  /// 处理模型商行名称输入快捷键。
  /// @param event 键盘事件。
  function HandleProviderRowNameKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Enter') {
      event.preventDefault();
      onCommitEdit();
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      onCancelEdit();
    }
  }

  if (editing) {
    return (
      <div aria-current={active ? 'page' : undefined} className={className}>
        <input
          aria-label={I18n.modelSettings.providerNameAria.replace('{name}', provider.name)}
          autoFocus
          className="ModelProviderCardNameInput"
          onBlur={onCommitEdit}
          onChange={HandleProviderRowNameChange}
          onKeyDown={HandleProviderRowNameKeyDown}
          value={draftName}
        />
        <span aria-label={provider.status} className={`ModelProviderStatus ModelProviderStatus-${provider.status}`} />
      </div>
    );
  }

  return (
    <button
      aria-current={active ? 'page' : undefined}
      className={className}
      onClick={onSelect}
      onDoubleClick={onStartEdit}
      type="button"
    >
      <span className="ModelProviderName">{provider.name}</span>
      <span aria-label={provider.status} className={`ModelProviderStatus ModelProviderStatus-${provider.status}`} />
    </button>
  );
}

/// 渲染左侧模型商区域。
/// @param props.activeProviderId 当前选中模型商 ID。
/// @param props.deleting 当前是否正在删除。
/// @param props.draftName 编辑中的模型商名称。
/// @param props.editingProviderId 当前编辑中的模型商 ID。
/// @param props.editingProviderSource 当前模型商名称编辑入口。
/// @param props.onAddProvider 新增模型商回调。
/// @param props.onCancelEdit 取消编辑回调。
/// @param props.onChangeDraftName 更新编辑草稿回调。
/// @param props.onClearProviderSelection 清理模型商选中态回调。
/// @param props.onCommitEdit 提交编辑回调。
/// @param props.onDeleteProvider 删除模型商回调。
/// @param props.onSelectProvider 选中模型商回调。
/// @param props.onStartProviderEdit 开始编辑模型商名称回调。
/// @param props.providers 模型商列表。
function ProviderRail({
  activeProviderId,
  deleting,
  draftName,
  editingProviderId,
  editingProviderSource,
  onAddProvider,
  onCancelEdit,
  onChangeDraftName,
  onClearProviderSelection,
  onCommitEdit,
  onDeleteProvider,
  onSelectProvider,
  onStartProviderEdit,
  providers,
}: {
  activeProviderId: string | null;
  deleting: boolean;
  draftName: string;
  editingProviderId: string | null;
  editingProviderSource: ProviderEditSource | null;
  onAddProvider: () => void;
  onCancelEdit: () => void;
  onChangeDraftName: (name: string) => void;
  onClearProviderSelection: () => void;
  onCommitEdit: () => void;
  onDeleteProvider: () => void;
  onSelectProvider: (providerId: string) => void;
  onStartProviderEdit: (providerId: string, source: ProviderEditSource) => void;
  providers: ProviderItem[];
}) {
  const deleteDisabled = activeProviderId === null || deleting;

  /// 处理模型商区域空白点击。
  /// @param event 鼠标事件。
  function HandleProviderRailClick(event: MouseEvent<HTMLElement>) {
    const target = event.target;

    if (
      !(target instanceof Element)
      || target.closest('.ModelProviderCard') !== null
      || target.closest('.ModelProviderActions') !== null
    ) {
      return;
    }

    onClearProviderSelection();
  }

  return (
    <aside className="ModelProviderRail" aria-labelledby="provider-heading" onClick={HandleProviderRailClick}>
      <div className="ModelProviderTop">
        <h2 id="provider-heading">{I18n.modelSettings.providerRailTitle}</h2>
        <p>{I18n.modelSettings.providerRailDescription}</p>
      </div>

      <div className="ModelProviderActions">
        <button className="ModelProviderAddButton" onClick={onAddProvider} type="button">
          <IconGlyph name="plus" size={16} />
          <span>{I18n.modelSettings.addProvider}</span>
        </button>
        <button
          aria-disabled={deleteDisabled}
          aria-label={I18n.modelSettings.deleteProviderAria}
          className="ModelProviderDeleteButton"
          disabled={deleteDisabled}
          onClick={onDeleteProvider}
          type="button"
        >
          <IconGlyph name="trash-2" size={16} />
        </button>
      </div>

      <div className="ModelProviderListWrap">
        <ScrollArea ariaLabel="Model provider list" className="ModelProviderList">
          {providers.map((provider) => (
            <ProviderCard
              active={provider.id === activeProviderId}
              draftName={draftName}
              editing={provider.id === editingProviderId && editingProviderSource === 'rail'}
              key={provider.id}
              provider={provider}
              onCancelEdit={onCancelEdit}
              onChangeDraftName={onChangeDraftName}
              onCommitEdit={onCommitEdit}
              onSelect={() => onSelectProvider(provider.id)}
              onStartEdit={() => onStartProviderEdit(provider.id, 'rail')}
            />
          ))}
        </ScrollArea>
      </div>
    </aside>
  );
}

/// 渲染右侧表单头部。
/// @param props.draftName 编辑中的模型商名称。
/// @param props.editing 是否正在编辑模型商名称。
/// @param props.onCancelEdit 取消编辑回调。
/// @param props.onChangeDraftName 更新编辑草稿回调。
/// @param props.onCommitEdit 提交编辑回调。
/// @param props.onStartEdit 开始编辑回调。
/// @param props.providerName 当前模型商名称。
function ProviderFormHeader({
  draftName,
  editing,
  onCancelEdit,
  onChangeDraftName,
  onCommitEdit,
  onStartEdit,
  providerName,
}: {
  draftName: string;
  editing: boolean;
  onCancelEdit: () => void;
  onChangeDraftName: (name: string) => void;
  onCommitEdit: () => void;
  onStartEdit: () => void;
  providerName: string;
}) {
  /// 处理模型商名称输入。
  /// @param event 输入事件。
  function HandleProviderNameChange(event: ChangeEvent<HTMLInputElement>) {
    onChangeDraftName(event.target.value);
  }

  /// 处理模型商名称输入快捷键。
  /// @param event 键盘事件。
  function HandleProviderNameKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Enter') {
      event.preventDefault();
      onCommitEdit();
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      onCancelEdit();
    }
  }

  return (
    <header className="ModelFormHeader">
      <div className="ModelFormTitleWrap">
        {editing ? (
          <input
            aria-label={I18n.modelSettings.providerRailTitle}
            autoFocus
            className="ModelProviderNameInput"
            onBlur={onCommitEdit}
            onChange={HandleProviderNameChange}
            onKeyDown={HandleProviderNameKeyDown}
            value={draftName}
          />
        ) : (
          <h2>{providerName}</h2>
        )}
        {!editing ? (
          <button aria-label={I18n.modelSettings.editProviderNameAria} className="ModelProviderNameEditButton" onClick={onStartEdit} type="button">
            <IconGlyph name="square-pen" size={11} />
          </button>
        ) : null}
      </div>
    </header>
  );
}

/// 渲染未选择模型商时的空状态。
/// @param props.loading 是否正在加载。
function ModelProviderEmptyState({ loading }: { loading: boolean }) {
  return (
    <div className="ModelProviderEmptyState" aria-label={loading ? I18n.modelSettings.loadingProviders : I18n.modelSettings.noProviderSelected} role="status">
      <IconGlyph name="boxes" size={52} />
      <span>{loading ? I18n.modelSettings.loadingProviders : I18n.modelSettings.noProviderSelected}</span>
    </div>
  );
}

/// 渲染单个模型行。
/// @param props.model 模型数据。
/// @param props.onChangeProtocol 协议变化回调。
/// @param props.onSelect 选中当前模型回调。
/// @param props.onToggleEnabled 启用状态切换回调。
/// @param props.protocolOptions 协议下拉选项。
/// @param props.selected 当前模型是否选中。
function ModelRow({
  model,
  onChangeProtocol,
  onSelect,
  onToggleEnabled,
  protocolOptions,
  selected,
}: {
  model: ModelItem;
  onChangeProtocol: (protocol: ModelProtocol) => void;
  onSelect: () => void;
  onToggleEnabled: () => void;
  protocolOptions: SelectFieldOption<string>[];
  selected: boolean;
}) {
  const className = selected ? 'ModelRow ModelRowSelected' : 'ModelRow';
  const modelListAriaLabels = I18n.modelSettings.modelListAriaLabels;
  const modelListLabels = I18n.modelSettings.modelListLabels;
  const enabledToggleLabel = `${model.enabled
    ? modelListAriaLabels.disableModelPrefix
    : modelListAriaLabels.enableModelPrefix} ${model.name}`;

  /// 处理模型行键盘选择。
  /// @param event 键盘事件。
  function HandleModelRowKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onSelect();
    }
  }

  /// 阻止行内控件触发模型行选中。
  /// @param event 鼠标或键盘事件。
  function StopModelRowSelection(event: MouseEvent<HTMLDivElement> | KeyboardEvent<HTMLDivElement>) {
    event.stopPropagation();
  }

  return (
    <div
      aria-selected={selected}
      className={className}
      onClick={onSelect}
      onKeyDown={HandleModelRowKeyDown}
      role="row"
      tabIndex={0}
    >
      <div className="ModelNameCell" role="cell">
        <span>{model.name}</span>
      </div>
      <div className="ModelProtocolCell" role="cell">
        <div
          className="ModelProtocolSelectWrap"
          onClick={StopModelRowSelection}
          onKeyDown={StopModelRowSelection}
        >
          <SelectField
            ariaLabel={`${model.name} ${modelListLabels.protocol}`}
            className="ModelProtocolSelect"
            fontSize={11}
            height={26}
            options={protocolOptions}
            value={model.protocol}
            width={172}
            onChange={onChangeProtocol}
          />
        </div>
      </div>
      <div className="ModelEnabledCell" role="cell">
        <div
          className="ModelEnabledToggleWrap"
          onClick={StopModelRowSelection}
          onKeyDown={StopModelRowSelection}
        >
          <ModelToggle
            enabled={model.enabled}
            label={enabledToggleLabel}
            onToggle={onToggleEnabled}
          />
        </div>
      </div>
    </div>
  );
}

/// 渲染模型表格。
/// @param props.models 模型列表。
/// @param props.onChangeModelProtocol 修改模型协议回调。
/// @param props.onEditModel 编辑模型参数回调。
/// @param props.onRefresh 刷新模型回调。
/// @param props.onToggleAllModels 切换全部模型回调。
/// @param props.onToggleModel 切换单个模型启用状态回调。
/// @param props.protocolOptions 协议下拉选项。
/// @param props.refreshError 刷新错误信息。
/// @param props.refreshing 是否正在刷新。
function ModelListPanel({
  models,
  onChangeModelProtocol,
  onEditModel,
  onRefresh,
  onToggleAllModels,
  onToggleModel,
  protocolOptions,
  refreshError,
  refreshing,
}: {
  models: ModelItem[];
  onChangeModelProtocol: (modelId: string, protocol: ModelProtocol) => void;
  onEditModel: (modelId: string) => void;
  onRefresh: () => void;
  onToggleAllModels: () => void;
  onToggleModel: (modelId: string) => void;
  protocolOptions: SelectFieldOption<string>[];
  refreshError: string;
  refreshing: boolean;
}) {
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const allModelsEnabled = models.length > 0 && models.every((model) => model.enabled);
  const editDisabled = selectedModelId === null;
  const modelListAriaLabels = I18n.modelSettings.modelListAriaLabels;
  const modelListLabels = I18n.modelSettings.modelListLabels;
  /// 模型行选择区域根节点，用于判断外部点击。
  const modelListSectionRef = useRef<HTMLElement | null>(null);

  /// 选中指定模型行。
  /// @param modelId 模型 ID。
  function SelectModel(modelId: string) {
    setSelectedModelId(modelId);
  }

  /// 编辑当前选中的模型。
  function EditSelectedModel() {
    if (selectedModelId === null) {
      return;
    }

    onEditModel(selectedModelId);
  }

  useEffect(() => {
    if (selectedModelId !== null && !models.some((model) => model.modelId === selectedModelId)) {
      setSelectedModelId(null);
    }
  }, [models, selectedModelId]);

  useEffect(() => {
    if (selectedModelId === null) {
      return;
    }

    /// 处理模型行选择区域外点击。
    /// @param event 指针事件。
    function HandleDocumentPointerDown(event: PointerEvent) {
      const target = event.target;

      if (!(target instanceof Element)) {
        return;
      }

      const section = modelListSectionRef.current;

      if (section !== null && section.contains(target)) {
        const keepSelection = target.closest('.ModelRow') !== null || target.closest('.ModelListActions') !== null;

        if (keepSelection) {
          return;
        }
      }

      setSelectedModelId(null);
    }

    document.addEventListener('pointerdown', HandleDocumentPointerDown, true);

    return () => {
      document.removeEventListener('pointerdown', HandleDocumentPointerDown, true);
    };
  }, [selectedModelId]);

  return (
    <section className="ModelListSection" ref={modelListSectionRef} aria-labelledby="model-list-heading">
      <div className="ModelListActions">
        <div className="ModelListPrimaryActions">
          <button className="ModelRefreshButton" disabled={refreshing} onClick={onRefresh} type="button">
            <IconGlyph className={refreshing ? 'ModelRefreshIconSpinning' : ''} name="refresh-cw" size={14} />
            <span>{modelListLabels.refresh}</span>
          </button>
          {refreshError.length > 0 ? (
            <span className="ModelRefreshError" role="alert">
              {refreshError}
            </span>
          ) : null}
        </div>
        <div className="ModelListSecondaryActions">
          <button
            aria-disabled={editDisabled}
            className="ModelEditButton"
            disabled={editDisabled}
            onClick={EditSelectedModel}
            type="button"
          >
            <IconGlyph name="square-pen" size={14} />
            <span>{modelListLabels.edit}</span>
          </button>
          <div className="ModelSelectAll">
            <span>{modelListLabels.selectAll}</span>
            <ModelToggle
              disabled={models.length === 0}
              enabled={allModelsEnabled}
              label={modelListAriaLabels.toggleAllModels}
              onToggle={onToggleAllModels}
            />
          </div>
        </div>
      </div>

      <div className="ModelListPanel" role="table" aria-labelledby="model-list-heading">
        <div className="ModelListHeader" role="row">
          <span id="model-list-heading" role="columnheader">{modelListLabels.modelName}</span>
          <span role="columnheader">{modelListLabels.protocol}</span>
          <span role="columnheader">{modelListLabels.enabled}</span>
        </div>
        <div className="ModelRowsWrap">
          <div aria-label={modelListAriaLabels.list} className="ScrollArea ModelRows" role="rowgroup" tabIndex={0}>
            {models.map((model) => (
              <ModelRow
                key={model.modelId}
                model={model}
                protocolOptions={protocolOptions}
                selected={model.modelId === selectedModelId}
                onChangeProtocol={(protocol) => onChangeModelProtocol(model.modelId, protocol)}
                onSelect={() => SelectModel(model.modelId)}
                onToggleEnabled={() => onToggleModel(model.modelId)}
              />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/// 渲染模型设置面板。
/// @param props.onModelsChange 模型配置保存或删除成功后的通知回调。
function ModelSettingsPanel({ onModelsChange }: ModelSettingsPanelProps) {
  const [activeProviderId, setActiveProviderId] = useState<string | null>(null);
  const [apiProviderApis, setApiProviderApis] = useState<string[]>([]);
  const [apiKeyHidden, setApiKeyHidden] = useState(true);
  const [deletingProvider, setDeletingProvider] = useState(false);
  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const [editingProviderSource, setEditingProviderSource] = useState<ProviderEditSource | null>(null);
  const [editingModelId, setEditingModelId] = useState<string | null>(null);
  const [loadingProviders, setLoadingProviders] = useState(false);
  const [providerNameDraft, setProviderNameDraft] = useState('');
  const [providers, setProviders] = useState<ProviderItem[]>([]);
  const [refreshError, setRefreshError] = useState('');
  const [refreshingModels, setRefreshingModels] = useState(false);
  const [saveError, setSaveError] = useState('');
  const [savingProvider, setSavingProvider] = useState(false);
  /// 取消编辑标记，用于拦截 Escape 后触发的 blur 提交。
  const cancelProviderEditRef = useRef(false);
  /// 新增模型商 ID 序号。
  const newProviderIndexRef = useRef(1);

  const apiProviderSelectOptions = BuildApiSelectOptions(apiProviderApis);
  const activeProvider = activeProviderId === null
    ? null
    : providers.find((provider) => provider.id === activeProviderId) ?? null;
  const editingActiveProvider = activeProvider !== null
    && editingProviderId === activeProvider.id
    && editingProviderSource === 'header';
  const providerDraftDirty = activeProvider !== null
    && editingProviderId === activeProvider.id
    && providerNameDraft.trim() !== activeProvider.provider.name;
  const activeProviderDirty = activeProvider !== null && (providerDraftDirty || IsProviderDirty(activeProvider));
  const editingModel = activeProvider === null || editingModelId === null
    ? null
    : activeProvider.models.find((model) => model.modelId === editingModelId) ?? null;

  /// 加载所有模型商模型数据。
  /// @param nextActiveProviderName 加载后需要选中的模型商名称。
  async function LoadProviderData(nextActiveProviderName?: string) {
    setLoadingProviders(true);

    try {
      const output = await invoke<AllProviderModelsOutput>('all_provider_models_map');
      const nextProviders = BuildProviderItems(output);
      const nextActiveProvider = nextActiveProviderName
        ? nextProviders.find((provider) => provider.provider.name === nextActiveProviderName) ?? null
        : null;

      setApiProviderApis(output.apiProviderApis);
      setProviders(nextProviders);
      setActiveProviderId(nextActiveProvider?.id ?? null);
      setEditingProviderId(null);
      setEditingProviderSource(null);
      setEditingModelId(null);
      setProviderNameDraft('');
    } catch (error) {
      ReportModelSettingsBackendError('加载模型商模型数据失败', error);
    } finally {
      setLoadingProviders(false);
    }
  }

  useEffect(() => {
    void LoadProviderData();
  }, []);

  /// 选择模型商。
  /// @param providerId 模型商 ID。
  function SelectProvider(providerId: string) {
    setActiveProviderId(providerId);
    setEditingProviderId(null);
    setEditingProviderSource(null);
    setEditingModelId(null);
    setRefreshError('');
    setSaveError('');
  }

  /// 清理模型商选中态。
  function ClearProviderSelection() {
    setActiveProviderId(null);
    setEditingProviderId(null);
    setEditingProviderSource(null);
    setEditingModelId(null);
    setRefreshError('');
    setSaveError('');
  }

  /// 开始编辑指定模型商名称。
  /// @param providerId 模型商 ID。
  /// @param source 编辑入口。
  function StartProviderNameEditForProvider(providerId: string, source: ProviderEditSource) {
    const provider = providers.find((currentProvider) => currentProvider.id === providerId);

    if (provider === undefined) {
      return;
    }

    cancelProviderEditRef.current = false;
    setActiveProviderId(provider.id);
    setProviderNameDraft(provider.provider.name);
    setEditingProviderId(provider.id);
    setEditingProviderSource(source);
    setEditingModelId(null);
  }

  /// 开始编辑当前模型商名称。
  function StartProviderNameEdit() {
    if (activeProvider === null) {
      return;
    }

    StartProviderNameEditForProvider(activeProvider.id, 'header');
  }

  /// 提交当前模型商名称。
  function CommitProviderNameEdit() {
    if (cancelProviderEditRef.current) {
      cancelProviderEditRef.current = false;
      return;
    }

    if (editingProviderId === null) {
      return;
    }

    const nextName = BuildUniqueProviderName(providerNameDraft, providers, editingProviderId);

    setProviders((currentProviders) =>
      currentProviders.map((provider) => {
        if (provider.id !== editingProviderId) {
          return provider;
        }

        return {
          ...provider,
          name: nextName,
          provider: {
            ...provider.provider,
            name: nextName,
          },
        };
      })
    );
    setProviderNameDraft(nextName);
    setEditingProviderId(null);
    setEditingProviderSource(null);
  }

  /// 取消当前模型商名称编辑。
  function CancelProviderNameEdit() {
    const editingProvider = providers.find((provider) => provider.id === editingProviderId);

    cancelProviderEditRef.current = true;
    setProviderNameDraft(editingProvider?.provider.name ?? '');
    setEditingProviderId(null);
    setEditingProviderSource(null);
  }

  /// 新增未保存模型商。
  function AddProvider() {
    const providerId = `${NewProviderIdPrefix}:${newProviderIndexRef.current}`;
    const defaultApi = GetDefaultApi(apiProviderApis);
    const defaultProviderName = GetDefaultProviderName();
    const provider: ProviderRecord = {
      api: defaultApi,
      apiKey: '',
      baseUrl: '',
      name: defaultProviderName,
    };
    const providerItem: ProviderItem = {
      id: providerId,
      models: [],
      name: defaultProviderName,
      provider,
      savedModels: null,
      savedProvider: null,
      status: 'idle',
    };

    newProviderIndexRef.current += 1;
    cancelProviderEditRef.current = false;
    setProviders((currentProviders) => [providerItem, ...currentProviders]);
    setActiveProviderId(providerId);
    setProviderNameDraft(defaultProviderName);
    setEditingProviderId(providerId);
    setEditingProviderSource('rail');
    setEditingModelId(null);
    setRefreshError('');
    setSaveError('');
  }

  /// 从界面移除模型商。
  /// @param providerId 模型商 ID。
  function RemoveProviderFromView(providerId: string) {
    const providerIndex = providers.findIndex((provider) => provider.id === providerId);
    const nextProviders = providers.filter((provider) => provider.id !== providerId);
    const nextActiveProvider = nextProviders[providerIndex] ?? nextProviders[providerIndex - 1] ?? null;

    setProviders(nextProviders);
    setActiveProviderId(nextActiveProvider?.id ?? null);
    setEditingProviderId(null);
    setEditingProviderSource(null);
    setEditingModelId(null);
    setRefreshError('');
    setSaveError('');
  }

  /// 删除当前模型商。
  async function DeleteActiveProvider() {
    if (activeProvider === null || deletingProvider) {
      return;
    }

    setDeletingProvider(true);

    try {
      if (activeProvider.savedProvider !== null) {
        await invoke<void>('delete_provider', { name: activeProvider.savedProvider.name });
        onModelsChange();
      }

      RemoveProviderFromView(activeProvider.id);
    } catch (error) {
      ReportModelSettingsBackendError(`删除模型商失败: ${activeProvider.provider.name}`, error);
    } finally {
      setDeletingProvider(false);
    }
  }

  /// 更新当前模型商字段。
  /// @param field 字段名。
  /// @param value 字段值。
  function UpdateActiveProviderField(field: keyof Pick<ProviderRecord, 'api' | 'apiKey' | 'baseUrl'>, value: string) {
    if (activeProvider === null) {
      return;
    }

    setProviders((currentProviders) =>
      currentProviders.map((provider) => {
        if (provider.id !== activeProvider.id) {
          return provider;
        }

        return {
          ...provider,
          provider: {
            ...provider.provider,
            [field]: value,
          },
        };
      })
    );
  }

  /// 切换 API Key 明文和密文显示。
  function ToggleApiKeyHidden() {
    setApiKeyHidden((hidden) => !hidden);
  }

  /// 更新当前模型商模型列表。
  /// @param models 模型列表。
  function UpdateActiveProviderModels(models: ModelItem[]) {
    if (activeProvider === null) {
      return;
    }

    setProviders((currentProviders) =>
      currentProviders.map((provider) => {
        if (provider.id !== activeProvider.id) {
          return provider;
        }

        return {
          ...provider,
          models,
          status: GetProviderStatus(models),
        };
      })
    );
  }

  /// 打开模型参数编辑界面。
  /// @param modelId 模型 ID。
  function EditModelParams(modelId: string) {
    if (activeProvider === null || !activeProvider.models.some((model) => model.modelId === modelId)) {
      return;
    }

    setEditingModelId(modelId);
  }

  /// 取消模型参数编辑。
  function CancelModelParamsEdit() {
    setEditingModelId(null);
  }

  /// 确认模型参数编辑。
  /// @param nextModel 下一份模型数据。
  function ConfirmModelParamsEdit(nextModel: ModelItem) {
    if (activeProvider === null) {
      return;
    }

    UpdateActiveProviderModels(
      activeProvider.models.map((model) => (model.modelId === nextModel.modelId ? nextModel : model))
    );
    setEditingModelId(null);
  }

  /// 更新模型协议。
  /// @param modelId 模型 ID。
  /// @param protocol 协议。
  function ChangeModelProtocol(modelId: string, protocol: ModelProtocol) {
    if (activeProvider === null) {
      return;
    }

    UpdateActiveProviderModels(
      activeProvider.models.map((model) =>
        model.modelId === modelId
          ? {
              ...model,
              protocol,
            }
          : model
      )
    );
  }

  /// 切换模型启用状态。
  /// @param modelId 模型 ID。
  function ToggleModel(modelId: string) {
    if (activeProvider === null) {
      return;
    }

    UpdateActiveProviderModels(
      activeProvider.models.map((model) =>
        model.modelId === modelId
          ? {
              ...model,
              enabled: !model.enabled,
            }
          : model
      )
    );
  }

  /// 切换全部模型启用状态。
  function ToggleAllModels() {
    if (activeProvider === null || activeProvider.models.length === 0) {
      return;
    }

    const enabled = !activeProvider.models.every((model) => model.enabled);

    UpdateActiveProviderModels(
      activeProvider.models.map((model) => ({
        ...model,
        enabled,
      }))
    );
  }

  /// 刷新当前模型商远端模型列表。
  async function RefreshActiveProviderModels() {
    if (activeProvider === null || refreshingModels) {
      return;
    }

    setRefreshError('');
    setRefreshingModels(true);

    try {
      const provider = activeProvider.provider;
      const records = await invoke<ModelRecord[]>('fetch_models_from_provider', {
        input: {
          api: provider.api,
          apiKey: provider.apiKey,
          baseUrl: provider.baseUrl,
          name: provider.name,
        },
      });
      const nextModels = records.map((record) => BuildModelItem(record, apiProviderApis));

      UpdateActiveProviderModels(nextModels);
      setRefreshError('');
    } catch (error) {
      const errorMessage = ReportModelSettingsBackendError(`刷新模型列表失败: ${activeProvider.provider.name}`, error);
      /// 刷新失败时清空模型列表，避免继续展示过期数据。
      UpdateActiveProviderModels([]);
      setRefreshError(errorMessage);
    } finally {
      setRefreshingModels(false);
    }
  }

  /// 返回提交名称后的当前模型商。
  /// @param provider 模型商。
  function GetProviderWithCommittedName(provider: ProviderItem) {
    if (editingProviderId !== provider.id) {
      return provider;
    }

    const nextName = BuildUniqueProviderName(providerNameDraft, providers, provider.id);

    return {
      ...provider,
      name: nextName,
      provider: {
        ...provider.provider,
        name: nextName,
      },
    };
  }

  /// 保存当前模型商和模型列表。
  async function SaveActiveProvider() {
    if (activeProvider === null || savingProvider) {
      return;
    }

    const providerToSave = GetProviderWithCommittedName(activeProvider);
    const providerInput = {
      api: providerToSave.provider.api,
      apiKey: providerToSave.provider.apiKey,
      baseUrl: providerToSave.provider.baseUrl,
      name: providerToSave.provider.name,
    };
    const providerUpdateInput = {
      api: providerToSave.provider.api,
      apiKey: providerToSave.provider.apiKey,
      baseUrl: providerToSave.provider.baseUrl,
    };
    const records = providerToSave.models.map((model) => BuildModelRecord(providerToSave.provider.name, model));

    setSaveError('');
    setSavingProvider(true);
    setProviders((currentProviders) =>
      currentProviders.map((provider) => provider.id === providerToSave.id ? providerToSave : provider)
    );
    setEditingProviderId(null);
    setEditingProviderSource(null);
    setProviderNameDraft(providerToSave.provider.name);

    try {
      if (providerToSave.savedProvider === null) {
        await invoke<ProviderRecord>('create_provider', { input: providerInput });
      } else if (providerToSave.savedProvider.name !== providerToSave.provider.name) {
        await invoke<ProviderRecord>('create_provider', { input: providerInput });
        await invoke<ModelRecord[]>('sync_models_by_provider', {
          models: records,
          providerName: providerToSave.provider.name,
        });
        await invoke<void>('delete_provider', { name: providerToSave.savedProvider.name });
        await LoadProviderData(providerToSave.provider.name);
        onModelsChange();
        setSaveError('');
        return;
      } else {
        await invoke<ProviderRecord>('update_provider', {
          input: providerUpdateInput,
          name: providerToSave.savedProvider.name,
        });
      }

      await invoke<ModelRecord[]>('sync_models_by_provider', {
        models: records,
        providerName: providerToSave.provider.name,
      });
      await LoadProviderData(providerToSave.provider.name);
      onModelsChange();
      setSaveError('');
    } catch (error) {
      setSaveError(ReportModelSettingsBackendError(`保存模型商失败: ${providerToSave.provider.name}`, error));
    } finally {
      setSavingProvider(false);
    }
  }

  return (
    <section className="ModelSettingsPanel" aria-labelledby="model-settings-title">
      <SettingsPanelHeader
        description={I18n.settings.modelDescription}
        saveDisabled={activeProvider === null || !activeProviderDirty || savingProvider}
        saveError={saveError}
        title={I18n.settings.modelTitle}
        titleId="model-settings-title"
        onSave={SaveActiveProvider}
      />

      <div className="ModelSettingsContent">
        <ProviderRail
          activeProviderId={activeProviderId}
          deleting={deletingProvider}
          draftName={providerNameDraft}
          editingProviderId={editingProviderId}
          editingProviderSource={editingProviderSource}
          providers={providers}
          onAddProvider={AddProvider}
          onCancelEdit={CancelProviderNameEdit}
          onChangeDraftName={setProviderNameDraft}
          onClearProviderSelection={ClearProviderSelection}
          onCommitEdit={CommitProviderNameEdit}
          onDeleteProvider={DeleteActiveProvider}
          onSelectProvider={SelectProvider}
          onStartProviderEdit={StartProviderNameEditForProvider}
        />
        <div className="ModelContentDivider" aria-hidden="true" />

        <section className="ModelFormPanel" aria-labelledby="model-settings-title">
          {activeProvider !== null ? (
            <>
              <ProviderFormHeader
                draftName={providerNameDraft}
                editing={editingActiveProvider}
                providerName={activeProvider.provider.name}
                onCancelEdit={CancelProviderNameEdit}
                onChangeDraftName={setProviderNameDraft}
                onCommitEdit={CommitProviderNameEdit}
                onStartEdit={StartProviderNameEdit}
              />
              {editingModel !== null ? (
                <ModelParamsEditor
                  model={editingModel}
                  onCancel={CancelModelParamsEdit}
                  onConfirm={ConfirmModelParamsEdit}
                />
              ) : (
                <div className="ModelFormBody">
                  <ConfigField
                    label="Base URL"
                    placeholder={I18n.modelSettings.baseUrlPlaceholder}
                    value={activeProvider.provider.baseUrl}
                    onChange={(value) => UpdateActiveProviderField('baseUrl', value)}
                  />
                  <ConfigField
                    action={
                      <button
                        aria-label={apiKeyHidden ? I18n.modelSettings.showApiKeyAria : I18n.modelSettings.hideApiKeyAria}
                        className="ModelMaskButton"
                        onClick={ToggleApiKeyHidden}
                        type="button"
                      >
                        <IconGlyph name={apiKeyHidden ? 'eye' : 'eye-off'} size={17} />
                      </button>
                    }
                    label="API Key"
                    placeholder={I18n.modelSettings.apiKeyPlaceholder}
                    type={apiKeyHidden ? 'password' : 'text'}
                    value={activeProvider.provider.apiKey}
                    onChange={(value) => UpdateActiveProviderField('apiKey', value)}
                  />
                  <ConfigSelectField
                    label="API"
                    options={apiProviderSelectOptions}
                    value={activeProvider.provider.api}
                    onChange={(value) => UpdateActiveProviderField('api', value)}
                  />
                  <ModelListPanel
                    models={activeProvider.models}
                    protocolOptions={apiProviderSelectOptions}
                    refreshError={refreshError}
                    refreshing={refreshingModels}
                    onChangeModelProtocol={ChangeModelProtocol}
                    onEditModel={EditModelParams}
                    onRefresh={RefreshActiveProviderModels}
                    onToggleAllModels={ToggleAllModels}
                    onToggleModel={ToggleModel}
                  />
                </div>
              )}
            </>
          ) : (
            <ModelProviderEmptyState loading={loadingProviders} />
          )}
        </section>
      </div>
    </section>
  );
}

export default ModelSettingsPanel;
