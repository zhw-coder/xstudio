import type { ChangeEvent, ReactNode } from 'react';
import { SelectField } from '../../components';
import type { SelectFieldOption } from '../../components';

/// 渲染配置字段。
/// @param props.action 右侧附加操作。
/// @param props.label 字段标签。
/// @param props.onChange 字段值变化回调。
/// @param props.placeholder 占位提示文字。
/// @param props.type 输入类型。
/// @param props.value 字段值。
export function ConfigField({
  action,
  label,
  onChange,
  placeholder = '',
  type = 'text',
  value,
}: {
  action?: ReactNode;
  label: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: 'password' | 'text';
  value: string;
}) {
  /// 处理配置字段输入。
  /// @param event 输入事件。
  function HandleConfigValueChange(event: ChangeEvent<HTMLInputElement>) {
    onChange(event.target.value);
  }

  return (
    <section className="ModelConfigField" aria-label={label}>
      <span className="ModelConfigLabel">{label}</span>
      <div className="ModelConfigValueRow">
        <input
          aria-label={label}
          className="ModelConfigInput"
          onChange={HandleConfigValueChange}
          placeholder={placeholder}
          spellCheck={false}
          type={type}
          value={value}
        />
        {action}
      </div>
    </section>
  );
}

/// 渲染配置下拉字段。
/// @param props.label 字段标签。
/// @param props.onChange 字段值变化回调。
/// @param props.options 下拉选项。
/// @param props.value 字段值。
export function ConfigSelectField({
  label,
  onChange,
  options,
  value,
}: {
  label: string;
  onChange: (value: string) => void;
  options: SelectFieldOption<string>[];
  value: string;
}) {
  return (
    <section className="ModelConfigField ModelConfigFieldInline" aria-label={label}>
      <span className="ModelConfigLabel">{label}</span>
      <div className="ModelConfigValueRow">
        <SelectField
          ariaLabel={label}
          className="ModelConfigSelect"
          fontSize={13}
          height={28}
          options={options}
          value={value}
          width={420}
          onChange={onChange}
        />
      </div>
    </section>
  );
}

/// 渲染开关。
/// @param props.disabled 是否禁用。
/// @param props.enabled 是否启用。
/// @param props.label 无障碍标签。
/// @param props.onToggle 切换回调。
export function ModelToggle({
  disabled = false,
  enabled,
  label,
  onToggle,
}: {
  disabled?: boolean;
  enabled: boolean;
  label: string;
  onToggle?: () => void;
}) {
  return (
    <button
      aria-checked={enabled}
      aria-disabled={disabled}
      aria-label={label}
      className={enabled ? 'ModelToggle ModelToggleEnabled' : 'ModelToggle'}
      disabled={disabled}
      onClick={onToggle}
      role="switch"
      type="button"
    >
      <span />
    </button>
  );
}
