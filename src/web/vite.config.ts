import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      "/device-tokens": "http://127.0.0.1:8080",
      "/exports": "http://127.0.0.1:8080",
      "/health": "http://127.0.0.1:8080",
      "/model-packs": "http://127.0.0.1:8080",
      "/people": "http://127.0.0.1:8080",
      "/ready": "http://127.0.0.1:8080",
      "/search": "http://127.0.0.1:8080",
      "/trash": "http://127.0.0.1:8080",
      "/uploads": "http://127.0.0.1:8080",
      "/auth": "http://127.0.0.1:8080",
      "/assets": "http://127.0.0.1:8080",
      "/setup": "http://127.0.0.1:8080",
      "/sessions": "http://127.0.0.1:8080"
    }
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    setupFiles: "./src/test/setup.ts"
  }
});
