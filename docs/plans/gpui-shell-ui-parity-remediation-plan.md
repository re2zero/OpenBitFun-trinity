# GPUI 壳 × 原桌面（web-ui）界面 1:1 差距盘点与整改计划

盘点日期 **2026-10-15**。壳基线 `fa36bb6e1`（工作区另有未提交的并发规模档，见 §6.1 W-4）；
原桌面（web-ui）在本分支内基线 `a537f5c70`。真相源：
`src/web-ui/src/app/scenes/registry.ts`、`.../settings/settingsRegistry.ts`、`.../nav-registry.ts`、
`src/web-ui/src/app/components/SceneBar/types.ts`、`src/apps/gpui-shell/src/{tabs,shell,settings,manage,git,files,search,tasks,insights,trinity}.rs`。

配套文档：`gpui-shell-parity-and-switch-plan.md`（能力域矩阵 §2 40 行 + 布局矩阵 §2.3 18 行 + 切换机制）。
**本文是「界面逐一」粒度的差距盘点与分档整改计划**：把 parity 的两套矩阵落到每个具体界面/页面，
并把 2026-10-14 之后的新增量与历代遗留清单（T3 大项、小扫尾、R7c 尾货、并发规模档）合并成一份可勾选的计划。

## 0. 口径与方法

- **1:1 尺度**（沿用 parity §2.3）：布局结构、信息架构、交互位、视觉语言（同 design token 下）；
  **不是像素级复刻**（渲染器不同，字形/亚像素差异天然存在）。
- **状态四态**：`同构` / `子集`（差额逐条列出）/ `缺` / `不补`（已拍板，注明来源）。
- **方法**：双端源码逐界面核对（web 渲染锚点 × 壳实现锚点/缺席证据），缺口逐条给整改动作与档位；
  视觉类改动验收沿用既有口径（Xvfb 冒烟 0 panic/0 缺资产 + 双端并排签字 + 证据归档）。
- **死面排除原则**：web 侧注册但**当前不可达**的面不作为 1:1 整改目标（§1.6 给出证据）。
  为这些面补实现属于「超过 web 现状」，需公子点名，否则不得按「web 有」立项。
- **形态差异只记一处**：同一能力的挂载形态分歧（如终端 = 底部停靠面板 vs 主 tab）
  在表中合并为一行，不重复计为多个缺口。
- **档位定义**：
  - **A 档 小项**：单点 UI/字段/反馈，一天内可完成，可批量打包成一两个会话；
  - **B 档 中项**：单个界面的一次功能补齐或一个对话框族，0.5-1 会话/项；
  - **C 档 大项**：新宿主/新子系统/独立里程碑，>1 会话，需点名立项；
  - **不补**：已拍板不做或平台硬约束（含来源），仅登记防误补。

## 1. 界面总表（逐一）

### 1.1 框架层（Phase 3 R1-R9 已签，全部同构）

| # | 界面 | 壳内实现 | 桌面对应物 | 状态 |
|---|---|---|---|---|
| F1 | 三列骨架 + 双顶栏（45px NavBar / SceneTopBar） | `shell.rs::render_nav_top_bar` / `render_scene_top_bar` | 桌面 `92ce04f8d` 后拓扑 | 同构 |
| F2 | 资源 tab 条（会话 tab + 场景 tab、可关可切、横向滚动） | `tabs.rs`（`SceneId`/`SceneDef`/`TabsState`） | `SceneBar` + `registry.ts` | 同构 |
| F3 | NavBar 前进/后退 | `TabsState` back/forward 栈 | `NavBar` 箭头 | 同构 |
| F4 | 侧栏（搜索一切触发 + 五个导航行 + 会话区头固定槽 + 工作区分组树） | `shell.rs::render_sidebar` / `sidebar_block_into` | `MainNav` + `StickySectionHeader` | 同构（钉法不同，见 parity §2.3 行 17） |
| F5 | 全局状态栏 / 侧栏 footer（用户槽 + trinity 芯片 + 终端/工作区/齿轮） | `render_sidebar_footer` | `PersistentFooterActions` + 用户槽 | 同构 |
| F6 | 窗口钮 / 客户端装饰 | `render_window_controls` | 桌面窗口钮 | 同构（Xvfb 无 WM 不渲染属正常） |
| F7 | 欢迎面（无 tab 落地面） | `render_welcome_face` | `WelcomeScene` | 同构；打字机轮播不补（静态 = web reduced-motion 形态，在案） |
| F8 | 会话页分栏动作（AuxPane split/swap/maximize、底部终端位置偏好 bottom/right） | 壳 = 固定右面板（文件/变更）+ 底部终端；无换位/最大化动作 | `SessionScene.tsx:206-276`、`AuxPane.tsx:162-203` | 子集 → **G-F8**（A 档，可选） |

### 1.2 主场景（web `SceneTabId` 全量对照）

| # | web 场景（SceneTabId） | 壳对应 | 状态 | 差距 |
|---|---|---|---|---|
| S1 | `session`（AI 会话）+ `session:<id>` 多工作区 tab | 会话 tab（多 tab、事件泵按 session 路由） | 同构 | 遗留 A 档小项 G-S1/G-S2 |
| S2 | `welcome`（无 tab 落地） | 欢迎面 `render_welcome_face` | 同构 | — |
| S3 | `terminal`（终端主 tab；含终端工具栏/状态栏/退出码） | 底部停靠终端面板（多标签 + profile） | 形态分歧 + 子集 | G-T1/G-T2 |
| S4 | `shell`（ShellScene 包装 TerminalScene） | — | 死面（§1.6） | 不补 |
| S5 | `git`（Git 场景，Ctrl+Shift+G 可达；Branches/Graph 视图在 web 不可达） | Git 面板（tab + 右面板；Files/History/Worktrees/Snapshots 四视图） | 子集 | G-G1..G-G6 |
| S6 | `settings` | 设置场景（导航列 + 680 居中页） | 同构（页覆盖 15/21） | §1.3 |
| S7 | `file-viewer`（该 scene 仅是 WorkspaceResourcePanel 别名） | Files 面板（树 + 查看器） | 同构（形态并入右面板） | G-FL1/G-FL2/G-FL3 |
| S8 | `assistant` / `profile`（助理工作区 + 助理档案 Nursery） | — | **缺** | **G-N1（C 档，未立项）** |
| S9 | `agents`（智能体场景） | 设置「智能体」页 | 子集 | G-A1..G-A7 |
| S10 | `skills`（技能场景） | 设置「技能」页 | 子集 | G-K1..G-K5 |
| S11 | `ecosystem-compatibility`（生态兼容） | 扩展与兼容导航行 → 诚实空态 | **缺**（空态诚实） | **G-E1（C 档）** |
| S12 | `miniapps` / `miniapp:<id>` | 小应用导航行 → 诚实空态 | 缺（保留裁剪） | 不补（需 JS 宿主桥；对齐前提是大工程） |
| S13 | `pages`（发布；web 入口已隐藏） | — | 不补 | 不补（web 亦隐藏入口，在案） |
| S14 | `browser` | — | 不补 | 不补（Linux 无 webview 方案，平台硬约束） |
| S15 | `todos`（任务看板） | 任务看板（概览 + 月历 + 全部任务） | 子集 | G-P1..G-P4 |
| S16 | `insights`（洞察，位于 `scenes/my-agent/InsightsScene.tsx`） | 洞察页（tab，快捷动作入口） | 子集 | G-I1..G-I6 |
| S17 | `trinity` + `trinity-memory` | 认知体页（认知态 + 记忆面） | 子集 | G-Y1..G-Y6 |
| S18 | `content:<id>`（内容画布 tab：diff/编辑器/平台面板/Mission Control） | — | **缺** | **G-X5（C 档 = T3-20）** |
| S19 | 动态：`onboarding`（觉醒向导，挂 TrinityScene/门控） | — | 缺 | 并入 G-Y3 |
| S20 | 会话 AuxPane 右侧内容画布 | 右面板仅文件/变更 | 缺 | 同 G-X5 |
| S21 | `file-viewer` nav（工作区资源面板：文件 + 终端上下分栏） | 右面板 = 文件/变更（终端在底部） | 子集（形态分歧在案） | G-FL4 |

### 1.3 设置（web 21 页 × 壳 15 项）

壳树（`settings.rs::category_entries` / `settings_entries`，五组 15 项）对照 web
（`settingsRegistry.ts:52-417`，五类 21 页）；`HIDDEN_REGISTRY_PAGES`（`settings.rs:609`）为不出树的 8 类。

