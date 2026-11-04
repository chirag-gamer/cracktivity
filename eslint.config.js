import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/", "src-tauri/", "node_modules/"] },
  ...tseslint.configs.recommended,
);
