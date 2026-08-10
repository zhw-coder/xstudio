import type { ChangeEvent, ReactNode } from 'react';
import { useState } from 'react';
import { IconGlyph } from '../../components';
import { I18n } from '../../i18n';
import {
  GetCompatPlaceholder,
  GetContextWindowPlaceholder,
  GetMaxTokensPlaceholder,
  GetThinkingLevelMapPlaceholder,
  ModelInputOptions,
} from './constants';
import { ModelToggle } from './ModelControls';
import { CostMapEditor, ModelParamEntryList, ThinkingLevelMapEditor } from './ModelNestedEditors';
import type { ModelInputValue, ModelItem, ModelNestedEditor, ModelParamsDraft } from './types';
import { ApplyModelParamsDraft, BuildModelParamsDraft, FormatJsonDisplay } from './utils';

/// 渲染模型参数文本输入字段。
/// @param props.label 字段标签。
/// @param props.onChange 字段值变化回调。
/// @param props.placeholder 占位提示文字。
/// @param props.value 字段值。
function ModelParamTextField({
  label,
  inputMode = 'text',
  onChange,
  placeholder,
  value,
}: {
  label: string;
  inputMode?: 'numeric' | 'text';
  onChange: (value: string) => void;
  placeholder: string;
  value: string;
}) {
  /// 处理模型参数输入。
  /// @param event 输入事件。
  function HandleParamTextChange(event: ChangeEvent<HTMLInputElement>) {
    onChange(event.target.value);
  }

  return (
    <label className="ModelParamField ModelParamInlineField">
      <span className="ModelParamLabel">{label}</span>
      <input
        className="ModelParamInput"
        inputMode={inputMode}
        onChange={HandleParamTextChange}
        placeholder={placeholder}
        spellCheck={false}
        value={value}
      />
    </label>
  );
}

/// 渲染模型输入类型复选项。
/// @param props.checked 是否选中。
/// @param props.label 字段标签。
/// @param props.onToggle 切换回调。
function ModelParamCheckbox({
  checked,
  label,
  onToggle,
}: {
  checked: boolean;
  label: string;
  onToggle: () => void;
}) {
  return (
    <label className="ModelParamCheckbox">
      <input checked={checked} onChange={onToggle} type="checkbox" />
      <span>{label}</span>
    </label>
  );
}

/// 渲染只读 JSON 展示字段。
/// @param props.action 右侧配置按钮。
/// @param props.label 字段标签。
/// @param props.placeholder 空值占位提示。
/// @param props.value JSON 值。
function ModelParamJsonField({
  action,
  label,
  placeholder,
  value,
}: {
  action: ReactNode;
  label: string;
  placeholder: string;
  value: unknown;
}) {
  const displayValue = FormatJsonDisplay(value);

  return (
    <section className="ModelParamField ModelParamInlineField" aria-label={label}>
      <span className="ModelParamLabel">{label}</span>
      <div className="ModelParamJsonRow">
        <pre className={displayValue.length > 0 ? 'ModelParamJsonPreview' : 'ModelParamJsonPreview ModelParamJsonEmpty'}>
          {displayValue.length > 0 ? displayValue : placeholder}
        </pre>
        {action}
      </div>
    </section>
  );
}

