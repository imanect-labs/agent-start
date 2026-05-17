import type { Config } from "tailwindcss";

const rgb = (token: string) => `rgb(var(--${token}) / <alpha-value>)`;

const config: Config = {
  darkMode: "class",
  content: ["./app/**/*.{ts,tsx}", "./components/**/*.{ts,tsx}"],
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
