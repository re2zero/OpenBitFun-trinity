# GPUI 桌面壳 parity 矩阵与默认切换计划

本文件维护 `src/apps/gpui-shell`（GPUI 原生桌面壳）相对 Tauri 桌面壳的能力差异、缺口分级与默认切换机制。
它是接力计划 M10 的产出（gpui-frontend 分支，过程计划不随仓库发布）。基线：2026-10-09 @ `7c6586971`
（Phase 2 M7-M9 全部完成）。相关设计：

- 成功标准与不做项：本地接力计划 `.relay/PLAN.md` 的 D6 / D19 / D20（该文件不入库，读者看本文件即可）；
- GUI 宿主选择的产品规则：[产品定制与跨界面组装设计](../architecture/product-customization-blueprint.md) §2 / §7；
- CLI 平替先例：[CLI 产品线设计](../architecture/cli-product-line-design.md)；
- 场景面真相源：`src/web-ui/src/app/scenes/registry.ts`；命令面：`src/apps/desktop/src/api/`；
- **界面逐一差距与整改计划**（2026-10-15 全量盘点，A/B/C 分档 + 每项锚点）：
  [gpui-shell 界面 1:1 差距盘点与整改计划](gpui-shell-ui-parity-remediation-plan.md)。

## 0. 统计口径

- **分母 = 704 个 `#[tauri::command]`**（`grep -r '#\[tauri::command\]' src/apps/desktop/src | wc -l`，2026-10-09 实测）：
  685 个分布在 `api/` 下 65 个文件，另 19 个在 `openbitfun_control_host.rs`(4)、`appearance.rs`(4)、
  `frontend_workbench.rs`(5)、`lib.rs`(3)、`sleep_prevention.rs`(2)、`runtime/mod.rs`(1)。
- **可复用比例（逐文件分类，2026-10-09 探针）**：约 **609/685（~89%）** 是产品能力命令——native 前端可绕开 Tauri
  直接调用同一 core/service/SDK 接口；约 **46（~7%）** 是纯 Tauri/webview 管道（窗口 chrome、webview eval、
  事件桥、系统对话框、devtools）；约 **30（~4%）** 能力+传输混合（如 `set_config`、`terminal_create`、流式 turn）。
  因此"704"不等于 704 个待实现功能：壳只需为纯管道命令写原生替代。
- **场景面**：`registry.ts` 16 个注册场景 + 3 个动态场景（`miniapp:` / `content:` / welcome），另计左侧导航 4 种模式、
  设置页 21 页（5 类）、次要面板/对话框（canvas、review platform、远程连接、SSH、dispatch、MCP 交互等）。
- **三态判据**：
  - **已支持** = 壳内有实现且有真机或无头证据（引用会话编号/决策条目）；
  - **可低成本补** = 底座（core / runtime-services / 端口 / SDK）已存在，壳侧工作量 ≲ 1 会话；
  - **缺** = 需要新能力或大工程量（>1 会话），或已明确不做。

## 1. gpui-shell 已交付能力面（基线）

| # | 能力 | 实现 | 证据 |
|---|---|---|---|
| 1 | 会话列表 / 新建 / 恢复 / 流式 markdown / 工具卡五态 / 停止 / 自动标题 / 1k 虚拟化 | `runtime.rs`、`shell.rs` | M4A 点级回归；D16-D18 |
| 2 | 权限应答闭环（模态 FIFO 队列、拒绝/一次/始终、全进程覆盖） | `runtime.rs::spawn_permission_pump` + `shell.rs` 对话框 | M5 / D21 |
| 3 | 内置终端（PTY、自绘网格、ANSI、alt-screen、选中复制、粘贴、IME、回滚、流控、停靠分栏） | `terminal/{emulator,pane,row_runs,colors}.rs` | M7 / D24 / D26 / D29 |
| 4 | 设置页 3 页：外观（语言/主题）、终端（字号/字体/回滚/Shell）、工具权限（预设/自动批准），ConfigService 直通落盘 | `settings.rs` | M8 / D27-D29 |
| 5 | Git 只读变更面板（workspace diff 文件列表 + 自绘 unified diff，换行开关 + 横向滚动） | `git.rs` | M8 / D28 / D29 |
| 6 | 主题（theme-openbitfun 语义 token、light/dark、CLI preset 兼容、appearance id 映射） | `theme_openbitfun.rs`、`theme.rs` | M4B / D28 |
| 7 | i18n 三语（zh-CN / zh-TW / en-US）即时切换 | `i18n.rs` | D12 / D28 |
| 8 | 打包（tar.gz + deb）与自更新通道（`--check-update` / `--self-update` / `--update-manifest`，sha256 + 原子替换 + 回滚） | `update.rs`、`cli.rs`、`scripts/gpui-shell-package.mjs` | M9 / D30-D32 |
| 9 | 窗口装饰（TitleBar CSD）+ 侧栏导航 + 悬浮 composer | `shell.rs` | M4B / D28 |

## 2. Parity 矩阵（场景级）

状态列含义见 §0。缺口备注中「端口现成」指 core/runtime-services 已有对应 API，壳侧主要是 UI 与装配。