| web 页 | 壳 | 状态 | 差距编号 |
|---|---|---|---|
| application.general 通用 | ✅ 版本/开机自启/关闭行为/自动更新/完成通知/更新检查 | 子集 | G-SE1（防休眠）、G-SE2（两开关不补） |
| application.appearance 外观 | ✅ 语言/主题 | 子集 | G-SE4（外观包）、G-SE5（字号/动效） |
| application.pet 宠物 | — | 不补 | 不补（产品形态） |
| application.voice 语音 | — | 缺 | **G-SE6（C 档）** |
| application.shortcuts 快捷键 | — | 缺 | G-SE7（B 档 = T3-24 只读页；运行时改键不补） |
| application.terminal 终端 | ✅ 字号/字体/回滚/Shell 命令/终端 Profile | 同构（壳超集） | 无（web 只有默认终端 Combobox，壳不降级） |
| application.editor 编辑器 | — | 不补 | 不补（前置缺失：壳无编辑器宿主） |
| ai.models 模型 | ✅ 默认模型/已配置清单 | 子集 | G-SE8..G-SE12 |
| ai.memory 记忆 | — | 缺 | **G-SE13（B 档，未立项）** |
| ai.trinity 引擎 | — | 缺 | **G-SE14（B 档，未立项）** |
| workspace.session 会话与工作区 | ✅ 跳转卡（开全局搜索） | 同构 | G-SE15（默认 Harness/会话标题 UI） |
| workspace.worktrees 工作树 | ✅ 跳转卡（开 Git 面板 Worktrees） | 同构 | G-SE16（隔离设置字段 UI） |
| tools.execution 执行与权限 | ✅ 权限预设/自动批准 | 子集 | G-SE17..G-SE23 |
| tools.desktop-control 桌面控制 | — | 不补 | 不补（平台硬约束：browser/computer-use） |
| tools.automation 自动化 | ✅ hooks 三开关 | 子集 | G-SE24（quick actions 管理面）、G-SE25（hooks 导入/位置/兼容性） |
| tools.webSearch 网页搜索 | ✅ provider/HTTP 表单/凭证状态 | 同构 | 无（协议说明文档区留 web，在案） |
| tools.mcp MCP | ✅ 列表/增删改/启停/重启/JSON/资源/交互 | 子集 | G-M1..G-M3 |
| tools.acp ACP | — | 不补 | 不补（前置缺失：壳不接 ACP clients） |
| data.usage 调用统计 | ✅ 范围 + 汇总 + 趋势图 + 活动热力图 | 子集 | G-SE29 |
| data.archived 已归档 | ✅ 列表/恢复/删除/全删 | 子集 | G-SE30（按工作区分组） |
| data.diagnostics 日志与诊断 | ✅ 级别/日志目录/导出诊断包/clean-shutdown | 子集 | G-SE31（异常退出提示 UI） |

### 1.4 面板、叠加层与对话框

| # | 界面 | 壳 | web 锚点 | 状态 | 差距 |
|---|---|---|---|---|---|
| P1 | 全局搜索 overlay（Ctrl K） | `search.rs::render_overlay`（sessions + files 两路 + 快捷操作行） | `global-search/GlobalSearchRoot.tsx:356-682` | 子集 | G-Q1（actions/entities = T3-23） |
| P2 | 会话右面板（文件/变更 + 可弹出主 tab） | `render_resource_panel`（`session-panel-split`） | `workspace-resources/WorkspaceResourcePanel.tsx` | 同构（形态分歧在案） | G-FL4 |
| P3 | 终端（底部 pane：拖高记忆/标签/profile） | `render_terminal_panel`（`v_resizable` dock） | `BottomTerminalPane.tsx:21-56` | 子集 | G-T1 |
| P4 | 权限对话框（模态 FIFO） | `open_permission_dialog` | `flow_chat` 权限面 | 同构 | — |
| P5 | ask_user 提问对话框（多选/批量） | `open_question_dialog` | AskUser 面 | 同构 | — |
| P6 | MCP 交互应答（JSON 多行 + 预填） | `manage.rs` 交互对话框 | McpToolsConfig 交互 | 同构 | — |
| P7 | MCP 资源浏览 / JSON 原文编辑 | `manage.rs` | McpToolsConfig | 同构 | — |
| P8 | 会话用量弹窗 | `render_usage_button` | `SessionUsageModal`（App.tsx:957-981 区段） | 同构 | — |
| P9 | 通知中心面板 | — | `NotificationCenter`（同上区段） | 缺 | **G-X6（A 档）** |
| P10 | 公告 / 功能演示（announcement-system） | — | `App.tsx:976`、`shared/announcement-system/` | 缺 | **G-X1（B 档 = T3-21）** |
| P11 | 审查平台面板（review-platform canvas tab） | — | `panels/base/FlexiblePanel.tsx:157-158` | 缺 | **G-X2（C 档，依赖未就位）** |
| P12 | 内容画布（Canvas/QuickLook/Mission Control/diff tab） | — | `panels/content-canvas/ContentCanvas.tsx:247-249` | 缺 | **G-X5（C 档 = T3-20）** |
| P13 | SSH 连接 / Remote Connect 对话框 / 远程文件浏览 | — | `App.tsx:947,984`、`RemoteConnectDialog/` | 缺 | **G-X3（C 档 = T3-25）** |
| P14 | 更新提示卡 / 更新 UI / 每日门控 | `update.rs` + 设置页 | `UpdateNotificationCard` | 同构 | — |
| P15 | 托盘（未读角标 / mark-all-read / 显隐） | `tray.rs` | 桌面托盘 | 同构 | — |
| P16 | 浮动 mini chat / ComputerUse 控制卡 / 桌面宠物 | — | `AppLayout.tsx:58` 等 | 不补 | 不补（产品形态 / 平台硬约束） |
| P17 | 诊断包导出 + 日志目录 | `diagnostics.rs` | DiagnosticsSettingsPage | 同构 | — |
| P18 | 首次运行/引导 | — | `scenes/onboarding/`、`TrinityAwakenGate`（AppLayout.tsx:767） | 缺 | 并入 G-Y3 |

### 1.5 壳侧独有面（web 无对应物，防未来会话误删）

- **插件 UI 贡献场景**：`SceneId::External` + `plugin_ui.rs` 渲染器 + core `PluginUiProvider`/`host.ui.*` 通道（R7 全档）。
  web 无此产品形态（OpenCode 协议亦无 GUI 贡献）；这是壳的净新增面，1:1 语义上「无对应」而非「多出来要删」。
- 会话区头固定槽（web 用 CSS sticky，壳结构性钉住，parity §2.3 行 17 在案）。
- 设置导航覆盖侧栏且导航行常驻（web 导航行随滚动移出，刻意分歧在案）。

### 1.6 死面考据（不作为整改目标）

| web 面 | 证据 | 结论 |
|---|---|---|
| Git 场景 Branches/Graph 两视图 | `gitSceneStore.setActiveView` 唯一调用方 = `GitNav.tsx:37,60`，而 `GitNav` 未在 `nav-registry.ts:40-51` 注册（该注册表只有 settings/file-viewer），全仓无其它调用 → 两视图无 UI 入口 | web 不可达；G-G4 属「超过 web」，但仍按 T3-22 保留（API/实现成本低，且 web 后续可同享） |
| `shell` 场景（ShellNav） | `registry.ts:166-173` 注册、`SceneViewport.tsx:338-339` 渲染，但无 opener；`ShellNav` 未挂载且 `globalSearchArchitecture.test.ts:128` 断言其不得注册 | 不可达；壳终端能力由底部面板承担即可 |
| `file-viewer` scene | `FileViewerNav.tsx:1-2` 只是 `WorkspaceResourcePanel` 再导出 | 无独立界面；壳按右面板对照 |
| `trinity-memory` tab | 仅存在于 `SceneBar/types.ts:34` 类型联合，无注册/无渲染 | 纯类型残留 |
| 终端场景无专属 nav | 终端左栏历史由 ShellNav 承担，现已不可达 | 壳无需 TerminalNav |

## 2. 差距明细（逐条）

每条格式：现象 → 双端锚点 → 整改动作 → 档位。编号即计划勾选单位。

### 2.1 会话（G-S）

- **G-S1 turn footer 完成时间/耗时 meta**：web `ModelRoundItem` footer 有完成时间；壳 footer 三钮（fork/copy/快照）无 meta。
  锚点：web `session/ChatPane.tsx` 派生链；壳 `shell.rs` `turn_footer_plan`。
  动作：事件层补 turn 时间戳（现事件无时间戳、重载无来源）后渲染 meta；或显式记为不补。
  **A 档（需事件层小改）**。
- **G-S2 copy 成功态反馈（工具行）**：代码块复制已有 2s 勾选反馈（`shell.rs:5365` 一带），工具行「复制」钮无成功态。
  动作：复用同一 2s 勾选模式。**A 档**。
