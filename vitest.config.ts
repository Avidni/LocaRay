import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    exclude: ["node_modules/**", "src-tauri/**", "tests/e2e/**"],
    setupFiles: ["./src/test-setup.ts"],
  },
});
