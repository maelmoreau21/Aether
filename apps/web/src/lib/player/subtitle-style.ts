export interface SubtitleStyle {
  fontSize?: 'small' | 'medium' | 'large';
  fontFamily?: 'sans-serif' | 'serif' | 'monospace';
  textColor?: string;
  backgroundColor?: string;
  backgroundOpacity?: number; // 0 to 100
  textShadow?: 'none' | 'outline' | 'drop-shadow';
}

export const DEFAULT_SUBTITLE_STYLE: Required<SubtitleStyle> = {
  fontSize: 'medium',
  fontFamily: 'sans-serif',
  textColor: '#ffffff',
  backgroundColor: '#000000',
  backgroundOpacity: 0, // no ugly box background by default!
  textShadow: 'drop-shadow'
};

export function sanitizeSubtitleStyle(input?: unknown): SubtitleStyle {
  if (!input || typeof input !== 'object') return {};
  const data = input as Record<string, unknown>;
  const style: SubtitleStyle = {};

  if (data.fontSize === 'small' || data.fontSize === 'medium' || data.fontSize === 'large') {
    style.fontSize = data.fontSize;
  }
  if (data.fontFamily === 'sans-serif' || data.fontFamily === 'serif' || data.fontFamily === 'monospace') {
    style.fontFamily = data.fontFamily;
  }
  if (typeof data.textColor === 'string' && /^#[0-9a-fA-F]{6}$/.test(data.textColor)) {
    style.textColor = data.textColor;
  }
  if (typeof data.backgroundColor === 'string' && /^#[0-9a-fA-F]{6}$/.test(data.backgroundColor)) {
    style.backgroundColor = data.backgroundColor;
  }
  if (typeof data.backgroundOpacity === 'number' && Number.isFinite(data.backgroundOpacity) && data.backgroundOpacity >= 0 && data.backgroundOpacity <= 100) {
    style.backgroundOpacity = Math.round(data.backgroundOpacity);
  }
  if (data.textShadow === 'none' || data.textShadow === 'outline' || data.textShadow === 'drop-shadow') {
    style.textShadow = data.textShadow;
  }

  return style;
}

export function subtitleStyleToCss(style?: SubtitleStyle): {
  color: string;
  fontSize: string;
  fontFamily: string;
  backgroundColor: string;
  textShadow: string;
} {
  const merged = { ...DEFAULT_SUBTITLE_STYLE, ...sanitizeSubtitleStyle(style) };

  const fontSizeMap = {
    small: 'clamp(15px, 1.6vw, 22px)',
    medium: 'clamp(18px, 2.2vw, 30px)',
    large: 'clamp(22px, 2.8vw, 38px)'
  };

  const fontFamilyMap = {
    'sans-serif': 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
    'serif': 'Georgia, "Times New Roman", serif',
    'monospace': '"SF Mono", "Cascadia Code", Consolas, monospace'
  };

  const textShadowMap = {
    'none': 'none',
    'outline': '-1px -1px 0 #000, 1px -1px 0 #000, -1px 1px 0 #000, 1px 1px 0 #000, 0 2px 4px rgba(0,0,0,0.8)',
    'drop-shadow': '0 2px 4px rgba(0,0,0,0.9), 0 0 2px rgba(0,0,0,0.8)'
  };

  // Convert hex color + opacity percentage to rgba
  let bgRgba = 'transparent';
  if (merged.backgroundOpacity > 0) {
    const hex = merged.backgroundColor.replace('#', '');
    const r = parseInt(hex.substring(0, 2), 16);
    const g = parseInt(hex.substring(2, 4), 16);
    const b = parseInt(hex.substring(4, 6), 16);
    const a = merged.backgroundOpacity / 100;
    bgRgba = `rgba(${r}, ${g}, ${b}, ${a})`;
  }

  return {
    color: merged.textColor,
    fontSize: fontSizeMap[merged.fontSize],
    fontFamily: fontFamilyMap[merged.fontFamily],
    backgroundColor: bgRgba,
    textShadow: textShadowMap[merged.textShadow]
  };
}
