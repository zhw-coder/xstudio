import { useEffect, useId, useRef, useState } from 'react';
import type { CSSProperties, KeyboardEvent, ReactNode } from 'react';
import IconGlyph from './IconGlyph';
import type { IconName } from './IconGlyph';

export interface SelectFieldOption<TValue extends string> {
  icon?: IconName;
  iconColor?: string;
  label: ReactNode;
  type?: 'option';
  value: TValue;
}

export interface SelectFieldGroupOption {
  label: ReactNode;
  type: 'group';
}

export type SelectFieldItem<TValue extends string> = SelectFieldGroupOption | SelectFieldOption<TValue>;

export type SelectFieldOptionAlignment = 'left' | 'center' | 'right';

interface SelectFieldProps<TValue extends string> {
  ariaLabel: string;
  backgroundColor?: string;
  borderRadius?: number | string;
  className?: string;
  fontSize?: number | string;
  height?: number | string;
  optionAlignment?: SelectFieldOptionAlignment;
  options: SelectFieldItem<TValue>[];
  title?: string;
  value: TValue;
  width?: number | string;
  onChange?: (value: TValue) => void;
}

type SelectFieldPlacement = 'bottom' | 'top';

/// 下拉框默认最大高度。
const SelectFieldMaxListboxHeight = 220;

/// 下拉框和触发按钮之间的间距。
const SelectFieldGap = 2;

/// 下拉框最小可用高度。
const SelectFieldMinListboxHeight = 96;

/// 选择器视口边距。
const SelectFieldViewportPadding = 8;

/// 判断是否是可选项。
/// @param item 下拉列表项。
function IsSelectFieldOption<TValue extends string>(item: SelectFieldItem<TValue>): item is SelectFieldOption<TValue> {
  return item.type !== 'group';
}

/// 查找最近可滚动祖先。
/// @param element 起始元素。
function GetScrollParent(element: HTMLElement | null): HTMLElement | null {
  let current = element?.parentElement ?? null;

  while (current) {
    const style = window.getComputedStyle(current);
    const overflowY = style.overflowY;

    if (overflowY === 'auto' || overflowY === 'scroll') {
      return current;
    }

    current = current.parentElement;
  }

  return null;
}

/// 计算下拉框弹出方向和最大高度。
/// @param trigger 触发按钮。
/// @param scrollParent 最近滚动容器。
function GetListboxLayout(trigger: HTMLElement, scrollParent: HTMLElement | null) {
  const triggerRect = trigger.getBoundingClientRect();
  const boundaryRect = scrollParent?.getBoundingClientRect();
  const boundaryTop = boundaryRect ? boundaryRect.top : 0;
  const boundaryBottom = boundaryRect ? boundaryRect.bottom : window.innerHeight;
  const topLimit = Math.max(SelectFieldViewportPadding, boundaryTop + SelectFieldViewportPadding);
  const bottomLimit = Math.min(window.innerHeight - SelectFieldViewportPadding, boundaryBottom - SelectFieldViewportPadding);
  const spaceAbove = triggerRect.top - topLimit;
  const spaceBelow = bottomLimit - triggerRect.bottom;
  const placement: SelectFieldPlacement =
    spaceBelow < SelectFieldMinListboxHeight + SelectFieldGap && spaceAbove > spaceBelow ? 'top' : 'bottom';
  const availableHeight = placement === 'top' ? spaceAbove - SelectFieldGap : spaceBelow - SelectFieldGap;
  const maxHeight =
    availableHeight < SelectFieldMinListboxHeight
      ? Math.max(48, availableHeight)
      : Math.min(SelectFieldMaxListboxHeight, availableHeight);

  return { maxHeight, placement };
}

