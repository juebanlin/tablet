import { defineConfig } from "vite";

// 产物直接落到 Tauri 的 frontendDist（ui/）下的 univer/ 子目录；
// 由 crates/app-tauri/build.rs 检查是否已构建、main.rs 用 WebviewUrl::App 打开。
export default defineConfig({
  root: "src",
  base: "./",
  build: {
    outDir: "../../ui/univer",
    emptyOutDir: true,
    // Univer 是插件集合，单块体积大是预期；不按 500KB 警告拆包。
    chunkSizeWarningLimit: 8000,
  },
});
