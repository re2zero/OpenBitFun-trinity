# GPUI 壳（gpui 分支）现状与开发底账

> 2026-10-25 新起点。gpui-frontend 时代的计划/整改文档已提炼归档，本文只记现状与
> 对后续开发有用的事实。1:1 目标：以 web-ui（Tauri 桌面）为唯一参照，把壳界面做到 1:1。

## 1. 基线与上游同步

- 分支 `gpui` = upstream/main（`1f78b811c`）+ 两笔：
  - `bd45e78ce` feat(core): 壳所需的平台支撑（ui-contributions 通道、git/pty/filesystem
    服务扩展、function-agents、insights 别名修复、core ftl 1331 键×3）
  - `409ed245d` feat(gpui): 壳应用 `src/apps/gpui`（crate `openbitfun-gpui`）及其构建接线
- 上游同步：`git fetch upstream && git merge upstream/main`。本分支不携带 trinity 与
  desktop 宿主翻转，预期冲突面 ≈0；`Cargo.lock` 与 `interactive-capabilities` 生成物
  冲突跑对应再生成脚本收口。
- 壳对 core 的触点（上游若重构这些文件需人工核对）：
  `services-integrations/src/git/runtime_port.rs`、`services/terminal/src/pty/*`、
  `services-core/src/filesystem/tree.rs`、`contracts/runtime-ports/src/git_port.rs`、
  `contracts/product-domains/src/plugin_ui.rs`、`assembly/core/src/plugin_ui*.rs`、
  `assembly/core/src/service/i18n/service.rs`（`register_locale_resource`，宿主自有
  ftl 注册口）。以上均为上游 PR 回流候选。
- 壳自足面：i18n 文案在 `src/apps/gpui/locales/*.ftl`（`gpui-shell-*` 键，1140×3，
  init 时经 `register_locale_resource` 注册进 core Fluent 服务）；core 侧
  `assembly/core/locales` 与上游逐字节一致。壳内无 Trinity 面（场景/引擎页/侧栏
  灯/传输对话框已全部移除；`ai.memory` 记忆设置页保留——core 共享配置，web 同款）。

## 2. 壳能力面（已交付）

会话列表/多 tab/流式转写（工具卡五态、子代理身份与折叠组、turn footer meta）、权限
应答闭环、真 PTY 终端面板、Git 六视图（status/diff/branches/graph/snapshot/reset）、
设置九页、技能/智能体/洞察/任务看板/记忆/MCP（含远程 OAuth）/小应用与扩展（插件 UI
贡献场景 R7）、托盘与防休眠、搜索联邦、自更新与打包（tar.gz+deb，
`scripts/gpui-package.mjs`）。

真机验收（2026-10-25，Xvfb 口径）已通过，55 帧存 /tmp（临时）。布局与原壳同构，
深色主背景 `#0e0e10`（原壳 `#161616`），主题 6 档。

## 3. 界面 1:1 对照要点

- 参照真相源：web-ui 桌面拓扑（无 TitleBar/无横贯底栏，NavBar 45 + SceneTopBar 45 +
  footer ≈35）。改布局前先对 web 考据，双端并排（`desktop:dev` + 壳）是唯一有效对照法。
- **壳侧独有面（web 无对应物，勿删）**：插件 UI 贡献场景（`SceneId::External` +
  `plugin_ui.rs` 渲染器 + core `host.ui.*` 通道，设计见
  `docs/architecture/extensions/plugin-ui-contributions-design.md`）；会话区头固定槽；
  设置导航覆盖侧栏且导航行常驻（刻意分歧）。
- **web 死面考据（不作为 1:1 目标）**：Git Branches/Graph 在 web 无 UI 入口（壳已实现
  属超出 web）；`shell` 场景（ShellNav）不可达；`file-viewer` 仅为再导出；
  `trinity-memory` tab 是纯类型残留；终端无专属 nav。
- 布局矩阵与签字截图已废弃；布局证据以真机双端并排帧为准（拍摄配方：Xvfb 冷门私有号
  + 隔离根 + wmctrl 不可用改 xdotool windowsize/windowmove）。

## 4. 已知问题底账（待点名修复）

1. 发送失败时 composer 输入不回滚（最伤使用感）。
2. 「检查更新」旁 self-update 拒替说明为英文长文案（i18n 漏网）。
3. 无模型时 composer 档位名裸英文 `primary`（tooltip 已中文）。
4. 防休眠开关视觉回填秒级滞后（后端正确，gpui-kit 开关异步回填链）。

未立项：pdf 预览（G-FL3）、右面板终端列表、IMM 输入法链路真机验证、托盘/关闭行为/
更新全链的桌面真机复验（宜装 deb 自验）。

## 5. 验证口径

```bash
# 2026-11 壳已拆分为 src/apps/gpui/crates/ 下的 gpui-* crate（bin 包名
# openbitfun-app），不再有"单 crate 50min / rustc 峰值 ~48GB"的编译形态；
# 各 crate 独立编译、峰值内存回归常量级。
cargo check -p openbitfun-app --lib --tests
cargo test -p gpui-shell --lib
pnpm run fmt:rs                                 # 只格式化改动文件
node --test scripts/gpui-package.test.mjs scripts/audit-rust-surface-colors.test.mjs
node scripts/audit-frontend-colors.mjs --surface gpui
```

冒烟要点：隔离根启动（默认隔离到 `~/.local/share/openbitfun-app`）；杀壳扫
`/proc/*/exe` 且模式必须带尾星（二进制替换后 readlink 带 `(deleted)`）；Xvfb 用冷门
私有显示器号；gpui-kit 开关的视觉回填走异步事件链，自动化采样要移开鼠标+延时。

## 6. 契约注意

- i18n 键前缀 `gpui-shell-*`：crate 已改名 `openbitfun-gpui`，键前缀保留（历史契约）。
  ftl 在 `src/apps/gpui/locales/`（1140 键×3 平齐），改动后三语键数必须对齐；
  core 侧 locale 与上游一致，勿把壳键写回 core。
- 主题 color surface id 为 `gpui`（`scripts/frontend-color-surface-registry.json`、
  baseline `scripts/theme-color-governance-baseline.gpui.json`）。
- 打包：`pnpm run gpui:build && pnpm run gpui:package`；deb glibc 地板=本机构建环境
  （≥2.38），与 CI 地板（2.35）不同，对外分发以 CI 产物为准。