| # | 能力域 / 场景 | 桌面命令域（命令数） | 状态 | 缺口与备注 |
|---|---|---|---|---|
| 1 | 会话对话闭环 | agentic_api(58) 的 turn/session 子集 | 已支持 | 会话级事件流、工具卡、停止、标题齐备；富转写（M24）：工具行语义摘要 + 探索/执行折叠组 + 子代理身份卡（见 §2.3 第 14 行） |
| 2 | 会话管理操作（重命名/删除/归档/导出/fork/压缩） | session_api(19)、agentic_api | 已支持 | M13 起全量落地：重命名 / 删除 / 归档+恢复 / fork / 压缩 / 导出转写（`runtime.rs` 直调 SDK 全族；T1-4 归档页批量删除；导出走 `read_session_transcript`） |
| 3 | 模型 / Provider 选择 | `get_ai_model_catalog` + config 模型档案 | 已支持 | 模型设置页（`settings.rs::models_page`：provider/key/模型选择）+ 会话级 mode/model 切换（`update_session_mode` / `update_session_model`，composer 选择器） |
| 4 | 附件 / 图片上下文（粘贴图片、拖文件、上传） | clipboard_file_api(3)、context_upload_api(1)、file_drop_preview_api(1) | 已支持 | composer 附件区 + turn 请求 `attachments: Vec<AgentInputAttachment>`；拖放/剪贴板走 gpui-pre 原生（XDND / wl_data_device / `ClipboardItem`） |
| 5 | 交互式提问（ask_user / user question） | tool_api(8) 的 question 两条 | 已支持 | 提问对话框 + FIFO 队列（与权限队列同型；`PendingUserQuestion` 快照拉取 + `submit_user_answers` 应答） |
| 6 | 子代理投影 / BTW 侧会话 / 任务详情 | subagent_api(10)、btw_api(2) | 已支持（近似） | 子详情视图 = 全量转写（快照树超集）+ 贴底跟随；后台任务/父 turn 徽章计数、review 族「审核中」标签（T2-6）；BTW 侧会话 + busy 停止（T1-5）。web 内嵌投影的 last-round 截断为渲染量控制，壳无内嵌面（D67） |
| 7 | 语音输入 / 实时通话 | speech_api(18) | 缺 | ASR 模型管理 + 实时音频，大工程 |
| 8 | 工作区打开 / 新建 / 最近工作区 | commands.rs(79) 的 workspace 子集 | 缺 | **壳固定 `current_dir()` 为工作区（`runtime.rs`）**；core `WorkspaceService`/`WorkspaceManager` 能力齐（open/quick_open/create/switch/close/get_recent/get_opened/list/search），多工作区运行时由 `WorkspaceRuntimeService::ensure_runtime_for_workspace_binding` 管；但壳的 ownership（`fixed_workspace`）与 local services（`build_local_runtime_services(root)`）都在启动时按 root 固化 → **切工作区 = 运行时重装配**（比纯 UI 重，主要成本在此）。另注意 `current workspace` 指针与 desktop/CLI 共享，多壳并行时需决定 per-process 选择语义 |
| 9 | 文件树 / 文件查看（code/markdown/image/pdf） | commands.rs、`search_api` | 已支持（子集） | 文件树 + 文件查看器（`files.rs`：文本/代码、markdown 渲染、图片解码，均设上限与拒绝注记）；pdf 查看未做 |
| 10 | 内容标签页 / Canvas / Mission Control / Quick Look | canvas_api(5) + web-ui 工作台 | 缺 | 多标签工作台，大工程 |
| 11 | 全局搜索（files / sessions / actions / entities） | search_api(3) + web-ui 全局搜索 | 已支持（子集） | files / sessions 两路已落地（`search.rs`）；actions / entities 两路缺 |
| 12 | 编辑器与 inline AI | editor_ai_api(2) + web-ui 编辑器 | 缺 | 壳内无编辑器定位，明确后置 |
| 13 | 终端面板 | terminal_api(14) | 已支持 | 多会话标签 + 停靠分栏 + 命名 profile 编辑（M17；共享 `terminal.*` 键与 desktop 同域） |
| 14 | Git 面板 | git_api(31)、git_agent_api(3)、diff_api(3) | 已支持（子集） | 状态 + diff + 写操作（stage/commit/push 族 + **pull**，2026-10-15）+ history + worktree + 快照恢复 + 信任横幅 + **非仓库态 init 引导**；分支 chip 带 ahead/behind 徽标、行内按文件 discard/delete、文件列↔diff 可拖分隔条（`04a1675c5`）；graph / branches 图形面缺 |
| 15 | Worktree 管理 | worktree_api(8) | 已支持 | 全生命周期（list/create/remove/recreate/promote/bind）经 git 面板落地 |
| 16 | 设置页（21 页 / 5 类） | config_api(21)、i18n_api(5) | 部分（13/21） | 🔶见 §2.1 设置缺口明细 |
| 17 | 定时任务 / 任务看板 | cron_api(5) | 已支持 | 定时任务页 + 看板（`tasks.rs`，core `service/cron` 直调） |
| 18 | 洞察 / 用量报告 | insights_api(5)、token_usage_api(1) | 已支持 | insights 报告 + usage 数字面 + 趋势图/活动热力图（T2-8；热力格 UTC 锚定为有意简化） |
| 19 | 技能管理 / 技能市场 | skill_api(13) | 已支持（子集） | 技能管理页（目录扫描 + 全局启停）+ 每 mode 技能选择（T2-11）；市场下载缺 |
| 20 | 智能体管理（agents / custom agents / subagents） | custom_agent_api(5)、subagent_api、agentic_api | 已支持（子集） | agents 管理页 + 创建/编辑/删除卡片（T2-12，编辑粒度 = 壳创建面六字段）；subagents 编辑器缺 |
| 21 | MCP 管理（servers / resources / prompts / OAuth） | mcp_api(23) | 已支持（子集） | 管理页 + 增删改 + 运行中交互应答（多行 + 预填）+ JSON 原文编辑 + 资源浏览（T2-7）；OAuth 流程缺 |
| 22 | 生态兼容（外部来源导入 / ACP 客户端 / external hooks） | external_sources_api(27)、acp_client_api(16)、external_hooks_api(5) | 缺 | 大工程（发现/冲突解决/批导入） |
| 23 | 插件 / 扩展管理页 | 生态 + 插件运行时 | 缺 | 依赖 22 |
| 24 | MiniApps（运行时 / 库 / 市场 / 提交） | miniapp_api(39)、miniapp_market_api(13)、miniapp_agent_api(5)、miniapp_export_api(1) | 缺 | 需 JS worker 宿主桥 + 市场；大工程，后置 |
| 25 | 页面发布 / 部署（pages） | pages_api(10) | 缺 | web-ui 亦隐藏入口，后置 |
| 26 | 外观市场（appearance packs） | appearance_market_api(9)、market_image_api(1)、account_identity_api(3) | 缺 | 后置 |
| 27 | SSH 远程工作区（连接 / 远程文件 / 端口转发 / WSL） | ssh_api(34) | 缺 | **壳当前声明本地优先**（PLAN 范围）；远程工作区属独立大项 |
| 28 | Remote Connect / 设备中心（QR / 机器人 / 账号） | remote_connect_api(31) | 缺 | 同上，属远程控制场景（web-ui 永续承担） |
| 29 | Peer Device Mode | peer_host_invoke(5) + 命令注册表 deny set | 缺 | 成本高，与产品命令代理基础设施强耦合 |
| 30 | Detached Dispatch | dispatch_api(18) | 缺 | 同上 |
| 31 | 内嵌浏览器 / computer-use / 浏览器控制 | browser_api(8)、browser_control_api(6)、computer_use_api(6)、html_preview_api(2) | 缺（明确不做） | Linux 无 webview 方案；computer-use 为 Windows-only |
| 32 | 托盘 / 通知 / 开机自启 / 单实例 / 系统文件对话框 | system_api(18) | 已支持（Linux） | 托盘（ksni SNI）+ 自启 + 单实例锁 + 关闭语义 + OS 通知（`shell.rs::show_system_notification`，turn done/failed + T1-2 通知开关页）；文件对话框走 gpui 原生 pickers |
| 33 | 应用内更新 UI（检查 / 下载 / 安装提示） | update_api(3) | 已支持 | 引擎（D32）+ UI（检查/下载/安装/重启）+ 每日门控（T1-3）；feed 接入仍按 §4.4 拍板时同批 |
| 34 | 公告 / 更新提示 | announcement_api(6) | 缺 | 可后置 |
| 35 | 桌面宠物 / 工具条模式 / 浮动 mini chat | — | 缺（明确后置） | 与壳定位无关 |
| 36 | Trinity 认知控制台 | trinity_api(26) | 缺 | 薄 RPC 转发；已转正为接力清单 T2-15（D77） |
| 37 | 审查平台 / 深度审查 | review_platform_api(10) + review-team 前端 | 缺（review 族投影已有标签） | 平台本体后置；review 族 agent_type 已有「审核中」标签与子视图（T2-6） |
| 38 | 账号 / 身份（GitHub 登录、市场） | account_identity_api(3) | 缺 | 服务市场能力时一并 |
| 39 | 会话快照 / 回滚（turn 文件 accept/reject） | snapshot_service(22) | 已支持 | 快照恢复经 Git 面板落地（`git.rs` snapshot 族） |
| 40 | 调试 / devtools | debug_api(8) | 不适用 | webview 专属；壳走原生日志 |

### 2.1 设置页缺口明细（第 16 行展开）

壳设置页现状 13 页（`settings.rs` 页表）：通用、外观、模型、终端、权限、自动化、网页搜索、技能、智能体、MCP 服务器、用量统计、归档会话、诊断。对照 web 21 页的缺口：

**R3 处置落地（2026-10-12）**：壳内树已重组为 `settingsRegistry` 同构的五组结构
（应用与界面/AI 与模型/会话与工作区/工具与集成/数据与维护，`SettingsCategory`/`SettingsEntry`
驱动，页面措辞对齐 web 注册表：执行与权限/调用统计/已归档会话/日志与诊断/MCP）。
上表缺口的处置：`workspace.session`→跳转卡（打开全局搜索）、`workspace.worktrees`→
跳转卡（打开 Git 面板并直落 Worktree 视图）；pet/voice/shortcuts/editor/memory/
trinity/desktop-control/acp 八类**不出树**（无假入口；后续补页时按注册表 id 落位）。
技能/智能体为壳侧扩展项（web 无对应设置注册页），归入工具与集成组。
一一对应由 `settings_tree_covers_the_registry_one_to_one` 测试锚定。