- **G-S3 用户行 hover 动作（edit/rollback）**：web `UserMessageItem` hover 显「编辑/回滚」；壳 R2d 已判「edit 无命令、rollback 已在 fork 菜单」。
  动作：维持不补（在案），只登记。**不补**。

### 2.2 终端（G-T）

- **G-T1 底部终端位置偏好（bottom/right）与双击尺寸档**：web `BottomTerminalPane` 有 bottom/right 偏好 + 双击在
  collapsed/compact/comfortable/expanded 间切换（`SessionScene.tsx:49-53,244-276`）；壳只有拖高 + 记忆高度。
  动作：加位置偏好与双击档位（纯壳侧布局状态）。**A 档**。
- **G-T2 终端条目动作（停止/在文件管理器显示/启动命令徽标）**：web `WorkspaceTerminals.tsx:23-101` 与
  `ShellNavEntryItem.tsx:50-127`；壳终端标签有标题（OSC/profile）与关闭，无「在文件管理器显示」等行内动作。
  动作：标签右键/行内菜单补「在文件管理器显示」；其余按需。**A 档（可选）**。

### 2.3 Git（G-G）

- **G-G1 非仓库态（not-a-repository）**：web `GitScene.tsx:143-182` 有 init 引导（把 init 指令填进 chat）；
  壳只有 trust 横幅（`git.rs` trust 族），无 not-a-repo 态。
  动作：读状态失败分类补「非仓库」态与「初始化」动入口。**A 档**。
  **✅ 交付（2026-10-15，A 批 1，`04a1675c5`）**：port 的 `NotFound` 分类进 `not_a_repository`
  态，正文换 init 卡（`git.rs::render_not_a_repository`）；按钮经 `ShellWindow::prefill_composer`
  把 prompt 交给 composer（渲染期才拿得到 `set_value` 要的 window）。证据帧 `g1-02`（卡片）、
  `g1-03`（点击后 composer 预填）。
- **G-G2 AI 生成 commit message / quick commit**：web `WorkingCopyView.tsx:157-201,428-470`；
  壳 commit 输入 + 提交已有（`git.rs:536`），无 AI 生成。
  动作：接 core 的 commit-message 生成能力（需确认 SDK 通路）或点名后做。**B 档**。
- **G-G3 按文件 discard / delete**：web `WorkingCopyView.tsx:477-673` 逐文件 stage/unstage/discard/delete；
  壳已 `stage_file`/`unstage_file`/`stage_all`（`git.rs:505-535`），无 discard/delete。
  动作：补两动作 + 危险确认（远程工作区门控）。**B 档**。
  **✅ 交付（2026-10-15，A 批 1，`04a1675c5`）**：两段式行内动作（首击「确认恢复/确认删除」danger 钮，
  次击执行）；tracked 走新 port 方法 `discard_files`（先 unstage 再 restore），untracked 走工作区
  `FileSystemService::delete_file`（故远程工作区显式禁用）。证据帧 `g1-01`/`g1-04` + 冒烟执行态
  （`git status` 与工作区文件核验）。
- **G-G4 branches / graph**：T3-22。web 侧不可达（§1.6），壳侧补 = 净新增；core API 已有。
  动作：壳内补 Branches（创建/切换/删除/复制 hash/重置到提交）与 Graph 视图；如需 1:1 语义，可随后在 web 侧接线同享。
  **B 档（原 T3 大项，因 web 亦不可达降级为 B）**。
- **G-G5 文件列表 ↔ diff 分隔条拖拽 + 快捷键**：web `WorkingCopyView.tsx:283-380`；
  壳 diff 与列表为固定布局（`git.rs` 无 resizable）。
  动作：主 tab 形态加 `h_resizable`；紧凑面板维持定宽。**A 档**。
  **✅ 交付（2026-10-15，A 批 1，`04a1675c5`）**：主 tab 文件列 ↔ diff 列共用一条可拖分隔条
  （`git-files-split`，180–560px），停靠面板仍是单列定宽。
- **G-G6 pull 与 ahead/behind**：web `WorkingCopyView.tsx:396-425`（分支名 + ahead/behind + pull/push 三件套）；
  壳有分支名展示（`git.rs:1173` 读 `overview.current_branch`）与 push（`git.rs:549`），**无 pull、无 ahead/behind**
  （`git.rs` 全文 grep 零命中）。
  动作：补 `repository_overview` 的 ahead/behind 读数与 pull 动作（远程工作区按既有降级口径）。**A 档**。
  **✅ 交付（2026-10-15，A 批 1，`04a1675c5`）**：`GitRepositoryOverview` 增 `ahead`/`behind`
  （`serde(default)`，旧载荷可读），`GitPort` 增 `pull`；overview 改为直读 libgit2（**顺带修掉
  remotes 恒空的旧缺陷**——它让 push 钮在 overview 落地后恒禁用）。壳侧分支 chip 带 ↑/↓ 徽标
  （tooltip 说明）+ pull 钮，与 push 同门槛（无 remote 即禁用）。证据帧 `g1-01`（`main ↑1`）。

### 2.4 文件（G-FL）

- **G-FL1 文件树工具行：新建文件 / 新建文件夹 / 删除 / 重命名 + 刷新**：web FilesPanel toolbar
  （`WorkspaceResourcePanel.tsx:147-171`）；壳只有 `files-reload`（`files.rs:755`）。
  动作：走 core `FileSystemService` 写面（本地工作区）＋远程/peer 显式降级（不做静默）。**B 档**。
- **G-FL2 树/搜索视图切换**：web FilesPanel 有树 ↔ 搜索两态；壳无（全库搜索在 overlay，文件树无内嵌搜索）。
  动作：files 页加过滤/搜索输入（本地过滤即可，全库搜索已有）。**A 档**。
  **✅ 交付（2026-10-15，A 批 1，`04a1675c5`）**：树顶过滤框（`files-filter-placeholder`），
  对已加载行按路径做大小写不敏感过滤（树是懒加载的，全库匹配仍归 Ctrl K overlay），
  无匹配时出 `files-no-matches` 提示；列表计数随过滤同步重置。
- **G-FL3 pdf 查看**：web 有（parity §2 行 9）；壳只 text/code/markdown/image（`files.rs:139,153`）。
  动作：pdf 渲染器选型（gpui 无内置）→ 或显式不补。**B 档 / 待点名**。
  **本轮判定（2026-10-15，A 批 1）：维持待点名**——gpui 无内置渲染器，要补得引入
  pdfium/mupdf 级原生依赖（远超 A 档），先出选型结论再动；本轮不改。
- **G-FL4 右面板形态：web = 文件 + 终端上下分栏**（`WorkspaceResourcePanel.tsx:173-199`，可拖拽）；
  壳 = 文件 / 变更（tab 互斥），终端在底部。
  动作：记录为已签形态分歧；若公子要求 1:1，则把终端列表并入右面板（需同时保留底部分栏）。**不补（默认）/ 待点名**。

### 2.5 搜索（G-Q）

- **G-Q1 actions / entities 两路搜索**（T3-23）：web `productActionCatalog.ts` + GlobalSearchRoot 多路；
  壳只有 sessions/files 两路（`search.rs:294-317`）+ 快捷操作六行。
  动作：接 action 目录（壳已有 `quick_actions` 投影）成可搜索路由 + entities 面（sessions/files/skills/agents 等聚合）。**B 档**。

### 2.6 设置（G-SE）

- **G-SE1 防休眠 prevent-sleep**（T3-19）：web `ApplicationSettingsPages.tsx:305-319`；壳无 wake-lock。
  动作：选型（org.freedesktop.ScreenSaver/portal inhibit）后加开关 + 生效接线。**B 档（依赖选型）**。
- **G-SE2 权限请求通知 / 启动提示开关**：T1-2 已判：壳无对应消费点（权限在窗内模态、无启动提示面），复制即假开关。
  **不补（在案）**。
- **G-SE3（无缺口，登记）**：通用页的「自动更新」开关 + 检查每日门控壳已同构（`settings.rs::general_group`
  的 `auto_update_field` + T1-3 快照门控）；后续盘点勿重复立项。
- **G-SE4 外观包（导入/导出/市场/画廊）**：web `AppearancePackageConfigSection.tsx:409-541` + `AppearanceMarketDialog.tsx`；
  壳无外观包概念。**不补（依赖未就位：外观市场属市场能力）**，字号见 G-SE5。
