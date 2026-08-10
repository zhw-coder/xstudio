import { forwardRef } from 'react';
import type { ReactNode } from 'react';

interface ScrollAreaProps {
  ariaLabel: string;
  children: ReactNode;
  className?: string;
}

/// 渲染通用滚动区域，内容未溢出时不显示滚动条。
/// @param props.ariaLabel 滚动区域无障碍名称。
/// @param props.children 滚动内容。
/// @param props.className 附加样式类。
const ScrollArea = forwardRef<HTMLDivElement, ScrollAreaProps>(function ScrollArea(
  { ariaLabel, children, className = '' },
  ref
) {
  return (
    <div
      ref={ref}
      aria-label={ariaLabel}
      className={`ScrollArea ${className}`.trim()}
      role="region"
      tabIndex={0}
    >
      {children}
    </div>
  );
});

export default ScrollArea;
