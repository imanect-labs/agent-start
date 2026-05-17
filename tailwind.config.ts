import type { Config } from "tailwindcss";
import { heroui } from "@heroui/react";

const config: Config = {
  content: [
    "./app/**/*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
    "./node_modules/@heroui/theme/dist/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {},
  },
  darkMode: "class",
  // heroui plugin bundles its own tailwindcss types and conflicts with the
  // root tailwindcss types — safe to ignore at the type level.
  plugins: [heroui() as unknown as Config["plugins"] extends (infer U)[] ? U : never],
};

export default config;