- **G-SE5 字号偏好与动效（reduced motion）**：web `FontPreferencePanel.tsx:82-138`（UI 尺寸级别 + 自定义 12-20px）
  与动效设置；壳跟随系统/组件的 reduce-motion portal，无设置项。
  动作：加「界面字号」设置（映射到 gpui base 字体档）＝ A 档；动效开关（壳内动效点少）＝ A 档（可选）。
  **✅ 交付「界面字号」（2026-10-15，A 批 2）**：外观页新增「界面与对话」档位下拉（紧凑/小/默认/中等/大/自定义）＋
  自定义档才出现的字号行（12–20 px）；读写共享 `font`（`{uiSize:{level,customPx?}}`，与 web `FontPreference` 同文档）。
  渲染侧在 `ShellWindow::render` 顶部按「档位对 14px 基准的比值 × 壳自身 16px em」调 `window.set_rem_size`，**默认档 = 壳原有字号**
  （老装机上该设置是 no-op，其余档位按 web 同样的相对比例缩放）。证据帧 `a2-01`（字号行）/`a2-02`（六档菜单）/`a2-03`（切「大」后整壳等比放大）。
  **动效开关不补**（维持原判：壳跟随系统/组件的 reduce-motion，无设置项可复制）。
- **G-SE6 语音输入**（设置页 + composer 语音按钮 + ASR 模型管理）：web `VoiceInputConfig.tsx` + `speech_api(18)`。
  动作：独立立项（模型下载/麦克风诊断/实时音频）。**C 档（未立项）**。
- **G-SE7 快捷键只读页**（T3-24）：web `KeyboardShortcutsTab.tsx`（录制/冲突/重置）；
  壳键位绑定集中在 `shell.rs`（含焦点链三件套坑）。
  动作：只读展示壳内生效键位（分组 + 描述），不做运行时改键（体系改造）。**B 档**。
- **G-SE8 订阅认证（账号登录 → 导入为模型配置）**：web `ModelSettingsPage.tsx:3595-3785`；
  壳无账号面。动作：接模型账号态（core 已有账号/身份服务则装配，否则 C 档）。**B/C 档（先考据）**。
- **G-SE9 models.dev 目录**：web `ModelSettingsPage.tsx:3902-3933,4036-4122`（来源/缓存/revision + 刷新）；壳无。
  **B 档**。
- **G-SE10 流空闲超时（TTFT/idle）**：web `ModelSettingsPage.tsx:3935-3977`；壳无。**A 档（纯配置字段）**。
  **✅ 交付**（2026-10-15，A 批 2）：模型页「模型响应超时」分节 —— 首次响应超时 + 响应间隔超时两行数字（0–86400 秒，0 = 不限时），
  读写 `ai.stream_ttft_timeout_secs` / `ai.stream_idle_timeout_secs`。0 落 `null`（web 清空输入同义）；核心会拒绝 0，
  壳侧因此不可能写出核心校验不过的值。分节注记同 web「修改后下一次对话生效」。证据帧 `a2-04`。
- **G-SE11 代理（HTTP proxy）**：web `:3979-4033`；壳无。**A 档（配置字段）**。
  **✅ 交付**（2026-10-15，A 批 2）：模型页「代理」分节 —— 启用开关 + 地址/用户名/密码三行（未启用时三行禁用，同 web）。
  写入按 key 合并（`persist_object_key`：先读回共享文档再合入该键），**本构建不认识的键不会被旧壳抹掉**（升级兼容）。
  **在案分歧**：密码行是明文输入（壳无掩码文本组件；同值本就明文存在共享配置里，壳既有 WebSearch 凭据行同形），已在行说明里写明。
  证据帧 `a2-04`（关闭态：三行禁用）/`a2-05`（启用 + 写入地址后 `ai.proxy` 落盘为 `{"enabled":true,"url":"http://127.0.0.1:7890"}`）。
- **G-SE12 高级模型配置**：自定义 Headers/请求体/跳过 SSL/请求格式/模型行编辑（分类、上下文窗口、reasoning presets）/
  测试连接（`ModelSettingsPage.tsx:2516-3323`）；壳只有默认模型下拉 + 清单。
  动作：分批补（测试连接 → 行编辑器 → headers/body 高级面）。**B 档（分两批）**。
- **G-SE13 记忆设置页**：web `MemorySettingsPage.tsx`（总开关/BTW/外部上下文策略/抽取与巩固模型/7 项高级/
  管理菜单：打开目录/重置/清空）。壳无（core 记忆能力在，仅缺 UI）。
  动作：新页（字段全量 + 危险确认）。**B 档（未立项，建议优先）**。
- **G-SE14 Trinity 引擎 LLM 配置页**：web `TrinityEngineConfig.tsx` + `TrinityLlmFields.tsx`（provider 预设/URL/模型/Key/测试连接/保存到 trinityd）；
  壳 trinity 页明确不含 engine 配置（`trinity.rs:14` 注释自陈）。**B 档**。
- **G-SE15 默认 Harness / 会话标题生成**：web `DefaultHarnessConfig.tsx:166-208`、`SessionTitleConfig.tsx:184-222`；
  壳无 UI（core 配置在）。**B 档**。
- **G-SE16 工作树隔离设置**：web `WorktreeSettingsPage.tsx:472-543`（根路径/分支前缀/复制改动/自动删除+上限）；
  壳跳转卡直达 Git 面板 Worktrees，无设置字段。**B 档**。
- **G-SE17 全局权限规则对话框**：web `GlobalPermissionRulesDialog.tsx:395-552`（规则表格/新增/排序/保存）；
  壳只有预设 + 自动批准。**B 档**。
- **G-SE18 工具执行超时 / 用户提问超时**：web `RuntimeSettingsPages.tsx:1299-1340`；壳无。**A 档**。
  **✅ 交付**（2026-10-15，A 批 2）：执行与权限页「工具运行」分节两行数字（0–3600 秒，0 = 不限时）。
  工具超时 0 落 `null`（核心「None = 不限时」）；提问超时 0 落 `0`（核心「null 或 0 均不限时」，与 web 写入值一致）。
- **G-SE19 子代理设置（默认模型 / 批量策略 / 最大并发）**：web `:1343-1432`；壳内 grep 零命中
  （`subagent_max_concurrency` 不在壳内以任何形式出现）——值是全局共享 config（web 页可改，壳只读消费其效果）。
  **A/B 档（配置字段 + 说明）**。
  **✅ 交付**（2026-10-15，A 批 2）：执行与权限页「子智能体」分节 —— 默认模型下拉（继承父会话 / 快速模型 / 主力模型 / 已启用的配置模型；
  落 `{kind,model_id}` 共享文档）、任务并行方式（安全并行 / 尽量并行两档；核心枚举里的 `serial` 若被别的产品面写下，**原样显示、不被壳改写**）、
  同时运行任务数（1–32，取 web 的 `SUBAGENT_MAX_CONCURRENCY_LIMIT`）与 Swarm 并发上限（1–64）两行数字。
  四个键 = `ai.agent_model_defaults.subagents.default` / `ai.subagent_batch_execution_policy` / `ai.subagent_max_concurrency` / `ai.swarm_max_concurrency`。
  **只加配置写面，未改核心并发行为**（壳自身不调度子代理，写下的值由核心在下次调度时读取）。
- **G-SE20 延迟加载工具开关**：web `:1435-1452`；壳无。**A 档**。
  **✅ 交付**（2026-10-15，A 批 2）：执行与权限页「按需加载工具」分节开关，写 `ai.enable_deferred_tool_loading`；
  关闭时行下显示 web 同款提示（会把全部工具定义喂给模型）。
- **G-SE21 工具参数 JSON 修复开关**：web `ToolJsonRepairSection.tsx:51-67`；壳无。**A 档**。
  **✅ 交付**（2026-10-15，A 批 2）：执行与权限页「工具参数 JSON 修复」分节开关，写 `ai.allow_tool_json_repair`。
  写 `true` 会被核心按默认值 prune 掉，等价 web 的 `resetConfig` 语义，壳侧无需单独的重置面。
- **G-SE22 审查容量（并行 reviewer / 排队上限）**：web `ReviewCapacitySection.tsx:88-118`；壳无。**A 档**。
  **✅ 交付**（2026-10-15，A 批 2）：执行与权限页「审核容量」分节 —— 审核并行数（1–2）+ 容量等待时间（0–3600 秒，步长 60），
  按 key 合并写 `ai.review_teams.default`（该文档里其余键、含本构建不认识的键，原样保留）。
- **G-SE23 「在聊天输入框显示权限控制」开关**：web `:1258-1270`；壳无（壳徽标常显）。
  动作：若 1:1 需可关；否则记为已签分歧。**A 档（可选）**。
- **G-SE24 快捷操作（quick actions）管理面**：web `QuickActionsConfig.tsx:409-520`（内置启停/编辑/删除 + 自定义新增）；
  壳 T2-10 只读消费（聊天头部菜单），无管理面。**B 档**。