| 类别 | 页面 | 状态 |
|---|---|---|
| application | general、appearance、terminal | 已支持（general：启动自启/关闭行为/任务完成通知开关/更新检查每日门控；appearance：语言/主题/appearance id；terminal：4 个共享键） |
| application | pet、voice、shortcuts、editor | 缺（pet/voice 与壳定位无关；shortcuts / editor 后置） |
| ai | models | 已支持（provider/key/模型选择页） |
| ai | memory、trinity engine | 缺（trinity 属 T2-15） |
| workspace | 会话/工作区搜索、worktrees | 缺（会话搜索已走全局搜索面；worktrees 走 Git 面板） |
| tools | execution、automation、webSearch、mcp | 已支持（execution：预设 + 自动批准；automation：hooks 总开关/项目级开关/git co-author，quick actions 经聊天头部菜单只读消费 T2-10；webSearch：provider/HTTP 端点表单 T2-9；mcp：管理页 + 交互 + JSON 编辑 T2-7） |
| tools | acp、desktop-control | 缺 |
| data | usage statistics、archived sessions、diagnostics/logging | 已支持（usage：数字面 + 趋势图/热力图；archived：列表/恢复/删除/全删 T1-4；diagnostics：日志级别 + 会话日志目录 + 导出诊断包 T2-13。壳日志无敏感数据门控，web 的 include_sensitive_diagnostics 开关不复制——该开关在壳内无对应行为，复制即假开关） |

### 2.2 底座复用面速查（"可低成本补"的判定依据）

- **`openbitfun_agent_runtime::sdk::AgentRuntime`（壳已用）** 已暴露：
  `rename_session` / `delete_session` / `archive_session` / `set_session_archived` /
  `update_session_model`(`_selection`) / `update_session_mode` / `list_agent_modes` /
  `start_session_compaction` / `undo_session` / `redo_session` / `rollback_session_to_turn` /
  `fork_session`(`_at_turn`/`_before_turn`) / `generate_session_usage` / `get_session_lineage` /
  `resolve_session_workspace_binding` / `search_workspace_references` /
  `workspace_references_for_message` / `manage_dialog_queue` / `steer_dialog_turn` /
  `run_user_shell_command`；权限族（`pending_permission_requests` / `respond_permission*` /
  `list_project_permission_*`）已用；`workspace_diff()` 已用（Git 面板）。
- **`RuntimeServices`（壳已持有 git 端口）** 端口清单：`filesystem`、`workspace`、`session_store`、
  `events`、`clock`、`terminal`、`remote_exec`、`network`、`git`、`mcp_catalog`、
  `remote_connection`、`remote_workspace`、`remote_projection`、`remote_capabilities`。
  **注意**：多数端口是能力标记（`FileSystemPort` / `WorkspacePort` / `NetworkPort` / `McpCatalogPort` /
  `RemoteConnectionPort` / `RemoteCapabilityPort` 无方法，只做可用性广告）；有方法的端口 =
  `session_store`（resolve_*）、`clock`、`git`（`workspace_diff`，壳已用）、`terminal`、`events`、
  `remote_workspace` / `remote_projection`。文件/工作区等实际能力走 core 服务（下表）。
- **core 服务目录（壳已 import `openbitfun_core`，可直接调用）**：`service/cron`（`scheduled-jobs` 已启用）、
  `service/mcp/server/manager`、`service/config`、`service/workspace`（+ 独立 `service/worktree` 生命周期）、
  `service/workspace_runtime`（多工作区运行时管理）、`service/filesystem`（`FileSystemService`：
  `build_file_tree` / `scan_directory` / `read_text`…）、`agentic/insights`（`InsightsService`）、
  `agentic/persistence`、`service/i18n`。
- **已有先例可照抄的壳内模式**：ConfigService 落盘桥（`settings.rs`）、端口 clone 进 runtime handle
  （`git.rs`）、事件泵 + 翻译（`runtime.rs`）、模态队列（权限对话框）。

### 2.3 布局 parity 矩阵（Phase 3 R1-R6 收口，2026-10-12）

Phase 3（接力计划 R1-R6）把壳的**布局与信息架构**对齐桌面（web-ui）。1:1 尺度 = 布局结构、
信息架构、交互位、视觉语言（同 design token 下），**不是像素级复刻**（渲染器不同，字形/亚像素
差异天然存在）。本矩阵逐行给出壳内锚点 × 桌面对应物 × 状态；「刻意分歧」均经双端并排签字
（截图见下表）或有在案拍板。提交序列：R1a `5b8e393f5` / R1b `3522ac28a` / R1c `167f03f71` /
R1d `74a58df08` / R2a `291f74d30` / R2b `a1f4bcd94` / R2c `167403c79` / R2d `77223da7d` /
R3 `34a3effef` / R4 `006cc8063` / R5 `5da468203`（gpui-frontend 分支）。