/// 渲染自绘下拉选择器。
/// @param props.ariaLabel 无障碍标签。
/// @param props.backgroundColor 触发框和选项列表背景色。
/// @param props.borderRadius 触发按钮边框圆角。
/// @param props.className 附加样式类。
/// @param props.fontSize 字体大小。
/// @param props.height 触发按钮高度。
/// @param props.optionAlignment 当前选中展示值对齐方式。
/// @param props.options 选项列表。
/// @param props.title 鼠标悬停提示。
/// @param props.value 当前选中值。
/// @param props.width 选择器宽度。
/// @param props.onChange 选中值变化回调。
function SelectField<TValue extends string>({
  ariaLabel,
  backgroundColor,
  borderRadius,
  className = '',
  fontSize,
  height,
  onChange,
  optionAlignment = 'left',
  options,
  title,
  value,
  width,
}: SelectFieldProps<TValue>) {
  const listboxId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const scrollLockCleanupRef = useRef<(() => void) | null>(null);
  const scrollLockTopRef = useRef(0);
  const scrollParentRef = useRef<HTMLElement | null>(null);
  const [activeValue, setActiveValue] = useState(value);
  const [maxHeight, setMaxHeight] = useState(SelectFieldMaxListboxHeight);
  const [open, setOpen] = useState(false);
  const [placement, setPlacement] = useState<SelectFieldPlacement>('bottom');

  const selectableOptions = options.filter(IsSelectFieldOption);
  const selectedOption = selectableOptions.find((option) => option.value === value) ?? selectableOptions[0];
  const activeOption = selectableOptions.find((option) => option.value === activeValue) ?? selectedOption;
  const selectedOptionText = typeof selectedOption?.label === 'string' ? selectedOption.label : '';
  const triggerAriaLabel = selectedOptionText ? `${ariaLabel}: ${selectedOptionText}` : ariaLabel;

  /// 判断事件是否发生在下拉列表内部。
  /// @param event 交互事件。
  function IsListboxEvent(event: Event) {
    const target = event.target;

    return target instanceof Element && target.closest('.SelectFieldListbox') !== null;
  }

  /// 锁定最近外层滚动容器，保留滚动条但禁止外层滚动。
  /// @param trigger 触发按钮。
  function LockScrollParent(trigger: HTMLElement) {
    if (scrollParentRef.current) {
      return scrollParentRef.current;
    }

    const scrollParent = GetScrollParent(trigger);

    scrollParentRef.current = scrollParent;

    if (scrollParent) {
      const lockedScrollParent = scrollParent;
      scrollLockTopRef.current = lockedScrollParent.scrollTop;

      /// 阻止滚轮和触摸移动带动外层滚动。
      /// @param event 交互事件。
      function PreventOuterScroll(event: Event) {
        if (IsListboxEvent(event)) {
          return;
        }

        event.preventDefault();
      }

      /// 外层滚动条被拖动时复位滚动位置。
      function RestoreOuterScrollTop() {
        if (lockedScrollParent.scrollTop !== scrollLockTopRef.current) {
          lockedScrollParent.scrollTop = scrollLockTopRef.current;
        }
      }

      lockedScrollParent.addEventListener('wheel', PreventOuterScroll, { passive: false });
      lockedScrollParent.addEventListener('touchmove', PreventOuterScroll, { passive: false });
      lockedScrollParent.addEventListener('scroll', RestoreOuterScrollTop);
      scrollLockCleanupRef.current = () => {
        lockedScrollParent.removeEventListener('wheel', PreventOuterScroll);
        lockedScrollParent.removeEventListener('touchmove', PreventOuterScroll);
        lockedScrollParent.removeEventListener('scroll', RestoreOuterScrollTop);
      };
    }

    return scrollParent;
  }

  /// 恢复外层滚动容器。
  function RestoreScrollParent() {
    scrollLockCleanupRef.current?.();

    scrollLockCleanupRef.current = null;
    scrollParentRef.current = null;
  }

  /// 关闭下拉框。
  function CloseListbox() {
    RestoreScrollParent();
    setOpen(false);
  }

  /// 打开下拉框。
  function OpenListbox() {
    const trigger = triggerRef.current;

    if (!trigger || selectableOptions.length === 0) {
      return;
    }

    const scrollParent = LockScrollParent(trigger);
    const layout = GetListboxLayout(trigger, scrollParent);

    setActiveValue(value);
    setMaxHeight(layout.maxHeight);
    setPlacement(layout.placement);
    setOpen(true);
  }

  /// 切换下拉框开关。
  function ToggleListbox() {
    if (open) {
      CloseListbox();
      return;
    }

    OpenListbox();
  }

  /// 选择指定值。
  /// @param nextValue 目标选项值。
  function SelectOption(nextValue: TValue) {
    onChange?.(nextValue);
    setActiveValue(nextValue);
    CloseListbox();
    triggerRef.current?.focus();
  }

  /// 移动当前活动选项。
  /// @param direction 移动方向。
  function MoveActiveOption(direction: 1 | -1) {
    if (selectableOptions.length === 0) {
      return;
    }

    const currentIndex = Math.max(
      0,
      selectableOptions.findIndex((option) => option.value === activeOption.value)
    );
    const nextIndex = (currentIndex + direction + selectableOptions.length) % selectableOptions.length;
    setActiveValue(selectableOptions[nextIndex].value);
  }

  /// 处理触发按钮键盘事件。
  /// @param event 键盘事件。
  function HandleTriggerKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();

      if (!open) {
        OpenListbox();
        return;
      }

      MoveActiveOption(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }

    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();

      if (!open) {
        OpenListbox();
        return;
      }

      SelectOption(activeOption.value);
      return;
    }

    if (event.key === 'Escape' && open) {
      event.preventDefault();
      CloseListbox();
    }

    if (event.key === 'Tab' && open) {
      CloseListbox();
    }
  }

  useEffect(() => {
    if (!open) {
      return;
    }

    const trigger = triggerRef.current;

    if (trigger) {
      LockScrollParent(trigger);
    }

    /// 处理外部点击。
    /// @param event 鼠标事件。
    function HandleDocumentMouseDown(event: MouseEvent) {
      const target = event.target;

      if (target instanceof Node && rootRef.current?.contains(target)) {
        return;
      }

      CloseListbox();
    }

    document.addEventListener('mousedown', HandleDocumentMouseDown);

    return () => {
      document.removeEventListener('mousedown', HandleDocumentMouseDown);
      RestoreScrollParent();
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    document.getElementById(`${listboxId}-${activeValue}`)?.scrollIntoView({ block: 'nearest' });
  }, [activeValue, listboxId, open]);

  const fieldWidth = typeof width === 'number' ? `${width}px` : width;
  const fieldHeight = typeof height === 'number' ? `${height}px` : height;
  const fieldFontSize = typeof fontSize === 'number' ? `${fontSize}px` : fontSize;
  const fieldBorderRadius = typeof borderRadius === 'number' ? `${borderRadius}px` : borderRadius;
  const fieldStyle = {
    ...(backgroundColor ? { '--select-field-background': backgroundColor } : {}),
    ...(fieldBorderRadius ? { '--select-field-border-radius': fieldBorderRadius } : {}),
    ...(fieldFontSize ? { '--select-field-font-size': fieldFontSize } : {}),
    ...(fieldHeight ? { '--select-field-height': fieldHeight } : {}),
    ...(fieldWidth ? { width: fieldWidth } : {}),
  } as CSSProperties;

  return (
    <div className={`SelectField ${className}`.trim()} ref={rootRef} style={fieldStyle}>
      <button
        aria-controls={open ? listboxId : undefined}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={open && activeOption ? `${listboxId}-${activeOption.value}` : undefined}
        aria-label={triggerAriaLabel}
        className="SelectFieldTrigger"
        onClick={ToggleListbox}
        onKeyDown={HandleTriggerKeyDown}
        ref={triggerRef}
        title={title}
        type="button"
      >
        <span
          className={`${selectedOption?.icon ? 'SelectFieldValue SelectFieldIconValue' : 'SelectFieldValue'} SelectFieldValueAlign${
            optionAlignment[0].toUpperCase()
          }${optionAlignment.slice(1)}`}
        >
          {selectedOption?.icon ? (
            <span className="SelectFieldValueIcon" style={selectedOption.iconColor ? { color: selectedOption.iconColor } : undefined}>
              <IconGlyph name={selectedOption.icon} size={15} />
            </span>
          ) : selectedOption?.label}
        </span>
        <IconGlyph className={open ? 'SelectFieldChevron SelectFieldChevronOpen' : 'SelectFieldChevron'} name="chevron-down" size={14} />
      </button>

      {open ? (
        <div
          className={`SelectFieldListbox SelectFieldListbox-${placement}`}
          id={listboxId}
          role="listbox"
          style={{ maxHeight }}
          tabIndex={-1}
        >
          {options.map((option, optionIndex) => {
            if (!IsSelectFieldOption(option)) {
              return (
                <div className="SelectFieldGroupLabel" key={`group-${optionIndex}`} role="presentation">
                  {option.label}
                </div>
              );
            }

            const active = option.value === activeOption.value;
            const selected = option.value === value;

            return (
              <button
                aria-selected={selected}
                className={active ? 'SelectFieldOption SelectFieldOptionActive' : 'SelectFieldOption'}
                id={`${listboxId}-${option.value}`}
                key={option.value}
                onClick={() => SelectOption(option.value)}
                onMouseEnter={() => setActiveValue(option.value)}
                role="option"
                tabIndex={-1}
                type="button"
              >
                <span className="SelectFieldOptionCheck">
                  {selected ? <IconGlyph name="check" size={14} /> : null}
                </span>
                {option.icon ? (
                  <span className="SelectFieldOptionIcon" style={option.iconColor ? { color: option.iconColor } : undefined}>
                    <IconGlyph name={option.icon} size={14} />
                  </span>
                ) : null}
                <span className="SelectFieldOptionLabel">{option.label}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

export default SelectField;
