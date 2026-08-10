import ActionButton from './ActionButton';
import IconGlyph from './IconGlyph';
import type { IconName } from './IconGlyph';

interface IconButtonProps {
  ariaLabel: string;
  className?: string;
  icon: IconName;
  onClick?: () => void;
  size?: number;
}

/// 渲染纯图标按钮。
/// @param props.ariaLabel 图标按钮无障碍名称。
/// @param props.className 附加样式类。
/// @param props.icon 图标名称。
/// @param props.onClick 点击回调。
/// @param props.size 图标尺寸。
function IconButton({ ariaLabel, className = '', icon, onClick, size = 16 }: IconButtonProps) {
  return (
    <ActionButton ariaLabel={ariaLabel} className={`IconButton ${className}`.trim()} onClick={onClick}>
      <IconGlyph name={icon} size={size} />
    </ActionButton>
  );
}

export default IconButton;