| # | 布局锚点 | 壳内实现 | 桌面对应物 | 状态 | 刻意分歧与理由 |
|---|---|---|---|---|---|
| 1 | 顶栏资源 tab 条 | `tabs.rs`（SceneId/SceneDef 注册表 + TabKind::Session/Scene + TabsState 纯逻辑）；渲染在 TitleBar 之下；会话与页面并 tab、可关可切 | `SceneBar` 资源 tab 条（会话/页面并 tab，可关可切可弹出） | 同构 | tab 弹出（pop-out）壳侧未做，资源面板头部保留弹出位语义 |
| 2 | 侧栏导航收敛 | `shell.rs` 导航行 = 智能助理/认知体/任务看板（NavPanel 序）；Files/Git/Insights 注册表行 `in_nav:false` | `registry.ts` 顶层导航 5 场景（智能助理/认知体/任务看板/小应用/扩展与兼容） | 同构（裁剪口径） | 小应用/扩展与兼容不放占位入口（公子拍板「小应用 web 不做」，PLAN B 类不做） |
| 3 | 全局搜索「搜索一切」 | `search.rs::render_overlay`（scrim + 顶部卡，Ctrl K / 点击；空 query 快捷操作行） | 侧栏顶搜索框 + Ctrl K 弹层 + `productActionCatalog` | 同构 | 快捷操作行 = productActionCatalog 的壳侧替身（新建会话/终端/文件/变更/洞察/设置）；actions/entities 两路搜索缺（§2 第 11 行） |
| 4 | 会话列表 = 工作区分组树 | 组头（工作区名 + chevron 折叠 + 组级绿点）+ 顶级行 + lineage 子会话缩进 + 行尾 live 绿点 +「显示更多 +N」（R1c） | 工作区分组树（独立 agent 组/工作区组/子会话缩进/活动点/显示更多） | 同构（两级投影） | 组层只落工作区组（壳单工作窗口无独立 agent 组数据）；孙代隐藏（web 两级投影口径） |
| 5 | 窗口底部全局状态栏 | 左簇用户槽 + trinity 灯芯片（awake/dormant/offline），右簇终端停靠开关 + 工作区切换 + 设置齿轮（R1d） | 窗口级全局状态栏（用户/trinity 芯片/设置齿轮） | 同构 | — |
| 6 | 首页 slogan 面 | 无 tab 落地面：品牌行 + slogan「把如果变成真。」+ 输入卡（R2b，`a1f4bcd94`） | `WelcomeScene`（logo + slogan + 输入框） | 同构 | 打字机轮播不做（静态 = web reduced-motion 形态） |
| 7 | composer 上下文条带 | 卡上方条带（左轨工作区 + mode，右轨本机 + 权限徽标）+ 模型选择器入卡 actions 行（R2a） | `ChatInputWorkspaceStrip` + expanded 布局（textarea 下行模型+发送） | 同构 | 数据全来自现有快照，零新命令；徽标点击开设置页 = web `openDefaultSettings` 语义 |
| 8 | 会话资源面板（文件/变更归位） | 停靠右面板（`h_resizable` splitter，默认 360/min 240/max 720）↔ 主资源 tab 互斥 + opening policy（R2c） | `workspace-resources`（持久导航面板 + 会话 tab 右面板 + 可弹出主 tab） | 同构 | 面板 compact 形态：Files 卸 viewer、Git 卸 diff（弹出即回）；policy = web README 壳侧映射 |
| 9 | 聊天密度（气泡/turn footer/节奏） | 用户气泡（648px 上限 = 72%×900、右对齐、3 行截断点击展开）+ turn footer fork/copy/快照 三钮 + per-entry 行距（R2d） | `UserMessageItem` 配方 + `ModelRoundItem` footer 图标族 + 900px 阅读列 | 同构 | copy 弹 full/result 双档（web `formatTranscriptTurnText` 形态）；快照钮走第 8 行 opening policy |
| 10 | 设置导航覆盖侧栏 + 内容居中列 | `settings::render_nav`（设置场景激活时占用导航列：`Sidebar` = 搜索设置栏 + 五组分类树，页行 30px 高 / 36px 图标位缩进 / 选中 quiet 药丸；查询折成扁平结果行 + 诚实空态）+ `settings::render_page`（gpui-kit 组件自带侧栏收 0 宽、页头 24px 粗标题 + 页副标题、`GroupBoxVariant::Fill` 组卡）+ 壳侧 `settings-column`（`max_w(680px).mx_auto()` 居中，边缘 1px 遮线盖住组件侧栏残留）+ 导航顶栏显示场景标题「设置」 | `SettingsNav`（`nav-registry` 场景导航，激活时替换 `MainNav`）+ `ConfigPageHeader` / `ConfigPageLayout`（`min(100% - 2*clamp(12px,3vw,20px), 680px)` 居中列 + `margin-inline: auto`） | 同构 | 刻意分歧：① 壳页 = 单卡片（壳页行集不映射桌面的 `ConfigPageSection` 子分节：桌面「通用」分「启动与更新 / 窗口与通知」两节，壳是六行一卡），组标题因此不再重复页名；② 行内分隔线、行标签字重（壳 14px regular vs 桌面 semibold）、副标题 14px muted vs 桌面 15px primary = gpui-kit 组件语言（沿用 R3/R5 已签壳自语言口径）；③ 导航列宽 300（壳既有侧栏宽，桌面约 348），组件自身的段间距 12px（桌面 16px）；④ 脏标记（draft registry）不做（壳无草稿注册表）；⑤ 搜索命中 = 页名/副标题/分类/注册 id（壳页行文案在组件内部不可索引，桌面索引还含页内文案短语） |
| 11 | 任务看板 = 概览卡 + 月历 | `tasks.rs`：四计数概览卡 + 周一首 7 列自绘月历（next_run/last_run 打点，today 描边）+「全部任务」分节（R4） | 计划概览卡 + 月历 | 同构 | 月历数据源 = cron 现有字段（不做新聚合查询）；图例带「壳内无历史运行日志」诚实注记 |
| 12 | 技能页 facet 左栏 + 右列表卡 | `SkillsFacetState` + 200px facet 栏（库/来源/探索 + 计数）+ 右列表卡（R4） | `SkillsScene` facet 左栏 + 右列表卡 | 同构 | 市场 = 显式降级卡（T3-17）；用户分组壳未接配置，不伪造 |
| 13 | 视觉语言（radius/选中/hover） | radius 全 token 化（卡片 `theme().radius`、小件 `radius_tokens().sm`）；侧栏选中 = 中性 quiet 药丸 + 主文字（R5） | token 体系同源 + `NavPanel` 行 quiet hover 语言 | 同构 | 内容密度（桌面大标题+卡片 vs 壳密行）为 R3/R4 已签壳自语言；`rounded_full` 装饰圆点不 token 化（壳无 radius=0 场景） |
| 14 | 富转写（工具行/折叠组/子代理身份） | `transcript.rs` 事实解析（族标签 + meta 计数）+ 连续读写执行折叠为「N 次探索与执行」chip（点开可展）+ `subagent_identity.rs`（FNV-1a 同 web 池：20 头像 / 30 昵称）+ 子代理身份卡与 wait/list 名册（M24） | `tool-cards/**` 各族卡面 + `grouping/policies.ts` explore 组（`workspaceGroup.summary`）+ `subagent-identity/**` 与 `assets/subagent-avatars` | 同构 | 身份种子 = 子会话 id（缺链接时退化为 agent 别名），与 web 同解；「运行中」仅在子会话本当跑（`live_sessions`），历史不假活；chip 折叠 = 成员行渲染为空 div（列表项数不变），点击 toggle 后 remeasure 该段 |
| 15 | 会话列表投影（分组切换 + 过滤菜单） | 会话区头前簇 = 分组切换（`ListTree` 分组树 ↔ `List` 全平铺）+ 过滤菜单（排序 最近更新/最新创建/名称 · 显示 全部/未读 · 筛选 运行中 · 重置 · 分区标签平铺一菜单）+ 尾部「全部折叠」（分组态）/「全部标为已读」；全平铺投影 = 跨工作区全局排序、行首带工作区名、行内 menu 原样，无分组头、无「显示更多」上限（`SessionListFilters` + `visible_rows`/`flat_entries`） | `WorkspaceSessionGroupingToggle` + `WorkspaceSessionFilterMenu` + `workspaceSessionView.ts`（六段 + `matchesWorkspaceSessionView` + `compareWorkspaceNavSessions`）+ `WorkspaceListSection layout="flat"` + `SessionsSection workspaceScopes` | 同构（诚实子集） | 刻意分歧：① 排序「状态」不做（壳无 error/attention/completed 会话状态数据）；② 显示「需要处理」不做（`needs_user_attention` 不在 `AgentSessionSummary`）；③ 筛选 工作树/环境/来源 三段不做（壳无 worktree/remote/detached/agentType 数据）；④ web per-workspace localStorage 持久化 → 壳窗口态（关窗即忘）；⑤ web 二级子菜单 → 壳平铺一菜单；⑥ 名称排序按大小写折叠比较（无 locale/numeric collation）；⑦ 全平铺保留子会话缩进（web 平铺流式展开）。**子项数不变量**：两态在同一子项数内换内容（全平铺 = 首子项 + 空占位子项），折叠/过滤/换视图都不动滚动锚点 |
| 16 | 工作区组头操作（卡片动作） | 组头行尾 = ＋新建会话 + ⋯菜单（新建会话 / 文件面板 / 复制路径 / 在文件管理器中打开）；异工作区组先切工作区再动手（`run_group_action` = `workspace_group_plan` + `pending_group_action`，在 `WorkspaceSwitched` 落地） | `WorkspaceItem.tsx` 卡片 hover 簇 `[＋新建会话][文件夹][⋯]` + ⋯ 菜单（新建会话/计划任务/项目权限/端口转发(仅远程)/复制路径/在文件管理器打开/批量管理/重置·删除助理） | 同构（诚实子集） | 刻意分歧：① web hover 显隐 → 壳常显（本 gpui 无父 hover 门控子元素；与会话行 menu 常显同惯例）；②「文件夹」动作移入 ⋯ 菜单（同一动作，省一枚常显钮）；③ 计划任务/项目权限/端口转发/批量管理会话/重置·删除助理 不做（壳无对应面，宁缺勿假）；④ 复制路径走载入的根路径 + 壳剪贴板，在文件管理器打开走 `process_manager` + `xdg-open`（与设置页日志目录同一实现） |
| 17 | 侧栏固定区（搜索 / 导航 / 会话区头） | 搜索触发、五个导航行、会话区头（视图切换 + 过滤 + 新建/归档/刷新/工作区菜单）全放 `Sidebar` 的固定头槽；滚动列表只剩工作区分组——滚到再多工作区，区头与其动作始终可点 | `NavigationPanelHeader`（品牌 + 搜索触发，钉住）→ 滚动容器内 `NavigationPanelContent`（导航项，随滚动移出）+ `StickySectionHeader`（会话区头，CSS `position: sticky` 钉在滚动容器顶） | 同构（钉法不同） | ① gpui 无 `position: sticky`，`gpui-component` 的 `Sidebar` 又只暴露 header/footer 两个固定槽，故壳把整块固定；② 读不到列表滚动量做「滚过才钉」：`ListState` 经 `use_keyed_state` 创建，键含**完整元素 id 栈**（`GlobalElementId`），组件外拿同一 key 只是新建另一个状态——已实测该探针恒为未滚动，故弃用；③ 可见分歧：web 导航行随滚动移出、壳导航行常驻（区头可达性与 web 一致，导航行占用约 180px 列表视口） |
| 18 | 会话渲染富元素（代码高亮/复制块/链接/路径/工具行状态与动作） | `markdown.rs`（syntect 高亮器经 `TextViewDefaults` 安装 + `code_block_actions` 语言标签/复制钮 + `classify_link`/`resolve_path`）+ `shell.rs`（`tool_leading_glyph`、行内 `打开`/`复制`、`open_transcript_path`）+ `files.rs`（`open_path_at` 行跳转、紧凑面板让位查看器） | `MarkdownRenderer`（prism 高亮 + `code-block-toolbar` CopyButton + `a` 前缀分派 `file-link`）· `ToolCardStatusSlot`/`AmbientToolCardHeader contentActions` · ExecProcess 复制命令 | 同构（诚实子集） | ① 高亮引擎 = 壳 syntect（默认语法集无 TS/TSX/TOML；与文件查看器同配色）vs web prism + domain prism token；② 工具栏锚位 = 组件右上动作簇 vs web 全宽 toolbar；③ 打开/复制钮常显（web hover 显隐，同 R8 惯例）；④ 工具行状态文字移出（web ambient 同）；⑤ 复制内容 = 命令全文；结果/输出文本复制与工具卡展开体见「未做」栏 |