/// 渲染模型参数编辑二级界面。
/// @param props.model 当前模型。
/// @param props.onCancel 取消回调。
/// @param props.onConfirm 确认回调。
function ModelParamsEditor({
  model,
  onCancel,
  onConfirm,
}: {
  model: ModelItem;
  onCancel: () => void;
  onConfirm: (model: ModelItem) => void;
}) {
  const [draft, setDraft] = useState<ModelParamsDraft>(() => BuildModelParamsDraft(model.model));
  const [nestedEditor, setNestedEditor] = useState<ModelNestedEditor | null>(null);
  const modelTitle = typeof model.model.id === 'string' ? model.model.id : model.name;
  const modelParamLabels = I18n.modelSettings.modelParamLabels;

  /// 更新模型参数草稿字段。
  /// @param nextDraft 下一份草稿。
  function ChangeDraft(nextDraft: Partial<ModelParamsDraft>) {
    setDraft((currentDraft) => ({
      ...currentDraft,
      ...nextDraft,
    }));
  }

  /// 切换模型输入类型。
  /// @param inputValue 输入类型。
  function ToggleInputValue(inputValue: ModelInputValue) {
    const nextInputValues = draft.inputValues.includes(inputValue)
      ? draft.inputValues.filter((value) => value !== inputValue)
      : [...draft.inputValues, inputValue];

    ChangeDraft({ inputValues: nextInputValues });
  }

  /// 确认模型参数。
  function ConfirmModelParams() {
    onConfirm({
      ...model,
      model: ApplyModelParamsDraft(model.model, draft),
    });
  }

  if (nestedEditor === 'thinkingLevelMap') {
    return (
      <ThinkingLevelMapEditor
        initialValue={draft.thinkingLevelMap}
        onCancel={() => setNestedEditor(null)}
        onConfirm={(value) => {
          ChangeDraft({ thinkingLevelMap: value });
          setNestedEditor(null);
        }}
      />
    );
  }

  if (nestedEditor === 'cost') {
    return (
      <CostMapEditor
        initialValue={draft.cost}
        title={modelParamLabels.cost}
        onCancel={() => setNestedEditor(null)}
        onConfirm={(value) => {
          ChangeDraft({ cost: value });
          setNestedEditor(null);
        }}
      />
    );
  }

  return (
    <section className="ModelParamEditor" aria-labelledby="model-param-editor-title">
      <div className="ModelParamEditorHeader">
        <h3 id="model-param-editor-title">{modelTitle}</h3>
        <span>{I18n.modelSettings.modelParamsTitle}</span>
      </div>

      <div className="ModelParamScroll">
        <section className="ModelParamField ModelParamInlineField" aria-label={modelParamLabels.reasoning}>
          <span className="ModelParamLabel">{modelParamLabels.reasoning}</span>
          <ModelToggle
            enabled={draft.reasoning}
            label={draft.reasoning ? I18n.modelSettings.reasoningDisableAria : I18n.modelSettings.reasoningEnableAria}
            onToggle={() => ChangeDraft({ reasoning: !draft.reasoning })}
          />
        </section>

        <section className="ModelParamField ModelParamInlineField" aria-label={modelParamLabels.input}>
          <span className="ModelParamLabel">{modelParamLabels.input}</span>
          <div className="ModelParamCheckboxGroup">
            {ModelInputOptions.map((inputValue) => (
              <ModelParamCheckbox
                checked={draft.inputValues.includes(inputValue)}
                key={inputValue}
                label={inputValue}
                onToggle={() => ToggleInputValue(inputValue)}
              />
            ))}
          </div>
        </section>

        <ModelParamTextField
          inputMode="numeric"
          label={modelParamLabels.contextWindow}
          placeholder={GetContextWindowPlaceholder()}
          value={draft.contextWindow}
          onChange={(value) => ChangeDraft({ contextWindow: value })}
        />

        <ModelParamTextField
          inputMode="numeric"
          label={modelParamLabels.maxTokens}
          placeholder={GetMaxTokensPlaceholder()}
          value={draft.maxTokens}
          onChange={(value) => ChangeDraft({ maxTokens: value })}
        />

        <ModelParamJsonField
          action={
            <button
              aria-label={I18n.modelSettings.setupThinkingLevelMapAria}
              className="ModelParamIconButton"
              onClick={() => setNestedEditor('thinkingLevelMap')}
              type="button"
            >
              <IconGlyph name="settings-2" size={14} />
            </button>
          }
          label={modelParamLabels.thinkingLevelMap}
          placeholder={GetThinkingLevelMapPlaceholder()}
          value={draft.thinkingLevelMap}
        />

        <ModelParamJsonField
          action={
            <button
              aria-label={I18n.modelSettings.setupCostAria}
              className="ModelParamIconButton"
              onClick={() => setNestedEditor('cost')}
              type="button"
            >
              <IconGlyph name="settings-2" size={14} />
            </button>
          }
          label={modelParamLabels.cost}
          placeholder="{}"
          value={draft.cost}
        />

        <section className="ModelParamField" aria-label="Headers">
          <span className="ModelParamLabel">Headers</span>
          <ModelParamEntryList
            entries={draft.headers}
            keyPlaceholder="Header"
            valuePlaceholder="Value"
            onChange={(headers) => ChangeDraft({ headers })}
          />
        </section>

        <section className="ModelParamField ModelParamInlineField" aria-label={modelParamLabels.compat}>
          <span className="ModelParamLabel">{modelParamLabels.compat}</span>
          <input
            className="ModelParamInput"
            onChange={(event) => ChangeDraft({ compat: event.target.value })}
            placeholder={GetCompatPlaceholder()}
            spellCheck={false}
            value={draft.compat}
          />
        </section>
      </div>

      <div className="ModelParamActions">
        <button className="ModelParamSecondaryButton" onClick={onCancel} type="button">{I18n.common.cancel}</button>
        <button className="ModelParamPrimaryButton" onClick={ConfirmModelParams} type="button">{I18n.common.confirm}</button>
      </div>
    </section>
  );
}

export default ModelParamsEditor;