- **G-SE25 hooks 导入 / 位置说明 / 兼容性**：web `HooksConfig.tsx:366-503`；壳只有总开关/项目开关/co-author。
  **B 档**。
- **G-SE29 用量分布与缓存/endpoint**：web `UsageStatisticsConfig.tsx:453-501`（模型/分组分布、筛选、缓存命中率、
  endpoint 分布）；壳 = 范围 + 汇总 + 趋势图 + 热力图（T2-8 已做）。**B 档**。
- **G-SE30 归档页按工作区分组**：web `ArchivedSessionsConfig.tsx:515-536`（可折叠 + 计数）；壳平铺列表。**A 档**。
  **⏳ 暂缓（2026-10-15，A 批 2 判）**：该页在 web 侧是**跨全部已打开工作区**拉归档会话再分组，而壳的归档列表只持当前工作区的行
  （`ShellWindow::sessions`；其它工作区的请求明确 `include_archived: false`）。1:1 需要先加一条「按工作区列归档会话」的运行时投影
  （新事件 + 页内折叠状态），属数据面而非设置字段，单列一小批更稳（与 A 批 4 会话面同批更省）。
- **G-SE31 异常退出提示**：web `ApplicationSettingsPages.tsx:478-495`（上次崩溃 Alert）；壳已有
  run-state/clean-shutdown 落盘（`diagnostics.rs`）与 `gpui-` 前缀隔离（T2-13），诊断页未展示该状态。
  动作：诊断页读 run-state 展示「上次非正常退出」。**A 档**。

### 2.7 技能（G-K）

- **G-K1 技能市场 / 安装导入**（T3-17）：web `useSkillMarket.ts` + Discover 面（`SkillsScene.tsx:775-915`）；
  壳 = 品牌空态降级卡。动作：desktop 私有 HTTP 需自实现（D44）→ 独立立项。**B/C 档（原 T3 大项）**。
- **G-K2 技能详情对话框**：web `SkillsScene.tsx:930-1122`（描述/来源/路径复制/在文件管理器打开/外部链接）；
  壳列表行无详情面。**A 档**。
- **G-K3 添加技能 / 删除技能**：web `:1124-1255`（level user/project + 路径输入/浏览 + 校验）；壳无写面。
  **B 档**。
- **G-K4 技能分组管理**：web `components/SkillGroupsView.tsx` + `SkillGroupEditor.tsx`（创建/编辑/复制/排序/删除）；
  壳 facet 有「分组」投影但无编辑（R4 判「用户分组 web 侧配置未接→不伪造」）。**B 档**。
- **G-K5 技能页搜索 / shadowed 过滤**：web `SkillsScene.tsx:321-331,298-303`；壳 facet + 列表，无搜索框。**A 档**。
  **✅ 交付**（2026-10-15，A 批 3）****：技能页顶过滤行（`manage.rs::skills_filter_row`）—— 搜索框（名称/描述/技能 key，
  trim + 大小写不敏感；key 是行内保留的路径派生身份，web 的 path 字段壳未保留）+「隐藏重复项」勾选
  （`SkillRow.shadowed`，daemon 侧标记；web 的 `isShadowed` 同源）。无匹配出 `manage-skills-no-match` 空态，
  与「无技能」空态区分。搜索框由 `manage::install_page_filters` 建成并订阅（逐键写 `SkillsFilterState`
  全局 + 重绘，同设置导航搜索的订阅范式）。证据帧 `a3-01..03`。

### 2.8 智能体（G-A）

- **G-A1 工具可见性编辑（tools tab，ToolGroupPicker）**：web `AgentsScene.tsx:1129-1158`；壳卡片只显示工具计数
  （`manage.rs` `AgentCardRow.tool_count`），创建/编辑对话框也只收 id/名称/描述/prompt 四字段
  （tools/readonly/review/policy/model 一律传 None 保留现值，`manage.rs:81-82,169-184`）。**B 档**。
- **G-A2 子代理勾选编辑（subagents tab）**：web `:1198-1294`（token 网格 + external 行禁用）；
  壳只有每 mode 技能选择（T2-11）。**B 档**。
- **G-A3 子代理模型覆盖**：web `:1113-1127`（默认/inherit/fast/primary/具体模型）；壳无。**A 档**。
- **G-A4 详情卡（能力 5 级条 / follow-up / 共享 profile / external provider）**：web `:1038-1110`；壳卡片简化。**A/B 档**。
- **G-A5 搜索 / source / kind 过滤**：web `:808-865`；壳无。**A 档**。
  **✅ 交付**（2026-10-15，A 批 3）****：智能体页顶部控制行（`manage.rs::agents_controls`）—— 搜索（名称/描述，trim +
  大小写不敏感）+「来源」菜单（任意/内置/用户/项目/外部）+「类型」菜单（任意/智能体/子代理）。类型过滤按
  web 语义**切换整块分区**（选「子代理」即只留子代理区 + 新建入口），故 `agents_group` 改为按
  `AgentsFilterState` 动态组装 items（`settings.rs` 调用点随之传 `cx`）。空结果出 `manage-agents-no-match`。
  证据帧 `a3-04..06`。
- **G-A6 Harness 概览区**：web `components/AgentHarnessOverview.tsx:70-107`；壳无。**A 档**。
- **G-A7 external agent → 生态页跳转**：依赖 G-E1。**C 档（随生态页）**。

### 2.9 MCP（G-M）

（MCP 页的三条缺口在草稿期曾拟编号 G-SE26..G-SE28，定稿统一为 G-M1..G-M3；G-SE26-28 不另存在。）

- **G-M1 OAuth 授权流 UI**（T3-18）：web `McpToolsConfig.tsx:1677-1775`（OAuth 状态/开始授权/redirectUri/token）；
  壳有登录式 token 编辑但无 OAuth 流程。**B 档**。
- **G-M2 粘贴/文件导入 + 预览导入**：web `MCPServerConfigDialog.tsx:217-342`；壳编辑器无粘贴导入预览。**A 档**。
- **G-M3 auth 模式细分（auto/oauth/token/headers）+ 远程认证对话框**：web 同文件；壳简化为 URL/凭据。
  **B 档（与 G-M1 同批）**。

### 2.10 任务看板（G-P）

- **G-P1 新建/编辑任务对话框**：web `components/TodoEditor.tsx:211-503`（名称/工作区 Combobox/agent 类型/
  schedule kind at|every|cron/时区/锚点/绑定 session/启用/prompt 1000 字）；壳只有列表操作（启停/立即运行/删除，`tasks.rs`）。
  动作：完整编辑器（core cron 写面已有）。**B 档（本组最大项）**。
- **G-P2 日详情面板**：web `TodosScene.tsx:615-673`（点日期看当日 occurrence + clear day）；壳月历无点击详情。**A 档**。
  **✅ 交付**（2026-10-15，A 批 3）****：日历格可点（唯一 id + 选中底色；再点同格=取消，同 web `onSelectDay` 的 toggle），
  卡片出日详情：标题「选中日期 · YYYY-MM-DD」（参数化键）+「清除」+ 当日条目（时间 + 任务名 + 
  下次运行/已运行/出错 chip，按时刻排序）或空态「当天没有安排。」。条目只列 store 真正携带的两个瞬时
  （每任务最多两条）——与日历图例「不含历史运行日志」的诚实口径一致。证据帧 `a3-08`。
- **G-P3 状态徽标与不可执行分区**：web `components/TodoItemRow.tsx:81-122` + `TodosScene.tsx:344-353`
  **✅ 交付**（2026-10-15，A 批 3）****：
  行徽标（`JobBadge` + 纯函数 `job_badges`/`next_run_job_id`：正在运行=上次运行中 / 下次运行=全局最近
  未来瞬时（web `isNextRun`）/ 已逾期=启用且瞬时已过）+ 不可执行分区（`inactive_reason`：未启用→已暂停 /
  一次性 At 且已跑过→已结束 / 启用但无下次运行→计划无效），分区头「无后续运行（N）」= web `inactive.title`。
  证据帧 `a3-07`（注意：徽标标签用「正在运行/已结束」而非 web 的共享词 Running/Done，因 core 契约无法引用
  shared 术语通道，同值会让 `i18n:audit` 的 `sharedTermDuplicates` 超基线）。
  （running/overdue/disabled/completed/invalid 徽标 + 不可执行任务分区）；壳行内已有 next_run/last_run 时间与
  上次运行状态（`tasks.rs:259-270`）+ 启停/运行/删除，缺 running/overdue 徽标与不可执行分区。**A 档**。
- **G-P4 30s tick 倒计时**：web `TodosScene.tsx:73-75`；壳无实时时钟刷新。**A 档（可选）**。

### 2.11 洞察（G-I）

