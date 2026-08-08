// @ts-check
import tseslint from "typescript-eslint";
import reactPlugin from "eslint-plugin-react";
import reactHooksPlugin from "eslint-plugin-react-hooks";
import prettierConfig from "eslint-config-prettier";

export default tseslint.config(
  {
    ignores: ["dist/", "node_modules/", "scripts/", "browser-tests/"],
  },
  ...tseslint.configs.strictTypeChecked,
  ...tseslint.configs.stylisticTypeChecked,
  reactPlugin.configs.flat["recommended"],
  reactHooksPlugin.configs.flat.recommended,
  prettierConfig,
  {
    languageOptions: {
      parserOptions: {
        project: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    settings: {
      react: {
        version: "detect",
      },
    },
    rules: {
      "react/react-in-jsx-scope": "off",
    },
  },
  // Config files at root are outside the tsconfig project; disable type-aware rules for them.
  {
    files: ["*.js", "playwright.config.ts", "vite.config.ts"],
    ignores: ["src/**"],
    ...tseslint.configs.disableTypeChecked,
  },
  // Non-null assertions and empty arrow functions are idiomatic in mock test setup.
  {
    files: ["**/*.test.ts", "**/*.test.tsx"],
    rules: {
      "@typescript-eslint/no-non-null-assertion": "off",
      "@typescript-eslint/no-empty-function": ["error", { allow: ["arrowFunctions"] }],
    },
  },
);