**双端对照截图归档**（本目录 `assets/gpui-shell-layout-parity/`，sha256 见文件名旁注；/tmp
会清，以此为准）。拍摄配方：壳 `DISPLAY=:97`（Xvfb），桌面 dev `DISPLAY=:98 pnpm run
desktop:dev`（窗口 1200×800@200,50，:98 屏 1600×900），两端同选 `openbitfun-tokyo-night`
主题；R5 签字图为左壳右桌 720px 高并排拼图，R4 为壳端场景帧。

| 文件 | 内容 | sha256 前 16 位 |
|---|---|---|
| `r5-cmp-1-home.png` | R5 签字：首页/会话（welcome + tab + 侧栏选中语言） | `8180d6bc8134c579` |
| `r5-cmp-2-tasks.png` | R5 签字：任务看板（概览卡 + 月历） | `aa66abb24522a7a9` |
| `r5-cmp-3-settings.png` | R5 签字：设置五组树 | `a941d35a895c88e7` |
| `r5-cmp-4-skills.png` | R5 签字：技能 facet（左栏 + 列表卡） | `acaa2fc815879bb0` |
| `r4-02-tasks-board.png` | R4 交付帧：看板真实 cron 数据（打点/today 描边） | `225c60fd38ecf828` |
| `r4-07-skills-all.png` | R4 交付帧：facet 全列表 + 徽标 | `012d3cccb39ad527` |
| `r4-13-builtin-fixed.png` | R4 交付帧：内置过滤 facet 选中 | `0f96b4d70529309c` |

**R8 侧栏尾项证据**（2026-10-13，Xvfb :91，私有显示号；两轮冒烟：修复前 / 修复后各一次，
`/tmp/r8-smoke/`）：组头 ⋯ 菜单（`r8-01-group-menu.png` `0c47ecf6c57b9f9b`：新建会话/文件/
复制路径/在资源管理器中打开，刻意子集）；文件面板动作（`r8-02-files-panel.png`
`8e9798311b99a959`：停靠右面板 + 路径）；全平铺视图（`r8-03-flat-all.png`
`20a80dbc86a82e12`：跨工作区全局排序 + 行首工作区别名 + 无分组头/无上限）；未读过滤空态
（`r8-04-unread-empty.png` `34dd6d6aaec58cd9`：诚实「没有符合当前筛选条件的会话」+ 过滤钮点亮）；
全部折叠（`r8-05-collapse-all.png` `12e81e4a14360829`）；滚动锚点（`r8-06-scroll-anchor.png`
`72f19b27f6a55720`：滚到底后折叠组，前四行 y 坐标不变 = 未跳顶）；异工作区先切后动
（`r8-07-foreign-switch-files.png` `8b926b90dfc7d976`：日志链 `Group action for workspace …:
switching before acting` → `Active workspace switched` → `Opening the files panel of the
switched workspace`，面板标题即切换后工作区路径）；＋新建会话不带折叠副作用
（`r8-08-plus-no-fold.png` `fec1113b9e22b935`：修 `cx.stop_propagation()` 后复测）；名称排序
（`r8-09-order-name.png` `7e6622fd1c824c17`）。复制路径实测 `xclip -o` 回读剪贴板 = 触发组
工作区根路径；两轮 0 panic、0 missing asset；冒烟建的两个临时工作区（`pm-a`/`pm-b`）与
工作区注册表条目、会话存储目录均已清理；「全部标为已读」已接线
（`RuntimeEvent::TrayMarkAllRead`）但本轮无后台未读可验，留待真机。

**R9 设置布局 1:1 证据**（2026-10-14，提交 `d4920fe35`，Xvfb :131 私有号，壳窗口 1200×800@200,50，
`OPENBITFUN_GPUI_SETTINGS_AUTOOPEN=1` autoopen；对照 = 归档并排图 `r5-cmp-3-settings.png` 的桌面右半
（桌面 UI 自 R5 未变），并排 `r9-03`）：

- `r9-01-settings-nav-general.png` `2ae6889f080d4539`：设置场景激活时**左列整列为设置导航**
  （搜索设置… + 应用与界面/AI 与模型/会话与工作区/工具与集成/数据与维护五组标题 + 缩进页行，通用选中药丸），
  导航顶栏 = `[面板][←][→] 设置`；右侧内容 = 680px 居中列（实测列 482..1162，居中期望 481.5），
  页头 24px 粗标题 + 副标题，组卡内行 = 标签/描述左 + 控件右。
- `r9-02-settings-nav-click-terminal.png` `770c33fb477528b7`：点导航行 → 内容整页切换
  （终端页：字号/字体/回滚行数/Shell 命令/终端 Profile；导航药丸随动）= 实机点击路径证据。
- `r9-03-settings-side-by-side.png` `e9c1cb88f3e8b29d`：左壳右桌面并排（两侧同缩放到 800px 高）：
  导航结构、页头文案与字号、居中列宽、行内控件位一致。
- 同一轮冒烟逐页点击扫过 15 个设置页（0 panic、0 missing asset；日志仅既有 xinput/wgpu 噪声），
  顺带修掉两处行内长文案溢出（控制列 280px 界宽内换行）与「搜索栏输入不触发重绘」
  （`InputEvent::Change` → `cx.notify`）。