- **G-I1 模型选择 + 范围 7/14/30/90**：web `InsightsScene.tsx:169-249`；壳范围 7/30/90（`insights.rs:20`）无模型选择。**A 档**。
  **✅ 交付**（2026-10-15，A 批 3）****：范围 chips 7/14/30/90（默认 30，选中态 Primary）+ 模型菜单（首项「主模型」=
  daemon 默认选择器 `primary`，其后目录里 `enabled` 的模型；选择消失时回退首项）+「生成报告」按钮；
  `runtime::generate_insights(days, model_id)` 透传 `InsightsService::generate(days, selector)`。
  **口径差异（记录）**：壳的模型目录投影不含 capabilities，故只按 `enabled` 过滤，未再按 web 的 `text_chat` 筛。
  证据帧 `a3-09`。
- **G-I2 生成五阶段进度面板**：web `:35-153`；壳只有「生成中…」+ 取消。**B 档**。
- **G-I3 统计行 + bar 图表组**：web `:772-1048`（goals/tools/languages/session types/响应时间/时段/tool errors/
  agent types/outcomes/friction）；壳渲染结构事实（headline + 四段叙事），无图表。**B 档**。
- **G-I4 8 段 + TOC 滚动联动**：web `:22-31,437-527`；壳四段（what's working/hindering/quick wins/looking ahead，
  `i18n.rs:102-105`）无 TOC。**B 档**。
- **G-I5 建议复制按钮（openbitfun.md 补丁/features/patterns）**：web `:1050-1160`；壳无。**A 档**。
  **✅ 交付**（2026-10-15，A 批 3）****：报告新增三段建议区（OPENBITFUN.md 增补 / 值得一试 / 用法模式），每行一个复制钮，
  入剪贴板的载荷与 web 一致（增补 `content` / 特性 `example_usage`（空则不出钮）/ 模式 `suggested_prompt`，
  后者标签「试试这个提示词」）；列表为空整段不渲染（同 web 条件）。
  **未做视觉冒烟**：本机洞察报告 store 为空且隔离环境无模型目录，无法出报告 → 待有报告时目视复验（见 HANDOFF-081）。
- **G-I6 Open HTML + 历史 5 份 meta 卡**：web `:287-431,529-577`；壳历史列表无 meta 细节、无 Open HTML
  （HTML 报告文件明确留给 desktop-web，`insights.rs:1-8`）。**A 档（meta）/ 不补（HTML）**。
  **✅ 交付 meta 半**（2026-10-15，A 批 3）****：历史区头「报告历史」+「仅保留最近 5 份报告」；每行 meta 卡 =
  生成时间（本地 `YYYY-MM-DD HH:MM`）+ 窗口（ISO 前 10 字，`start → end`）+ 会话 `X / Y` + 消息数 +
  小时 + 覆盖天数 + 生成用量 tokens + 生成模型（` + ` 连接）+ 目标/语言标签（去重、目标优先，同 web 取前 3）。
  Open HTML 维持**不补**（需 `plugin-opener` 原生打开，HTML 报告文件归 desktop-web）。
  **同 G-I5 的待复验项**：本机无报告数据，meta 卡只经纯函数测试（`history_range/history_timestamp/history_tags`），
  未目视。

### 2.12 认知体 Trinity（G-Y）

- **G-Y1 晨报卡**：web `TrinityScene.tsx:304-372`（greeting/隔夜整理/thoughts/urges）；壳无。**B 档**。
- **G-Y2 云记忆卡**：web `:131-292`（注册/登录 → passphrase ≥8 → sync/backup/restore + pending ops）；
  壳自陈不含云同步（`trinity.rs:14`）。**C 档（依赖云服务）**。
- **G-Y3 身份创建卡 + 觉醒向导（onboarding）**：web `:835-865,1217-1220`（IDENTITY/SOUL/USER 模板、
  删 BOOTSTRAP、设主助理、开新会话）；壳只有 status 的 awakened 读数与身份行。**B 档**。
- **G-Y4 记忆导出 / 导入 JSON**：web `:965-1211,628-689`（导出 + 导入 + being_id 不匹配二次确认）；
  壳记忆面有回忆/记存/强化/两步遗忘，无导出导入。**A 档**。
- **G-Y5 记忆行内编辑（memorize 修正）**：web `:1085-1190`；壳无行内编辑。**A 档**。
  **✅ 交付**（2026-10-15，A 批 3）****：每行「修正」钮 → 行体换成预填当前内容的共享 textarea（`trinity_edit_content`，
  窗口持有）+「保存修正 / 取消」（保存键在有内容时可用，空内容不得提交 = web 语义）；保存走
  `memorize {content, target_id}`，runtime 读 daemon 的 `corrected` 回执（新增 `RuntimeEvent::TrinityCorrection`）
  → 页面出「修正已应用。」或「原始记忆已不存在（可能已被遗忘）。」（web 的两条 toast 文案）。
  冒烟：开编辑器 → 取消（**未提交修正，不动机器人记忆图**），取消后行像素级还原。证据帧 `a3-11`。
- **G-Y6 认知态卡细节（小）**：web `:883-963`（emotion hero/focus、confidence、cycle #、needs、valence sparkline、
  记忆规模 nodes/triples/vocabulary）；壳已渲染 confidence / cycle # / nodes / triples（`trinity.rs:669-706`），
  仅缺 vocabulary 计数与 sparkline 粒度差异。动作：补 vocabulary 一行。**A 档（极小）**。
  **✅ 交付 vocabulary 行**（2026-10-15，A 批 3）****：PSI 解析补 `memory.vocabulary_size` → 卡上第三计数「词汇量」
  （本机实测 40519）；sparkline 粒度差异维持（`trinity.rs` 趋势条为既有形态，未在本批改）。证据帧 `a3-10`。

### 2.13 助理 / 角色面（G-N，未立项）

- **G-N1 助理档案（Nursery）整套**：web `scenes/profile/` + `scenes/assistant/`：
  助理画廊（主助理置顶/创建/设默认/删除）、`AssistantConfigPage`（头像选择器、名字/creature/vibe 行内编辑、
  IDENTITY/SOUL/USER persona 文档编辑 + 600ms 防抖自动保存、快捷输入、会话列表、计划任务）、
  `AssistantDefaultsPage`（skills/builtin tools/mcp 三段默认集 + 过滤 + 恢复产品默认）、`AssistantAvatarPicker`。
  壳无任何对应面（导航「智能助理」= 会话，`shell.rs:740-746`）。
  动作：独立立项（助理工作区 = core 的 assistant workspace 概念 + persona 文档读写 + 默认集）。**C 档**。

### 2.14 生态与其他面（G-E / G-X）

- **G-E1 生态兼容页**：web `EcosystemCompatibilityScene.tsx`（产品发现/账号/ACP run/批量导入/撤销/开发中提示）；
  壳 = 诚实空态。动作：独立立项（发现 + 冲突解决 + 批导入）。**C 档**。
- **G-X1 公告面**（T3-21）：web `shared/announcement-system/` + `App.tsx:976`；壳无。**B 档**。
- **G-X2 审查平台**：web `ReviewPlatformPanel`（canvas tab）；壳仅有 review 族标签（T2-6）。**C 档（依赖未就位）**。
- **G-X3 远程四件套**（SSH / Remote Connect / Peer / Dispatch，T3-25）：web 各对话框与面板；壳无。
  动作：各自独立里程碑（core 端口在，壳侧大工程）。**C 档**。
- **G-X5 内容画布 / Canvas / Mission Control / QuickLook**（T3-20）：web `content-canvas/`；
  壳无。**C 档**。
- **G-X6 通知中心面板**：web `NotificationCenter`；壳有 OS 通知 + 托盘角标即可，面板本身缺。**A 档**。

## 3. 整改计划（分档与建议排序）

### 3.1 A 档（小项，建议打包为 2-3 个会话）

建议分组（同文件/同面聚批，减少上下文切换）：

- **A 批 1 = Git + 文件**：G-G1（非仓库态）、G-G3 的 discard/delete（若判定可行）、G-G5（splitter）、G-G6（pull/ahead-behind）、
  G-FL2（树搜索）、G-FL3 判定（pdf 选型或显式不补）。
  **✅ 交付（2026-10-15，`04a1675c5`）**：G-G1 / G-G3 / G-G5 / G-G6 / G-FL2 五项落地，
  G-FL3 判为维持待点名（见 §2.4）。含 core 侧最小契约扩展（`ahead`/`behind` + `pull` + `discard_files`），
  壳 298 lib + 5 PTY + 5 update、git 契约 30、runtime-ports 12、i18n:audit 全绿；Xvfb 冒烟 0 panic
  （证据帧 `docs/plans/assets/gpui-shell-layout-parity/g1-*.png`）。
