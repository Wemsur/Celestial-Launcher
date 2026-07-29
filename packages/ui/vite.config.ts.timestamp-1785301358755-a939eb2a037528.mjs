// vite.config.ts
import path from "node:path";
import vue from "file:///C:/Users/iexam/github-project/CelestialLauncher/node_modules/.pnpm/@vitejs+plugin-vue@5.2.4_vi_224b2095dd9e600ad256f085d32f1f98/node_modules/@vitejs/plugin-vue/dist/index.mjs";
import { defineConfig } from "file:///C:/Users/iexam/github-project/CelestialLauncher/node_modules/.pnpm/vite@5.4.21_@types+node@24._2b8b6264ee1f7a19f82bb7b3cd5e4f3b/node_modules/vite/dist/node/index.js";
import svgLoader from "file:///C:/Users/iexam/github-project/CelestialLauncher/node_modules/.pnpm/vite-svg-loader@5.1.0_vue@3.5.27_typescript@5.9.3_/node_modules/vite-svg-loader/index.js";
var __vite_injected_original_dirname = "C:\\Users\\iexam\\github-project\\CelestialLauncher\\packages\\ui";
var vite_config_default = defineConfig({
  plugins: [
    vue(),
    svgLoader({
      svgoConfig: {
        plugins: [
          {
            name: "preset-default",
            params: {
              overrides: {
                removeViewBox: false,
                cleanupIds: {
                  minify: false
                }
              }
            }
          }
        ]
      }
    })
  ],
  cacheDir: ".vite",
  resolve: {
    alias: {
      "@": path.resolve(__vite_injected_original_dirname, "src"),
      "#ui": path.resolve(__vite_injected_original_dirname, "src"),
      "@modrinth/api-client": path.resolve(__vite_injected_original_dirname, "../api-client/src/index.ts")
    }
  },
  build: {
    lib: {
      entry: path.resolve(__vite_injected_original_dirname, "index.ts"),
      name: "ModrinthUI",
      formats: ["es"],
      fileName: "index"
    },
    rollupOptions: {
      external: ["vue"]
    }
  }
});
export {
  vite_config_default as default
};
//# sourceMappingURL=data:application/json;base64,ewogICJ2ZXJzaW9uIjogMywKICAic291cmNlcyI6IFsidml0ZS5jb25maWcudHMiXSwKICAic291cmNlc0NvbnRlbnQiOiBbImNvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9kaXJuYW1lID0gXCJDOlxcXFxVc2Vyc1xcXFxpZXhhbVxcXFxnaXRodWItcHJvamVjdFxcXFxDZWxlc3RpYWxMYXVuY2hlclxcXFxwYWNrYWdlc1xcXFx1aVwiO2NvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9maWxlbmFtZSA9IFwiQzpcXFxcVXNlcnNcXFxcaWV4YW1cXFxcZ2l0aHViLXByb2plY3RcXFxcQ2VsZXN0aWFsTGF1bmNoZXJcXFxccGFja2FnZXNcXFxcdWlcXFxcdml0ZS5jb25maWcudHNcIjtjb25zdCBfX3ZpdGVfaW5qZWN0ZWRfb3JpZ2luYWxfaW1wb3J0X21ldGFfdXJsID0gXCJmaWxlOi8vL0M6L1VzZXJzL2lleGFtL2dpdGh1Yi1wcm9qZWN0L0NlbGVzdGlhbExhdW5jaGVyL3BhY2thZ2VzL3VpL3ZpdGUuY29uZmlnLnRzXCI7aW1wb3J0IHBhdGggZnJvbSAnbm9kZTpwYXRoJ1xuXG5pbXBvcnQgdnVlIGZyb20gJ0B2aXRlanMvcGx1Z2luLXZ1ZSdcbmltcG9ydCB7IGRlZmluZUNvbmZpZyB9IGZyb20gJ3ZpdGUnXG5pbXBvcnQgc3ZnTG9hZGVyIGZyb20gJ3ZpdGUtc3ZnLWxvYWRlcidcblxuZXhwb3J0IGRlZmF1bHQgZGVmaW5lQ29uZmlnKHtcblx0cGx1Z2luczogW1xuXHRcdHZ1ZSgpLFxuXHRcdHN2Z0xvYWRlcih7XG5cdFx0XHRzdmdvQ29uZmlnOiB7XG5cdFx0XHRcdHBsdWdpbnM6IFtcblx0XHRcdFx0XHR7XG5cdFx0XHRcdFx0XHRuYW1lOiAncHJlc2V0LWRlZmF1bHQnLFxuXHRcdFx0XHRcdFx0cGFyYW1zOiB7XG5cdFx0XHRcdFx0XHRcdG92ZXJyaWRlczoge1xuXHRcdFx0XHRcdFx0XHRcdHJlbW92ZVZpZXdCb3g6IGZhbHNlLFxuXHRcdFx0XHRcdFx0XHRcdGNsZWFudXBJZHM6IHtcblx0XHRcdFx0XHRcdFx0XHRcdG1pbmlmeTogZmFsc2UsXG5cdFx0XHRcdFx0XHRcdFx0fSxcblx0XHRcdFx0XHRcdFx0fSxcblx0XHRcdFx0XHRcdH0sXG5cdFx0XHRcdFx0fSxcblx0XHRcdFx0XSxcblx0XHRcdH0sXG5cdFx0fSksXG5cdF0sXG5cdGNhY2hlRGlyOiAnLnZpdGUnLFxuXG5cdHJlc29sdmU6IHtcblx0XHRhbGlhczoge1xuXHRcdFx0J0AnOiBwYXRoLnJlc29sdmUoX19kaXJuYW1lLCAnc3JjJyksXG5cdFx0XHQnI3VpJzogcGF0aC5yZXNvbHZlKF9fZGlybmFtZSwgJ3NyYycpLFxuXHRcdFx0J0Btb2RyaW50aC9hcGktY2xpZW50JzogcGF0aC5yZXNvbHZlKF9fZGlybmFtZSwgJy4uL2FwaS1jbGllbnQvc3JjL2luZGV4LnRzJyksXG5cdFx0fSxcblx0fSxcblxuXHRidWlsZDoge1xuXHRcdGxpYjoge1xuXHRcdFx0ZW50cnk6IHBhdGgucmVzb2x2ZShfX2Rpcm5hbWUsICdpbmRleC50cycpLFxuXHRcdFx0bmFtZTogJ01vZHJpbnRoVUknLFxuXHRcdFx0Zm9ybWF0czogWydlcyddLFxuXHRcdFx0ZmlsZU5hbWU6ICdpbmRleCcsXG5cdFx0fSxcblx0XHRyb2xsdXBPcHRpb25zOiB7XG5cdFx0XHRleHRlcm5hbDogWyd2dWUnXSxcblx0XHR9LFxuXHR9LFxufSlcbiJdLAogICJtYXBwaW5ncyI6ICI7QUFBaVgsT0FBTyxVQUFVO0FBRWxZLE9BQU8sU0FBUztBQUNoQixTQUFTLG9CQUFvQjtBQUM3QixPQUFPLGVBQWU7QUFKdEIsSUFBTSxtQ0FBbUM7QUFNekMsSUFBTyxzQkFBUSxhQUFhO0FBQUEsRUFDM0IsU0FBUztBQUFBLElBQ1IsSUFBSTtBQUFBLElBQ0osVUFBVTtBQUFBLE1BQ1QsWUFBWTtBQUFBLFFBQ1gsU0FBUztBQUFBLFVBQ1I7QUFBQSxZQUNDLE1BQU07QUFBQSxZQUNOLFFBQVE7QUFBQSxjQUNQLFdBQVc7QUFBQSxnQkFDVixlQUFlO0FBQUEsZ0JBQ2YsWUFBWTtBQUFBLGtCQUNYLFFBQVE7QUFBQSxnQkFDVDtBQUFBLGNBQ0Q7QUFBQSxZQUNEO0FBQUEsVUFDRDtBQUFBLFFBQ0Q7QUFBQSxNQUNEO0FBQUEsSUFDRCxDQUFDO0FBQUEsRUFDRjtBQUFBLEVBQ0EsVUFBVTtBQUFBLEVBRVYsU0FBUztBQUFBLElBQ1IsT0FBTztBQUFBLE1BQ04sS0FBSyxLQUFLLFFBQVEsa0NBQVcsS0FBSztBQUFBLE1BQ2xDLE9BQU8sS0FBSyxRQUFRLGtDQUFXLEtBQUs7QUFBQSxNQUNwQyx3QkFBd0IsS0FBSyxRQUFRLGtDQUFXLDRCQUE0QjtBQUFBLElBQzdFO0FBQUEsRUFDRDtBQUFBLEVBRUEsT0FBTztBQUFBLElBQ04sS0FBSztBQUFBLE1BQ0osT0FBTyxLQUFLLFFBQVEsa0NBQVcsVUFBVTtBQUFBLE1BQ3pDLE1BQU07QUFBQSxNQUNOLFNBQVMsQ0FBQyxJQUFJO0FBQUEsTUFDZCxVQUFVO0FBQUEsSUFDWDtBQUFBLElBQ0EsZUFBZTtBQUFBLE1BQ2QsVUFBVSxDQUFDLEtBQUs7QUFBQSxJQUNqQjtBQUFBLEVBQ0Q7QUFDRCxDQUFDOyIsCiAgIm5hbWVzIjogW10KfQo=
