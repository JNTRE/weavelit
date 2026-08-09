/**
 * Weavelit Design System Tokens - TypeScript Definitions
 *
 * Type-safe token definitions for programmatic access.
 * These mirror the CSS custom properties in tokens.css.
 */

export interface ColorTokens {
  textPrimary: string;
  textSecondary: string;
  textTertiary: string;
  textDisabled: string;
  background: string;
  backgroundSubtle: string;
  backgroundHover: string;
  border: string;
  borderStrong: string;
  accentPrimary: string;
  accentPrimaryHover: string;
  accentSecondary: string;
  statusSuccess: string;
  statusWarning: string;
  statusError: string;
  statusInfo: string;
}

export interface SpacingTokens {
  xs: string;
  sm: string;
  md: string;
  lg: string;
  xl: string;
  "2xl": string;
}

export interface TypographyTokens {
  fontFamilyBase: string;
  fontFamilyMono: string;
  fontSize: {
    xs: string;
    sm: string;
    body: string;
    lg: string;
    headingSm: string;
    headingMd: string;
    headingLg: string;
  };
  fontWeight: {
    regular: string;
    medium: string;
    semibold: string;
    bold: string;
  };
  lineHeight: {
    tight: string;
    normal: string;
    relaxed: string;
  };
}

export interface BorderTokens {
  radiusNone: string;
  radiusSm: string;
  radiusMd: string;
  radiusLg: string;
  radiusFull: string;
}

export interface ShadowTokens {
  sm: string;
  md: string;
  lg: string;
}

export interface TransitionTokens {
  durationFast: string;
  durationNormal: string;
  durationSlow: string;
  timing: string;
}

export interface DesignTokens {
  colors: ColorTokens;
  spacing: SpacingTokens;
  typography: TypographyTokens;
  borders: BorderTokens;
  shadows: ShadowTokens;
  transitions: TransitionTokens;
}

/**
 * Default token values (light mode).
 * These values match the CSS custom properties in tokens.css.
 */
export const tokens: DesignTokens = {
  colors: {
    textPrimary: "#1a1a1a",
    textSecondary: "#666666",
    textTertiary: "#999999",
    textDisabled: "#cccccc",
    background: "#ffffff",
    backgroundSubtle: "#f9f9f9",
    backgroundHover: "#f5f5f5",
    border: "#e0e0e0",
    borderStrong: "#cccccc",
    accentPrimary: "#0066cc",
    accentPrimaryHover: "#0052a3",
    accentSecondary: "#6b7280",
    statusSuccess: "#10b981",
    statusWarning: "#f59e0b",
    statusError: "#ef4444",
    statusInfo: "#3b82f6",
  },
  spacing: {
    xs: "0.25rem",
    sm: "0.5rem",
    md: "1rem",
    lg: "1.5rem",
    xl: "2rem",
    "2xl": "3rem",
  },
  typography: {
    fontFamilyBase: "system-ui, -apple-system, sans-serif",
    fontFamilyMono: "'Monaco', 'Menlo', 'Ubuntu Mono', monospace",
    fontSize: {
      xs: "0.75rem",
      sm: "0.875rem",
      body: "1rem",
      lg: "1.125rem",
      headingSm: "1.25rem",
      headingMd: "1.5rem",
      headingLg: "1.875rem",
    },
    fontWeight: {
      regular: "400",
      medium: "500",
      semibold: "600",
      bold: "700",
    },
    lineHeight: {
      tight: "1.2",
      normal: "1.5",
      relaxed: "1.75",
    },
  },
  borders: {
    radiusNone: "0",
    radiusSm: "0.25rem",
    radiusMd: "0.375rem",
    radiusLg: "0.5rem",
    radiusFull: "9999px",
  },
  shadows: {
    sm: "0 1px 2px 0 rgba(0, 0, 0, 0.05)",
    md: "0 4px 6px -1px rgba(0, 0, 0, 0.1)",
    lg: "0 10px 15px -3px rgba(0, 0, 0, 0.1)",
  },
  transitions: {
    durationFast: "150ms",
    durationNormal: "300ms",
    durationSlow: "500ms",
    timing: "cubic-bezier(0.4, 0, 0.2, 1)",
  },
};
