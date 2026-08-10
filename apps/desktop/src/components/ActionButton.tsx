import type { ReactNode } from 'react';

interface ActionButtonProps {
  ariaControls?: string;
  ariaCurrent?: 'page';
  ariaExpanded?: boolean;
  ariaLabel?: string;
  children: ReactNode;
  className?: string;
  onClick?: () => void;
  type?: 'button' | 'submit';
}

/// 渲染通用按钮，统一 hover、focus 和点击语义。
/// @param props.ariaControls 按钮控制的元素 ID。
/// @param props.ariaCurrent 当前项语义。
/// @param props.ariaExpanded 按钮控制区域是否展开。
/// @param props.ariaLabel 按钮无障碍名称。
/// @param props.children 按钮内容。
/// @param props.className 附加样式类。
/// @param props.onClick 点击回调。
/// @param props.type 按钮类型。
function ActionButton({
  ariaControls,
  ariaCurrent,
  ariaExpanded,
  ariaLabel,
  children,
  className = '',
  onClick,
  type = 'button',
}: ActionButtonProps) {
  return (
    <button
      aria-controls={ariaControls}
      aria-current={ariaCurrent}
      aria-expanded={ariaExpanded}
      aria-label={ariaLabel}
      className={`ActionButton ${className}`.trim()}
      onClick={onClick}
      type={type}
    >
      {children}
    </button>
  );
}

export default ActionButton;
