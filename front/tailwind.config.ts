import type { Config } from "tailwindcss";

const rgb = (token: string) => `rgb(var(--${token}) / <alpha-value>)`;

const config: Config = {
  darkMode: "class",
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        app: rgb("app"),
        surface: {
          DEFAULT: rgb("surface"),
          elev: rgb("surface-elev"),
          muted: rgb("surface-muted"),
          sunken: rgb("surface-sunken"),
        },
        line: {
          DEFAULT: rgb("border"),
          strong: rgb("border-strong"),
        },
        fg: {
          DEFAULT: rgb("fg"),
          muted: rgb("fg-muted"),
          subtle: rgb("fg-subtle"),
          faint: rgb("fg-faint"),
        },
        accent: {
          DEFAULT: rgb("accent"),
          hover: rgb("accent-hover"),
          fg: rgb("accent-fg"),
          soft: rgb("accent-soft"),
          subtle: rgb("accent-subtle"),
        },
        neutral: {
          strong: rgb("neutral-strong"),
          "strong-fg": rgb("neutral-strong-fg"),
        },
        danger: {
          DEFAULT: rgb("danger"),
          fg: rgb("danger-fg"),
          soft: rgb("danger-soft"),
        },
        warn: rgb("warn"),
        success: rgb("success"),
      },
      ringColor: {
        DEFAULT: rgb("ring"),
      },
      fontSize: {
        // Unified type scale (retires arbitrary text-[Npx] usages).
        // Each entry: [size, lineHeight].
        "2xs": ["0.6875rem", { lineHeight: "1rem" }], // 11 / 16
        xs: ["0.75rem", { lineHeight: "1rem" }], // 12 / 16
        sm: ["0.84375rem", { lineHeight: "1.25rem" }], // 13.5 / 20
        base: ["0.9375rem", { lineHeight: "1.5rem" }], // 15 / 24
        lg: ["1.125rem", { lineHeight: "1.625rem" }], // 18 / 26
        xl: ["1.375rem", { lineHeight: "1.875rem" }], // 22 / 30
        "2xl": ["1.75rem", { lineHeight: "2.125rem" }], // 28 / 34
      },
      boxShadow: {
        xs: "var(--shadow-xs)",
        sm: "var(--shadow-sm)",
        md: "var(--shadow-md)",
        lg: "var(--shadow-lg)",
      },
      borderRadius: {
        sm: "var(--radius-sm)",
        DEFAULT: "var(--radius)",
        lg: "var(--radius-lg)",
        xl: "var(--radius-xl)",
      },
      fontFamily: {
        sans: [
          "ui-sans-serif",
          "system-ui",
          "-apple-system",
          "Segoe UI",
          "Roboto",
          "Helvetica Neue",
          "Arial",
          "Noto Sans JP",
          "sans-serif",
        ],
        mono: [
          "ui-monospace",
          "SFMono-Regular",
          "Menlo",
          "Monaco",
          "Consolas",
          "Liberation Mono",
          "Courier New",
          "monospace",
        ],
      },
    },
  },
};

export default config;