- **A 批 2 = 设置小字段**：G-SE5（字号）、G-SE10/G-SE11（流超时/代理）、G-SE18、G-SE20/G-SE21/G-SE22、
  G-SE19（子代理并发 UI）、G-SE30（归档分组）、G-SE31（异常退出提示）。共用同一套 SettingField 范式。
  **⚠ 部分交付（2026-10-15，A 批 2）**：八项字段落地 —— G-SE5 界面字号（外观页）、G-SE10 流超时 + G-SE11 代理（模型页）、
  G-SE18/G-SE19/G-SE20/G-SE21/G-SE22（执行与权限页：两个运行时超时、子智能体四个键、按需加载工具、JSON 修复、审核容量），
  共 61 个新键 ×3 语（ftl 800→861），壳侧 10 条新测试；**提交内容本身**壳 315 lib 全绿（含新页无头布局测试），
  工作区并集含并发设置测试 317 全绿；`i18n:audit` 0 错 0 警、`check:core-boundaries` 绿；Xvfb 冒烟见 §7 更新记录。
  **余下两项**：**G-SE30 暂缓**（理由见 §2.6，需先做归档跨工作区投影）；**G-SE31 未动** —— 工作区那份未登记的
  「上次异常退出」WIP 正是该语义（见 §6.1 W-4），按纪律原样留在工作区、等公子或作者点头后再接管/提交。
- **A 批 3 = 场景小项**：G-P2/G-P3（任务日详情/徽标）、G-I1/G-I5/G-I6（洞察小项）、G-Y4/G-Y5/G-Y6（trinity 小项）、
  G-K5（技能搜索）、G-A3/G-A5/G-A6（智能体小项）、G-M2（MCP 粘贴导入）、G-Q1 的前置（action 目录投影）。
  **⚠ 部分交付（2026-10-15，A 批 3，会话 74，提交 `5867bdf25`）**：G-P2、G-P3、G-I1、G-I5、G-I6(meta)、G-Y5、G-Y6、G-K5、G-A5 九项落地
  （**提交内容本身**壳 305 lib 全绿，工作区并集含并发会话的设置测试为 307；5 PTY + 5 update 不受影响；`i18n:audit` 0 错 0 警、core-boundaries 绿；Xvfb :189 冒烟 0 panic/0 缺资产，
  证据帧 `a3-01..11`）。**暂缓四项（附理由，下批首选）**：
  - **G-Y4 记忆导出/导入 JSON**：web 两处都用原生文件对话框（`plugin-dialog` save/open），壳无文件选择器
    （M24 已判「不放假入口」）——要么壳侧改用「固定导出目录 + 复制路径」范式（需先定设计，偏离 1:1 形态），
    要么维持待点名。
  - **G-A3 子代理模型覆盖**：需要 agent 编辑对话框加模型选择 + core `update_custom_entry_config(model, clear_model_override)`
    写面接线（`custom.rs:401-445`），属对话框族改造，与 G-A1/G-A2 同批更省。
  - **G-A6 Harness 概览区**：壳的 `AgentCatalog` 只投影 modes/subagents，harness 类 agent 未进目录 → 需先扩 core 投影
    （比纯 UI 大，归 B 档口径）。
  - **G-M2 MCP 粘贴/文件导入 + 预览**：粘贴解析 + 预览表（逐条勾选 + 目标 id 编辑 + 冲突提示 + 导入后强制 disabled）
    是独立对话框族，单列一批更稳。
  另 **G-I5/G-I6 的视觉复验待有洞察报告时补**（本机 store 为空）。
- **A 批 4 = 会话/终端/杂项**：G-S1/G-S2（turn footer meta / copy 反馈）、G-T1/G-T2、G-F8、G-X6（通知中心）。

验收：壳内锚点 + 无头断言（可测者）+ Xvfb 冒烟 0 panic/0 缺资产；视觉项出双端并排图。
每批完成后回写 parity §2/§2.3 对应行与本文勾选状态。

### 3.2 B 档（中项，单项立项；下表即建议排期）

| 序 | 项 | 编号 | 依赖/说明 | 预估 |
|---|---|---|---|---|
| 1 | 任务编辑器（at/every/cron/时区/绑定 session） | G-P1 | core cron 写面已有 | 1 会话 |
| 2 | 记忆设置页 | G-SE13 | core 记忆能力在，纯 UI | 0.5-1 |
| 3 | Trinity 引擎配置页 | G-SE14 | trinityd 配置面已有（web 同源） | 0.5 |
| 4 | 技能管理写面（添加/删除/分组/详情/搜索） | G-K2..K5 | 部分需 core 写面核对 | 1 |
| 5 | 智能体编辑面（工具/子代理/模型覆盖/过滤/详情） | G-A1..A6 | profiles 写路径已通（T2-11/12） | 1-1.5 |
| 6 | MCP OAuth + 导入/认证细分 | G-M1,G-M3 | OAuth 流程需 core 支持核对 | 1 |
| 7 | Git 尾巴（AI commit message、branches/graph） | G-G2,G-G4 | branches/graph 为净新增（§1.6） | 1-1.5 |
| 8 | 搜索 actions/entities | G-Q1 | action 目录 + 聚合查询 | 1 |
| 9 | 洞察补齐（模型/五阶段/图表/TOC） | G-I2..I4 | 复用报告结构，图表自绘 | 1-1.5 |
| 10 | 设置字段族（执行/自动化/模型高级） | G-SE12,17,24,25,29 | 多数为配置项 UI | 1.5-2 |
| 11 | 快捷键只读页 | G-SE7 | 键位表投影 | 0.5 |
| 12 | 公告面 | G-X1 | 载体选型（横幅/中心） | 0.5-1 |
| 13 | 防休眠 | G-SE1 | 选型先行（portal inhibit） | 0.5-1 |
| 14 | 技能市场 | G-K1 | desktop 私有 HTTP 自实现（D44） | 1-1.5 |
| 15 | 子代理设置/工作树设置/默认 Harness/会话标题 | G-SE15,G-SE16,G-SE19 | 配置字段族，可并入序 10 | 0.5-1 |

### 3.3 C 档（大项 / 独立里程碑，均需点名立项）

| 项 | 编号 | 备注 |
|---|---|---|
| 内容画布 / Canvas / Mission Control | G-X5（T3-20） | 多标签工作台，大工程 |
| 生态兼容（发现/账号/批导入/撤销） | G-E1 | 含 ACP run 面 |
| 助理 / 角色（persona + 默认集）面 | G-N1 | 未立项，本盘点新增 |
| 语音输入 | G-SE6 | ASR 模型管理 + 实时音频 |
| 远程四件套（SSH/RemoteConnect/Peer/Dispatch） | G-X3（T3-25） | 各自独立里程碑 |
| 审查平台 | G-X2 | 依赖未就位 |
| MiniApps 运行时与市场 / 外观市场 / 账号市场 | §1.2 S12、G-SE4 | 需 JS 宿主桥或市场能力 |
| R7c 产品化尾货 | W-1 | 见 §6.1 |
| 并发规模尾项 | W-2 | 见 §6.1 |
| 模型订阅认证（若账号服务缺失） | G-SE8 | 先考据再定档 |

### 3.4 不补清单（防误补，含来源）

| 项 | 来源 |
|---|---|
| 内嵌浏览器 / computer-use / 浏览器控制 | 平台硬约束（Linux 无 webview；computer-use Windows-only） |
| 桌面宠物 / 工具条模式 / 浮动 mini chat | 产品形态（D71 裁剪） |
| 编辑器设置页 / inline AI / ACP 设置页 | 前置缺失（壳无编辑器宿主 / 不接 ACP clients） |
| shortcuts 运行时改键 | 键位体系改造（T3-24 只做只读页） |
| pages 发布 | web 亦隐藏入口 |
| 权限请求通知 / 启动提示开关 / include_sensitive_diagnostics | 壳无对应消费点，复制即假开关（T1-2/T2-13 在案） |
| 打字机轮播（欢迎面） | 静态 = web reduced-motion 形态（R2b 在案） |
| 用户行 hover edit/rollback | edit 无命令、rollback 已在 fork 菜单（R2d 在案） |
| 死面（GitNav branches/graph 的 web 入口、ShellNav、trinity-memory、file-viewer scene） | §1.6 |
| 右面板终端列表（形态分歧默认保持） | R2c 决策；如需 1:1 点名再改 |

## 4. 验收口径

- **每项 Done 定义**：壳内实现锚点（模块/函数）+ 至少一条无头断言（可测者）+ Xvfb 冒烟（0 panic、
  0 missing asset；私有显示号）；视觉类另出双端并排签字图（壳 `DISPLAY=:9x` / 桌面 dev `:98`，同主题）。
