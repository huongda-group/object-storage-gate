import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";
import { tanstackRouter } from "@tanstack/router-plugin/rspack";

// https://rsbuild.dev/guide/basic/configure-rsbuild
export default defineConfig({
  plugins: [pluginReact()],
  source: {
    entry: { index: "./src/main.tsx" },
  },
  tools: {
    rspack: {
      plugins: [tanstackRouter({ target: "react", autoCodeSplitting: true })],
    },
  },
  html: {
    favicon: "src/assets/favicon.ico",
    title: "Object Storage Gate",
  },
  server: {
    proxy: {
      "/api": {
        // `localhost`, not 127.0.0.1: config/development.yaml binds the server to
        // `localhost`, which resolves to ::1 on macOS.
        target: "http://localhost:5150",
        changeOrigin: true,
        secure: false,
      },
    },
  },
});