headless 锚点：`settings_scene_nav_selects_pages_and_searches`（树 15 行渲染 / 选页切换 / 查询折结果行 /
无命中空态 / 列宽 ≤ 680）与 `settings_nav_projects_the_tree_and_the_search`（投影：五组分类树 + active 标记 +
扁平行 + 空态文案）。

**M24 富转写证据**（2026-10-13，Xvfb :81，真会话 `3261d9fc`「接力 M3」autoopen 渲染；归档帧
`m24-01-transcript-top.png` `ed3cd856dba0736d` / `m24-02-group-open.png` `3ba4bed7dcec1009` /
`m24-03-agent-rows.png` `0a2b2c1080cd7556`）：
壳 `OPENBITFUN_GPUI_AUTOOPEN_SESSION=<id>` 直开该会话，折叠 chip（8/48/3/1 次探索与执行 +
读取/命令计数）、点开后的语义工具行（运行命令/读取文件/退出码/行数）、子代理卡（鸭鸭
zigzag-duck · 阿 view-input · SwarmWorker）、AgentWait 失败名册（3 个失败 + 三头像）、
AgentList（3 个子代理 · 0 个运行中）全部渲染，展开/收起往返一次，0 panic（唯一 ERROR 为
会话内失效图片资源 + 启动期 wgpu 一次重试）。截图 `/tmp/gpui-m24/03-top.png`…`08-collapse-single.png`。

**会话渲染富元素 1:1 证据**（2026-10-14，公子点名「对比原来的会话渲染实现——高亮/链接/路径/卡片/状态/
快捷复制块/子代理卡片 1:1 复刻」；Xvfb :137 私有号；素材 = `examples/seed_transcript.rs` 种子会话
（离线导入路径，工作区 `/tmp/md-smoke-ws`，冒烟后已删）`OPENBITFUN_GPUI_AUTOOPEN_SESSION` autoopen）：