- **证据归档**：截图与 sha256 进 `docs/plans/assets/gpui-shell-layout-parity/`（沿用命名 `r{n}-*` / `md-*` 风格），
  并在 parity §2.3 证据段追加一行；本文勾选对应编号。
- **回写义务**：任何一项落地后刷新 parity 对应行（§2 能力域 / §2.1 设置 / §2.3 布局）与本文状态；
  i18n 新键须三语平齐并数键；主题/颜色改动走 `theme:color-audit:all`。
- **纪律**：提交前 `cargo test -p openbitfun-gpui-shell` + `pnpm run fmt:rs` + `check:core-boundaries` +
  hygiene；Xvfb 冒烟后清共享副作用（workspace_data.json / ~/.openbitfun/projects / 临时工作区）。

## 5. 建议执行顺序（按「1:1 感知密度 ÷ 成本」）

1. **A 批 1-4**（一批 2-3 会话，先把小项清零，减少噪声）；
2. **B 序 1-3**（任务编辑器 / 记忆设置页 / Trinity 引擎页 —— 三个「有空页或空能力」的用户可见大洞）；
3. **B 序 4-9**（技能 / 智能体 / MCP / Git / 搜索 / 洞察 —— 管理面补齐）；
4. **B 序 10-15**（设置字段族与市场、公告、防休眠）；
5. **C 档**按公子点名逐个立项（内容画布与生态导入建议排在最前，助理面与语音次之）。

## 6. 附录

### 6.1 工程性尾项（非界面，但属整改计划范围）

- **W-1 R7c 产品化尾货**：configured 插件 UI 接入装配、SourceApproved 门、worker 生命周期统一回收。
- **W-2 并发规模档**：内核血缘批量 API（322ms→~15ms，需动核心）、`ai.max_active_sessions` 配置化、
  1000 已加载会话 RSS 底数、真机千并发压测；壳侧已修（SessionRefresh 调度器 / 事件泵批量化 / Lagged 自愈）。
- **W-3 真机验收项**：splitter 拖拽、托盘菜单与角标、关闭行为、更新全链、玻璃观感/Shimmer 手感、IME 复验。
- **W-4 未提交改动**：并发规模档（`runtime.rs`/`shell.rs` + `examples/session_scale_probe.rs`，HANDOFF-079）
  现存放于 `stash@{0}`（"gpui T13"，并发会话 `3c274e9e1` 提交前 stash；内容备份
  `.relay/journal/handoff079-wip.patch`），**恢复时机需与创建它的会话核对，勿擅自 pop/丢弃**；该档提交仍需公子点头。
  **工作区当前另有一份未登记的未提交改动**：`settings.rs` 诊断页「上次异常退出」横幅（G-SE31 语义，
  `+153/-2`）+ `i18n.rs`（+33）+ ftl×3（各 +4 键）。它不是 A 批 3 的内容，且查不到登记作者——
  `state.json` 里会话 73 早在 13:10 已 done（交付 A 批 1），71/72 登记 active 但无 handoff，
  rollout 已轮转查不到该改动来源（settings.rs mtime 2026-10-15 18:20）。
  A 批 3 与 A 批 2 交付时都按同一纪律**只暂存自己的 hunk**（`git hash-object` + `update-index --cacheinfo`
  装入「只有本批内容」的自建副本，工作区其余部分一动未动；A 批 2 的隔离验证 = 受控换入换出 + 哈希自检，换入期 315 绿、
  恢复后并集 317 绿），该 WIP 原样留在工作区（A 批 2 交付后复核：其 4 个 ftl 键、i18n 的字段/helper、settings.rs 的横幅与两条测试
  仍在，内容逐字未变）；**接管或覆盖它前需公子或作者点头**。
- **W-5 A 批 3 未交付余项**：G-Y4（原生文件对话框）/ G-A3（子代理模型覆盖）/ G-A6（Harness 概览，需扩 core 投影）/
  G-M2（MCP 粘贴导入预览）四项暂缓（理由见 §3.1 批 3 行）；G-I5/G-I6 的视觉复验待本机出现洞察报告时补。

### 6.2 小扫尾四项（原 R2c/R2d 余留，归 A 档批 1/批 4）

Git header 左组 360px 省略号；Xvfb splitter 拖拽真机待验；turn footer 完成时间/耗时 meta（=G-S1）；
copy 成功态图标反馈（=G-S2）。

### 6.3 与既有清单的映射

- **T3 大项 17-25** → 本文：17→G-K1、18→G-M1、19→G-SE1、20→G-X5、21→G-X1、22→G-G4、23→G-Q1、24→G-SE7、25→G-X3。
- **parity §2 行 8（工作区打开/新建/最近）**：已由 M11 + HANDOFF-076 交付，本文按现状记「同构」；
  §2 行 8 的表格文字为 M10 基线口径，待下次整表刷新时改判。
- **parity §2 行 17（定时任务）**：M10 口径「已支持」过宽 —— 本次核对确认缺编辑器（G-P1），
  建议下次刷新把该行改判为「已支持（子集：无新建/编辑器）」。
- **parity §2 行 18/19/20/21（洞察/技能/智能体/MCP）**：同上，均应在下次整表刷新时按本文 §2. 差额改判为「子集」并注明编号。

## 7. 更新记录

| 日期 | 变更 |
|---|---|
| 2026-10-15 | **A 批 2（设置小字段）部分交付**（`4ba07163a`）：G-SE5 界面字号（外观页档位下拉 + 自定义 12–20 px，渲染侧 `set_rem_size`，默认档 = 壳原有字号）、G-SE10 流空闲/首字超时、G-SE11 代理（按 key 合并写、未知键不丢）、G-SE18 工具/提问超时、G-SE19 子智能体默认模型 + 批量策略 + 两个并发上限（只写配置、不动核心并发；证据帧 `a2-06`/`a2-07`/`a2-09`）、G-SE20 按需加载工具、G-SE21 参数 JSON 修复、G-SE22 审核容量（证据帧 `a2-08`）；G-SE30 暂缓（壳归档列表只持当前工作区，1:1 需先加跨工作区归档投影）；G-SE31 因工作区那份未登记 WIP 正是该语义而按纪律未动（见 §6.1 W-4）。ftl 800→861 ×3、壳 315 lib（提交内容）/317（工作区并集）全绿。Xvfb :190 冒烟 0 panic/0 缺资产（唯一 ERROR = Xvfb xinput 既有噪声），证据帧 `a2-01..09`：字号行 / 六档菜单 / 切「大」后整壳等比放大 / 模型页两分节（超时 600·600 与禁用态代理）/ 代理启用并写入地址后落盘 / 执行页工具与超时 / 子智能体与两个开关 / 审核容量 / 子智能体默认模型菜单（继承父会话·快速模型·主力模型） |
| 2026-10-15 | **A 批 3（场景小项）部分交付**（会话 74，与 A 批 2 并发）：G-P2 日详情面板、G-P3 行徽标 + 不可执行分区、G-I1 范围 7/14/30/90 + 模型选择、G-I5 建议三段复制、G-I6 历史 meta 卡、G-Y5 记忆行内修正（含 `corrected` 回执）、G-Y6 词汇量、G-K5 技能搜索 + 隐藏重复项、G-A5 智能体搜索 + 来源/类型过滤；G-Y4/G-A3/G-A6/G-M2 暂缓（理由见 §3.1）；证据帧 `a3-01..11`；W-4 按 stash + 并发 WIP 现状重写并新增 W-5 |
| 2026-10-15 | **A 批 1（Git + 文件）交付**（`04a1675c5`）：G-G1 非仓库态卡 + init 填 chat、G-G3 行内 discard/delete（两段式 + 远程降级）、G-G5 文件列↔diff 分隔条、G-G6 ahead/behind 徽标 + pull（顺带修 remotes 恒空）、G-FL2 文件树本地过滤；G-FL3 判为维持待点名。core 侧最小契约扩展：`GitRepositoryOverview` +`ahead`/`behind`、`GitPort` +`pull`/`discard_files`。证据帧 `g1-01..04`（Xvfb 冒烟：徽标/行内动作/armed 态/init 预填 + 执行态核验） |
| 2026-10-15 | 首版：界面逐一盘点（框架层 8 行 / 主场景 21 项 / 设置 21 页 / 面板与对话框 18 行 / 壳侧独有 3 项 / 死面 5 项）；差距明细 80 余条编号条目（G-*）含双端源码锚点；整改计划 A/B/C 分档 + 不补清单（含拍板来源）；验收口径与建议执行顺序；T3 17-25 与本文编号的映射；工程性尾项（R7c 尾货 / 并发规模 / 真机 / 未提交改动）归并登记 |
