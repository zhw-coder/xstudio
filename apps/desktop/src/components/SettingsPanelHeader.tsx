import { I18n } from '../i18n';
import IconGlyph from './IconGlyph';

interface SettingsPanelHeaderProps {
  description?: string;
  onSave?: () => void;
  saveDisabled?: boolean;
  saveError?: string;
  title: string;
  titleId: string;
}

/// 渲染设置页公共顶部标题和保存操作。
/// @param props.description 设置页说明文案。
/// @param props.onSave 保存回调。
/// @param props.saveDisabled 保存按钮是否禁用。
/// @param props.saveError 保存错误信息。
/// @param props.title 设置页标题。
/// @param props.titleId 标题元素 ID，用于面板 aria-labelledby。
function SettingsPanelHeader({
  description,
  onSave,
  saveDisabled = true,
  saveError = '',
  title,
  titleId,
}: SettingsPanelHeaderProps) {
  return (
    <header className="SettingsPanelHeader">
      <div className="SettingsPanelTitleWrap">
        <span id={titleId}>{title}</span>
        {description ? <p>{description}</p> : null}
      </div>

      {onSave || saveError.length > 0 ? (
        <div className="SettingsPanelHeaderActions">
          {saveError.length > 0 ? (
            <span className="SettingsSaveError" role="alert">
              {saveError}
            </span>
          ) : null}
          {onSave ? (
            <button
              aria-disabled={saveDisabled}
              className="SettingsSaveButton"
              disabled={saveDisabled}
              onClick={onSave}
              type="button"
            >
              <IconGlyph name="save" size={16} />
              <span>{I18n.common.save}</span>
            </button>
          ) : null}
        </div>
      ) : null}
    </header>
  );
}

export default SettingsPanelHeader;