- `md-01-code-toolbar.png` `0df062fb3b50abdf`：助手 markdown 富渲染 —— 标题/段落、**文件链接
  `src/demo.rs`（文件族点击样式）**、外部链接 `https://example.com`、```rust 代码块 = syntect 高亮 +
  右上「Rust + 复制」工具栏、连续调用折叠 chip（读取1·命令2·1 个失败）、Write 行带「打开」钮。
- `md-02-rows-expanded.png` `3312f3bd0de3edf1`：chip 点开后的工具行 —— 命令行（族图标 + `复制` 钮 +
  `退出码 0`）、Read 行（`打开` 钮 + 行窗/行数 chip）、**失败命令行 = 危险色 ✕ 字形**、Write 行。
- `md-03-copied.png` `ec625fa2c4231541`：代码块复制钮的 2s 勾选反馈（`xclip -o` 回读 = 整段代码）。
- `md-04-file-jump.png` `e52371a30c33aca0`：markdown 文件链接 `[src/demo.rs](src/demo.rs#L40)` →
  右面板打开该文件并**定位到第 40 行**（相对路径按活动工作区根解析）。
- `md-05-row-open.png` `7c435296be0ada9f`：工具行「打开」钮 → 同一查看器打开该文件（无行号定位）。
- 交互实测：外部链接点击 → 日志 `Opened external URL https://example.com`（走 `process_manager` +
  `xdg-open`，不经 ashpd/zbus）；命令行复制 → `xclip` 回读**完整多行命令**（非行内截断标题）。
- 本轮**顺带修既有 bug**：文件查看器正文空白 —— `gpui-pre` 的 `list` 元素按调用方样式定尺，不传
  显式尺寸即塌成零高盒、零行布局（`MessageScroller` 同款 `.size_full().min_h_0()` 是正解）；
  修后查看器渲染行号 + 高亮文本，并有 `files-code-row` 布局断言钉住。

**矩阵第 18 行（会话渲染富元素）**：壳内锚点 = `markdown.rs`（syntect `code_block_highlighter` 经
`TextViewDefaults` 安装 + `code_block_actions` 工具栏 + `classify_link`/`resolve_path` 链接路由）+
`shell.rs`（`tool_leading_glyph` 状态字形、行内 `打开`/`复制` 动作、`open_transcript_path`）+
`files.rs`（`open_path_at` 行跳转、紧凑面板「有文件即让位给查看器」）。桌面对应物 =
`MarkdownRenderer`（prism 主题 + `code-block-toolbar` CopyButton + `a` 前缀分派/`file-link`）·
`ToolCardStatusSlot` + `AmbientToolCardHeader contentActions` · `ExecProcess` 复制命令。**刻意分歧**：
① 高亮引擎 = 壳 syntect（默认语法集无 TS/TSX/TOML；与文件查看器同一配色）vs web prism-async-light +
domain prism token；② 工具栏锚位 = 组件右上动作簇（语言标签与复制钮并排）vs web 全宽 toolbar；
③ 打开/复制钮**常显**（web hover 显隐；同 R8 侧栏惯例）；④ 工具行不再显示状态**文字**（web ambient
同为图标/动效，文字只在 tooltip/aria）；⑤ 待批准行 = 族图标染警告色（web 同义）；⑥ 已取消行 = 族图标 +
0.6 透明度（web `opacity-muted`）。

## 3. 缺口分级（已拍板，2026-10-09：P0 立项为里程碑 M11）

**P0 — 默认切换的最小集**（不补则无法作为日常主入口；均为"端口现成、壳侧装配"类；**2026-10-09 公子拍板：立为里程碑 M11 开工**）：

1. **工作区打开/新建/最近工作区**（已支持列表见 §2 第 8 行）；
2. **模型 / Provider 选择**（含 models 设置页最小版）；
3. **会话管理基础操作**：重命名 / 删除 / 归档（导出、fork、压缩可随后）；
4. **附件 / 图片上下文**；
5. **系统集成最小集**：桌面通知（gpui-pre 原生，见 §2 第 32 行）+ 单实例 + 关闭行为（托盘按公子取舍）；
6. **交互式提问（ask_user）对话框**（SDK 与装配已通，见 §2 第 5 行；与权限队列同型，防死锁铁律）。

**P1 — 切换后可跟进**（有降级路径，不阻塞主入口）：

- 设置页其余类别（shortcuts / editor / automation hooks / webSearch / mcp / acp / data）；
- 文件树与文件查看、content 资源标签、全局搜索；
- Git 写操作与 history / branches / graph；worktree 管理；快照回滚；
- skills / agents / MCP 管理 UI；生态兼容导入；
- 子代理投影 / BTW / 任务详情；usage/insights；归档会话页；定时任务看板；公告；
- 终端多会话标签与 profile 编辑。

**P2 — 明确后置或不做**（与壳定位无关或与 PLAN Out of scope 一致）：

- 内嵌 webview 浏览器、computer-use、浏览器控制（Linux 无方案 / Windows-only）；
- 桌面宠物、工具条模式、浮动 mini chat；
- MiniApps 运行时与市场、页面发布、外观市场（大工程，且依赖 JS 宿主桥）；
- Trinity 控制台、审查平台、账号/市场（产品决策后再议）；
- SSH 远程工作区、Remote Connect、Peer Device、Dispatch（web-ui 永续承担；若要支持属独立里程碑）。

## 4. 默认切换机制设计（待公子拍板）

### 4.1 事实基线

- **两个壳 = 两个独立二进制 / 两个交付物**：Tauri 桌面（`productName: OpenBitFun`，bundle targets all）与
  `openbitfun-gpui-shell`（tar.gz + 独立 deb）；GUI toolkit 不同，**不可能同进程切换**。
- **共享数据面**：两壳使用同一 data namespace、同一用户配置与 session 存储（D13 / D27 / D28）。
  因此切换**不需要迁移任何数据**，可并存、可回退，回退 = 改回启动哪个二进制。
- **产品规则**（[产品定制蓝图](../architecture/product-customization-blueprint.md) §2 / §7）：GUI 宿主 / layout 选择是
  **构建期产品事实**；"运行时选产品"被明确排除；用户配置与插件不得改变交付形态或产品身份。

### 4.2 候选机制

| 方案 | 内容 | 评价 |
|---|---|---|
| M-A 并列发布 | 两个包并存、两个 `.desktop` 入口，用户自行选择启动；默认仍是 Tauri | 现状即可维持；零新机制；"默认"未改变 |
| **M-B 产品定义增 GUI 宿主选择（已选定，2026-10-09 公子拍板）** | 产品定义 desktop member 增加"默认 GUI 宿主"引用（= 蓝图 C1 的已注册 shell/layout 选择切片）；打包 adapter 据此决定桌面入口/默认包指向哪个二进制；未注册 ID / 能力冲突构建失败 | 与蓝图一致（§7 明示 GUI 可由产品定义选"已注册 shell/layout"）；需要真实消费者（build adapter + 打包脚本消费者），是本项的主要工作量。**现状锚点**（2026-10-09 复核）：schema v1 只收已消费字段且 `additionalProperties: false`（`products/schemas/product-definition.schema.json`）、默认产品 `products/openbitfun/product.jsonc`、解析器 `scripts/product-customization/resolver.mjs`、现有 desktop 消费者 `scripts/desktop-tauri-build.mjs:322-327`（覆盖 `productName` / `mainBinaryName` / `identifier`）、校验入口 `pnpm run product:check` / `product:test`。M-B 落地 = schema + resolver + 定义文件（含 fixtures）+ adapter 消费方 + 测试 |
| M-C 运行时选择器 | 同一启动器按用户配置选二进制 | 与蓝图 §2 冲突（用户配置不得改变交付形态）；不建议作为产品级默认机制；至多作为 M-A 上述两位用户的便捷 UX |

### 4.3 切换退出条件（建议）

默认切换动作需要同时满足（切换本身必须公子拍板，D6 / D19；**机制已选定 M-B、P0 已立项 M11**，切换动作仍待 P0 闭合后的终审拍板）：

1. §3 P0 清单闭合且逐条有真机证据；
2. 机制落地 = M-B（或在 M-B 未就绪时以 M-A 形式"并列发布 + 用户文档"，此时默认不变）；
3. 保留 Tauri 桌面入口至少一个发布周期作为回退路径；
4. 不做：不迁移/不删除任何数据、不静默改变默认、不在壳内实现"重启进另一个壳"。

### 4.4 feed 接入与签名（关联待拍板项）

更新通道引擎已就绪（D32），feed 接入 = `scripts/generate-linux-binaries-manifest.mjs` 增 `gpuiShell` 资产键
（与现有 `cli` / `relay` 并列一行；该脚本的 `asset()` 会强制要求发布资产目录里同时存在
`openbitfun-gpui-shell-<version>-<target>.tar.gz` 与其 `.sha256`，打包脚本已产出）+ 发布工作流产物 +
可选 minisign 签名（CLI 先例：`.sha256.sig` / `.sig` 存在即自动写入 `sha256SigUrl` / `sigUrl`）。
**已拍板（2026-10-09 公子）**：默认未切换前不占用产品发布通道，维持显式
`--update-manifest` 源；**拍板切换时同批完成 feed + 签名**（`sigUrl` 从 warn 升级为校验），作为切换动作的组成部分。

## 5. 方法学与刷新

- 命令计数：`grep -r '#\[tauri::command\]' src/apps/desktop/src | wc -l`；域归组按 `api/` 文件名 + 命令名前缀，
  逐文件标注"产品能力 / 纯管道 / 混合"。
- 场景面以 `registry.ts` 与 `settingsRegistry.ts` 为准；次要面板按 web-ui 真实挂载点归并。
- 刷新时机：每次较大能力落地后追加一行"更新记录"，或 M10 收口时整体复核；本文件首个基线 = 2026-10-09 @ `7c6586971`。
- 三态证据要求：标"已支持"必须能指向壳内模块；标"可低成本补"必须能指向现成端口/SDK 方法。
- 复核方法（2026-10-09 整固轮）：SDK 方法对照 `agent-runtime/src/sdk.rs` 实际签名；core feature 闭包用
  `cargo tree -p openbitfun-gpui-shell -e features -i openbitfun-core` 核；gpui 原生能力对照 vendored
  `gpui-pre 0.3.8` / `gpui-pre-linux 0.3.8` 源码（XDND、wl_data_device、`ClipboardItem`、通知）；
  设置页面对照 `settingsRegistry.ts`，配置键对照 `config-contracts` 的 `TerminalConfig` 等结构；
  §4 机制锚点对照 `products/`（schema + 定义 + fixtures）与 `scripts/product-customization/`、
  `scripts/desktop-tauri-build.mjs`、`scripts/generate-linux-binaries-manifest.mjs`。

## 6. 更新记录

| 日期 | 变更 |
|---|---|
| 2026-10-09 | 首个基线（M10 第 1 会话）：704 命令域统计、19 场景矩阵、P0/P1/P2 分级、M-A/B/C 机制设计 |
| 2026-10-09 | **三项拍板（M10 收口，公子亲手选定）**：① P0 立项为里程碑 M11（验收于开工时冻结）；② feed 维持显式 `--update-manifest`，切换拍板时同批接入 + minisign；③ 机制 = M-B（落地为里程碑 M12）。切换动作本身待 P0 闭合后终审 |
| 2026-10-09 | 整固复核（M10 第 2 会话，仅文档）：①逐条复核 §2「可低成本补」6 行 —— SDK 方法族（第 2/3/5/18 行）、端口、core 服务全部真实存在，无一虚标；②修正证据措辞 4 处：第 4 行改为「拖放/剪贴板走 gpui-pre 原生（XDND / wl_data_device / `ClipboardItem::new_image`）＋图片落盘助手为 desktop-local」、第 5 行补「装配已通」的直接证据（`build_with_event_source` 注入 `AgentInteractionResponsePort`）、第 8 行改为「切工作区 = 运行时重装配」（ownership 与 local services 启动即固化）并标出 `current workspace` 共享指针的跨壳耦合、§2.2 修正「多数端口是能力标记（无方法）」并把文件/工作区能力指向 core 服务；③补证据锚点：第 3 行补 catalog 三个 core 函数、第 17 行补 `scheduled-jobs` 已在闭包、第 18 行补 `InsightsService` 五个方法；④第 32 行补「通知可走 gpui-pre 原生 `show_system_notification`」；⑤§2.1 application 分类修正为 7 页（补 `terminal`，壳已覆盖其 4 个共享键）、去掉不存在的独立「appearance pack」页；⑥抽查「缺」档三行的证据锚点（第 15 行 `service/worktree` 生命周期、第 21 行 `McpManager` 家族、第 39 行 `service/snapshot`），去掉对标记端口的引用；⑦第 9 / 11 行同样把「端口现成」改为指向 core 服务（`FileSystemService` / `WorkspaceSearchService`）；⑧§4.2 / §4.4 补落地锚点：M-B 的 schema/resolver/adapter/校验入口逐文件点名，feed 接入点名 `generate-linux-binaries-manifest.mjs` 的 `gpuiShell` 资产键及其 `asset()` 存在性约束 |
| 2026-10-10 | M11-M23 落地提示（接力计划侧记录，§2 矩阵尚未逐行刷新）：P0 六项与 P1 大部分已由 M11-M23 交付——会话管理操作、模型页、附件、ask_user、文件树/查看器、全局搜索（files/sessions）、终端多标签 + profile、Git 写操作 + history + 信任横幅、worktree 全生命周期、快照恢复、tasks、insights/usage 数字面、skills/agents/MCP 管理页、子代理投影 / BTW / child views、托盘 + 自启 + 关闭语义、自更新 UI、OS 通知（turn done/failed）。逐行复核记入接力清单（.relay/PLAN.md 后置接力清单 T2-14）；残余差距清单同处（git graph/branch、content 标签、公告、搜索 actions/entities、生态导入等） |
| 2026-10-11 | **§2 矩阵整轮刷新（T2-14，接力清单项）**：逐行刷到 2026-10-11 真相（M11-M23 + 后置接力 T1-1..5 / T2-6..13 落地后）。已支持/子集：会话管理（2）、模型（3）、附件（4）、ask_user（5）、子代理投影/BTW（6，近似口径 D67）、文件树/查看器（9，pdf 缺）、全局搜索（11，files/sessions）、终端（13）、Git（14，graph/branches 缺）、worktree（15）、定时任务（17）、洞察/用量（18）、技能/智能体/MCP 管理（19/20/21，市场/编辑器/OAuth 缺）、系统集成（32）、更新 UI（33）、快照（39）；设置页 3/21 → 13/21（§2.1 改写，含 T2-13 诊断包导出与 include_sensitive_diagnostics 假开关不复制注记）。§1 基线表未动（M10 交付物口径不变）；残余缺口：git graph/branches、搜索 actions/entities、content 标签、生态导入、Trinity（T2-15）、托盘未读角标（T2-16）等 |
| 2026-10-12 | **§2.3 布局 parity 矩阵新增（Phase 3 R6 收口）**：布局/信息架构逐行锚点矩阵 13 行（R1a-R5 全部锚点 × 桌面对应物 × 同构/刻意分歧+理由）；双端对照签字截图 7 张归档 `assets/gpui-shell-layout-parity/`（含拍摄配方与 sha256）。Phase 3 R1-R6 主线收官；R7（插件功能+UI 一体架构）与 T3 大项维持「点名才动」；默认切换终审（§4.3）待公子重开 |
| 2026-10-13 | **真机 parity 修正四项（公子看真机后点名）**：桌面 `92ce04f8d`（simplify desktop shell UI）后拓扑已变——无独立 TitleBar/横贯底栏、侧栏 NavBar+footer 贯穿左列、SceneTopBar 带 tabs+场景动作+窗口钮。壳随之重排：分列式 45px 双顶栏、侧栏抽屉折叠（Ctrl B + 左上悬浮条）、tab 移右顶栏（横向滚动不覆盖动作区）、会话列表单行 30px、选中(selection)/hover(muted) token 分离、inline code 高亮 accent→neutral 弱底（`theme.rs::install_text_view_style` 随主题应用重装 TextViewDefaults）、composer 圆形发送钮 + placeholder「有什么可以帮您...」+「+」boost 菜单。壳 251 测绿，Xvfb :73 截图对照 0 panic |
| 2026-10-14 | **会话渲染富元素 1:1（公子点名「高亮/链接/路径/卡片/状态/快捷复制块/子代理卡片」）**：助手 markdown 三条钩子全接（`code_block_highlighter` 经 `TextViewDefaults` 装 syntect 高亮、`code_block_actions` = 语言标签 + 复制钮 2s 勾选反馈、`on_link_click` = http(s) 走 `process_manager`+`xdg-open`／文件路径按工作区根解析并激活文件查看器）；工具行 = 状态字形化（运行=转圈、失败=✕、待批准=警告色族图标、取消=0.6 透明）+ 行内「打开」（Read/Write/Edit）与「复制」（命令全文）动作；`files.rs` 增 `open_path_at` 行跳转、紧凑面板有文件时让位给查看器、并修掉 `gpui-pre` list 零尺寸导致的**查看器正文空白**（既有 bug）。壳 289 测绿（+4：链接分派/语言标签/高亮区间/工具栏与行动作布局断言 + files-code-row）；i18n 637×3；证据 `md-01..05`（§2.3 证据段） |
| 2026-10-14 | **R9 设置布局 1:1（`d4920fe35`，公子点名：左侧项覆盖侧栏、内容右侧居中）**：设置场景改为「场景导航列」——设置导航替换侧栏（`settings::render_nav`：搜索设置 + 五组分类树，`SettingsNav` 同构；查询折平结果 + 诚实空态）、内容列 680px 居中（`ConfigPageLayout` 同构）、页头改 24px 粗标题 + 新增 15 条页副标题文案（×3 语言，ftl 616→633）、组卡 `GroupBoxVariant::Fill`、去掉重复页名的组标题；顺带修控制列长文案溢出与导航搜索不重绘。壳 282 测绿（+1 无头导航测试、+2 投影断言）；证据 `r9-01..03`（§2.3 证据段）。刻意分歧按矩阵第 10 行「理由」栏在案（单卡片无子分节 / 行内语言随组件 / 列宽 300 / 无脏标记 / 搜索索引面） |
| 2026-10-13 | **§2.3 矩阵 R8 两行（会话列表投影 + 工作区组头操作）**：侧栏 1:1 尾项落地——分组切换 grouped/all（全平铺跨工作区全局排序 + 行首工作区名 + 无折叠上限）、过滤菜单（排序/显示/筛选 运行中/重置/全部折叠/全部标为已读）、组头 ＋/⋯（新建会话/文件面板/复制路径/在文件管理器打开，异工作区先切后动）。刻意分歧按矩阵各行「理由」栏在案（web-only 段：状态排序、需要处理、工作树/环境/来源、计划任务/项目权限/端口转发/批量管理/助理重置删除）。子项数不变量跨视图保持（平铺 = 首子项 + 空占位子项）；Xvfb 冒烟见 §6 次行 |
| 2026-10-13 | **§2.3 矩阵第 17 行（侧栏固定区）**：公子点名「要补」——会话区头与视图切换/过滤钮在侧栏滚动后不可达。修法 = 搜索/导航/会话区头整块进 `Sidebar` 固定头槽（列表只剩工作区分组，顺带把子项数常量 +2 也去掉）。先试过「读组件列表偏移、滚过再钉同款行」：`ListState` 键含完整元素 id 栈，壳外读不到（探针实测恒为未滚动），弃用；同源分歧（导航行不随滚动移出）已记入该行 |
| 2026-10-15 | **A 批 1 落地**（`04a1675c5`，见 `gpui-shell-ui-parity-remediation-plan.md` §2.3/§2.4）：Git 面板补 pull 与 ahead/behind 徽标、非仓库态 init 卡（按钮把 prompt 交给 composer）、按文件 discard/delete（两段式确认）；点 14 行状态同步；`GitPort` 增 `pull`/`discard_files`，`GitRepositoryOverview` 增 `ahead`/`behind`（并修掉 overview remotes 恒空） |
| 2026-10-15 | **界面逐一 1:1 差距全量盘点 + 整改计划另立新文件**（`gpui-shell-ui-parity-remediation-plan.md`，本文件 §2/§2.3 的能力域与布局矩阵仍为真相源，新文件是其上的界面粒度展开）：逐界面核对双端源码锚点，新增死面考据（GitNav branches/graph、ShellNav、file-viewer scene、trinity-memory 在 web 侧不可达，不作为 1:1 目标）、8 个设置缺页与从未立项的新缺口（助理 persona 面、记忆设置页、Trinity 引擎页、语音输入）、Git pull/ahead-behind 与 not-a-repo 态、任务编辑器、洞察图表/TOC 等；A/B/C 分档 + 每项锚点/动作/验收口径，并登记 T3 17-25 与本文编号的映射。§2 若干行（行 17/18/19/20/21）待下次整表刷新时按新文件差额改判为「子集」 |
