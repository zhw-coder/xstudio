import type { ReactNode } from 'react';

export type IconName =
  | 'arrow-left'
  | 'arrow-up'
  | 'bot'
  | 'boxes'
  | 'brain'
  | 'check'
  | 'chevron-down'
  | 'circle-alert'
  | 'circle-check-big'
  | 'circle-info'
  | 'code-2'
  | 'eye'
  | 'eye-off'
  | 'folder-kanban'
  | 'git-branch'
  | 'globe'
  | 'image'
  | 'list-tree'
  | 'messages-square'
  | 'minus'
  | 'paperclip'
  | 'panel-left-close'
  | 'panel-left-open'
  | 'plug-zap'
  | 'plus'
  | 'refresh-cw'
  | 'save'
  | 'search'
  | 'settings-2'
  | 'shield-check'
  | 'sliders-horizontal'
  | 'sparkles'
  | 'square'
  | 'square-pen'
  | 'trash-2'
  | 'wrench'
  | 'x';

interface IconGlyphProps {
  className?: string;
  name: IconName;
  size?: number;
}

/// 渲染 Lucide 风格线性图标。
/// @param props.className 附加样式类。
/// @param props.name 图标名称。
/// @param props.size 图标尺寸。
function IconGlyph({ className = '', name, size = 18 }: IconGlyphProps) {
  const paths: Record<IconName, ReactNode> = {
    'arrow-left': (
      <>
        <path d="m12 19-7-7 7-7" />
        <path d="M19 12H5" />
      </>
    ),
    'arrow-up': (
      <>
        <path d="M12 19V5" />
        <path d="m5 12 7-7 7 7" />
      </>
    ),
    bot: (
      <>
        <rect height="12" rx="4" width="16" x="4" y="8" />
        <path d="M12 8V4H8" />
        <path d="M2 14h2" />
        <path d="M20 14h2" />
        <path d="M9 14h.01" />
        <path d="M15 14h.01" />
      </>
    ),
    boxes: (
      <>
        <path d="M2.97 12.92A2 2 0 0 0 2 14.63v3.24a2 2 0 0 0 .97 1.71l3 1.8a2 2 0 0 0 2.06 0L12 19v-5l-5-3-4.03 2.42Z" />
        <path d="m7 16.5-4.74-2.85" />
        <path d="m7 16.5 5-3" />
        <path d="M7 16.5v5.17" />
        <path d="M12 13.5V19l3.97 2.38a2 2 0 0 0 2.06 0l3-1.8a2 2 0 0 0 .97-1.71v-3.24a2 2 0 0 0-.97-1.71L17 10.5l-5 3Z" />
        <path d="m17 16.5-5-3" />
        <path d="m17 16.5 4.74-2.85" />
        <path d="M17 16.5v5.17" />
        <path d="M7.97 4.42A2 2 0 0 0 7 6.13v4.37l5 3 5-3V6.13a2 2 0 0 0-.97-1.71l-3-1.8a2 2 0 0 0-2.06 0l-3 1.8Z" />
        <path d="M12 8 7.26 5.15" />
        <path d="m12 8 4.74-2.85" />
        <path d="M12 13.5V8" />
      </>
    ),
    brain: (
      <>
        <path d="M12 5a3 3 0 1 0-5.99.2A3 3 0 0 0 4 8a3 3 0 0 0 1.02 2.25A3 3 0 0 0 5 16a3 3 0 0 0 5.78 1.08" />
        <path d="M12 5a3 3 0 1 1 5.99.2A3 3 0 0 1 20 8a3 3 0 0 1-1.02 2.25A3 3 0 0 1 19 16a3 3 0 0 1-5.78 1.08" />
        <path d="M12 5v14" />
        <path d="M8 9h8" />
        <path d="M8 15h8" />
      </>
    ),
    check: <path d="m20 6-11 11-5-5" />,
    'chevron-down': <path d="m6 9 6 6 6-6" />,
    'circle-alert': (
      <>
        <circle cx="12" cy="12" r="10" />
        <path d="M12 8v4" />
        <path d="M12 16h.01" />
      </>
    ),
    'circle-check-big': (
      <>
        <path d="M21.8 8A10 10 0 1 1 17 3.3" />
        <path d="m9 11 3 3L22 4" />
      </>
    ),
    'circle-info': (
      <>
        <circle cx="12" cy="12" r="10" />
        <path d="M12 16v-4" />
        <path d="M12 8h.01" />
      </>
    ),
    'code-2': (
      <>
        <path d="m18 16 4-4-4-4" />
        <path d="m6 8-4 4 4 4" />
        <path d="m14.5 4-5 16" />
      </>
    ),
    eye: (
      <>
        <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" />
        <circle cx="12" cy="12" r="3" />
      </>
    ),
    'eye-off': (
      <>
        <path d="M10.7 5.1A10.9 10.9 0 0 1 12 5c7 0 10 7 10 7a13.2 13.2 0 0 1-1.67 2.68" />
        <path d="M6.61 6.61A13.5 13.5 0 0 0 2 12s3 7 10 7a9.7 9.7 0 0 0 5.39-1.61" />
        <path d="m2 2 20 20" />
        <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24" />
      </>
    ),
    'folder-kanban': (
      <>
        <path d="M3 6.5A2.5 2.5 0 0 1 5.5 4H9l2 2h7.5A2.5 2.5 0 0 1 21 8.5v8A2.5 2.5 0 0 1 18.5 19h-13A2.5 2.5 0 0 1 3 16.5z" />
        <path d="M8 11v4" />
        <path d="M12 10v5" />
        <path d="M16 12v3" />
      </>
    ),
    'git-branch': (
      <>
        <circle cx="18" cy="6" r="3" />
        <circle cx="6" cy="18" r="3" />
        <path d="M6 3v12" />
        <path d="M18 9a9 9 0 0 1-9 9" />
      </>
    ),
    globe: (
      <>
        <circle cx="12" cy="12" r="9" />
        <path d="M3 12h18" />
        <path d="M12 3a14 14 0 0 1 0 18" />
        <path d="M12 3a14 14 0 0 0 0 18" />
      </>
    ),
    image: (
      <>
        <rect height="18" rx="2" ry="2" width="18" x="3" y="3" />
        <circle cx="9" cy="9" r="2" />
        <path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21" />
      </>
    ),
    'list-tree': (
      <>
        <path d="M12 3v5" />
        <path d="M6 8h12" />
        <path d="M6 8v5" />
        <path d="M18 8v5" />
        <path d="M3 13h6v5H3z" />
        <path d="M15 13h6v5h-6z" />
      </>
    ),
    'messages-square': (
      <>
        <path d="M14 9a2 2 0 0 1-2 2H6l-4 4V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2z" />
        <path d="M18 9h2a2 2 0 0 1 2 2v10l-4-4h-6a2 2 0 0 1-2-2v-1" />
      </>
    ),
    minus: <path d="M5 12h14" />,
    paperclip: <path d="m21.4 11.6-8.6 8.6a5 5 0 0 1-7.1-7.1l9.2-9.2a3.3 3.3 0 0 1 4.7 4.7l-9.2 9.2a1.7 1.7 0 1 1-2.4-2.4l8.6-8.6" />,
    'panel-left-close': (
      <>
        <rect height="18" rx="3" width="18" x="3" y="3" />
        <path d="M9 3v18" />
        <path d="m16 9-3 3 3 3" />
      </>
    ),
    'panel-left-open': (
      <>
        <rect height="18" rx="3" width="18" x="3" y="3" />
        <path d="M9 3v18" />
        <path d="m13 9 3 3-3 3" />
      </>
    ),
    'plug-zap': (
      <>
        <path d="m13 2-2 6h4l-2 6" />
        <path d="M12 22v-5" />
        <path d="M9 8V5" />
        <path d="M15 8V5" />
        <path d="M6 8h12v3a6 6 0 0 1-12 0z" />
      </>
    ),
    plus: (
      <>
        <path d="M12 5v14" />
        <path d="M5 12h14" />
      </>
    ),
    'refresh-cw': (
      <>
        <path d="M3 12a9 9 0 0 1 15.5-6.3L21 8" />
        <path d="M21 3v5h-5" />
        <path d="M21 12a9 9 0 0 1-15.5 6.3L3 16" />
        <path d="M3 21v-5h5" />
      </>
    ),
    save: (
      <>
        <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2Z" />
        <path d="M17 21v-8H7v8" />
        <path d="M7 3v5h8" />
      </>
    ),
    search: (
      <>
        <circle cx="11" cy="11" r="7" />
        <path d="m20 20-3.5-3.5" />
      </>
    ),
    'settings-2': (
      <>
        <path d="M20 7h-9" />
        <path d="M14 17H4" />
        <circle cx="7" cy="7" r="3" />
        <circle cx="17" cy="17" r="3" />
      </>
    ),
    'shield-check': (
      <>
        <path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.68 0C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z" />
        <path d="m9 12 2 2 4-4" />
      </>
    ),
    'sliders-horizontal': (
      <>
        <path d="M21 4h-7" />
        <path d="M10 4H3" />
        <path d="M21 12h-9" />
        <path d="M8 12H3" />
        <path d="M21 20h-5" />
        <path d="M12 20H3" />
        <path d="M14 2v4" />
        <path d="M8 10v4" />
        <path d="M16 18v4" />
      </>
    ),
    sparkles: (
      <>
        <path d="M12 3 10 9l-6 2 6 2 2 6 2-6 6-2-6-2-2-6Z" />
        <path d="M19 3v4" />
        <path d="M21 5h-4" />
      </>
    ),
    square: <rect height="14" rx="2" width="14" x="5" y="5" />,
    'square-pen': (
      <>
        <path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
        <path d="M16 5 19 2l3 3-3 3-3-3Z" />
        <path d="M12 12 7 17l-1 3 3-1 5-5" />
      </>
    ),
    'trash-2': (
      <>
        <path d="M3 6h18" />
        <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
        <path d="M19 6 18 20a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
        <path d="M10 11v6" />
        <path d="M14 11v6" />
      </>
    ),
    wrench: (
      <>
        <path d="M14.7 6.3a4 4 0 0 0-5 5L3 18l3 3 6.7-6.7a4 4 0 0 0 5-5l-2.5 2.5-3-3z" />
      </>
    ),
    x: (
      <>
        <path d="M18 6 6 18" />
        <path d="m6 6 12 12" />
      </>
    ),
  };

  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
    >
      {paths[name]}
    </svg>
  );
}

export default IconGlyph;
