# Celestial Launcher 插件开发

本目录是编写 Celestial Launcher 插件的起点。插件是加载进启动器界面的一段
**已编译的 JavaScript**，通过启动器提供的 `api` 对象扩展功能，能力由权限系统
限定。

## 构建插件

把 `template/` 拷到**仓库之外**的任意位置。它是独立项目，留在仓库里会被父级
pnpm workspace 接管：

```bash
cp -r plugin-sdk/template ~/my-plugin
cd ~/my-plugin
pnpm install
pnpm build
```

> 若坚持在仓库内构建，必须用 `pnpm install --ignore-workspace`，否则 pnpm 会去
> 装整个 workspace，模板自己的 `node_modules` 建不出来，`vite` 命令也就找不到。

产物是 `dist/index.js`（外加 sourcemap）。

## 安装到启动器

**关键：插件根目录 = 包含 `manifest.json` 的那个文件夹，它的名字必须等于
manifest 里的 `id`。**

正确：

```
%APPDATA%\CelestialLauncher\plugins\com.example.my-plugin\
├── manifest.json          ← 必须在这一层
├── dist\
│   └── index.js           ← manifest 的 entry 写成 "dist/index.js"
└── build\ types\ src\ ...  ← 源码一并放着无妨
```

错误（把 `dist\` 当成了插件根）：

```
%APPDATA%\CelestialLauncher\plugins\com.example.my-plugin\dist\manifest.json
```

判断方法很简单：**哪个文件夹里有 `manifest.json`，那个文件夹就是插件根**。
manifest 的 `entry` 是相对这个根解析的。

放好后重启启动器，或在设置里开启插件热重载配合 `pnpm watch`，改代码即时生效。

## 目录说明

| 路径 | 作用 |
|---|---|
| `template/src/index.ts` | 入口，导出 `activate(api)` |
| `template/src/*.vue` | Vue 单文件组件（可选） |
| `template/manifest.json` | 插件清单：id、权限等 |
| `template/build/celestial-vue.mjs` | 构建插件：把 `vue` 导入指向启动器实例 |
| `template/types/celestial-plugin.d.ts` | 宿主 API 类型定义（自动补全） |
| `template/build/.generated/` | 构建时生成的 Vue shim，勿提交 |

## 关于 Vue

插件运行在启动器页面里，和启动器**共用同一个 Vue 实例**。你可以：

- 直接写 `.vue` 单文件组件，或 `import { ref, h } from 'vue'`；
- 也可以用 `activate` 传入的 `api.vue`。

两者最终都解析到启动器的 Vue：构建时 `build/celestial-vue.mjs` 会生成一个 shim
并把所有 `vue` 导入指向它，shim 再从启动器暴露的全局按名取用。

**不要**让插件自带打包一份 Vue——那会产生启动器渲染不了的另一个实例，组件挂不上。

构建后可以自查：在 `dist/index.js` 里搜 `from"vue"` / `from 'vue'`，
**不应该**搜到裸的 vue 导入；应该只看到从 `globalThis.__CELESTIAL_PLUGIN_VUE__`
读取的代码。

## 权限

插件默认**什么都不能做**。每项能力都要在 `manifest.json` 的 `permissions` 里
显式声明：

| 权限 | 风险 | 能力 |
|---|---|---|
| `style` | 低 | 注入 CSS |
| `storage` | 低 | 读写自己的数据目录 |
| `slot:<位置>` | 低 | 往插槽放 UI（如 `slot:sidebar.top`） |
| `route` | 低 | 注册自定义页面 |
| `event:<类型>` | 低 | 订阅应用事件（如 `event:instance`） |
| `hostapi:<名称>` | 高 | 调用宿主接口（如 `hostapi:instance.list`） |
| `region:<区域>` | 高 | 重排结构区域（`navbar`/`topbar`/`sidebar`） |
| `network:<域名>` | 高 | 访问指定域名 |

**低风险**权限装上即自动授予；**高风险**权限需要用户在插件管理界面手动批准，
未批准前对应调用会抛错。所以用高风险能力时要优雅降级，别让插件崩掉。

可用的插槽：`topbar.left`、`topbar.center`、`topbar.right`、`navbar.bottom`、
`sidebar.top`、`sidebar.bottom`。

## API 速览

```ts
export async function activate(api: PluginHostApi) {
  api.styles.add(css)                        // 注入样式
  api.slots.add('sidebar.top', { id, component })   // 加 UI
  api.routes.add({ path, component })        // 加页面
  api.router.push('/some/path')              // 导航
  await api.storage.set('key', 'value')      // 持久化
  api.events.on('instance', (payload) => {}) // 监听事件
  await api.hostApi.call('instance.list')    // 调宿主接口（高风险）
  api.regions.get('topbar')                  // 拿区域容器（高风险）
}
```

完整类型见 `template/types/celestial-plugin.d.ts`。
