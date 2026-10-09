# GPUI 桌面壳 parity 矩阵与默认切换计划

本文件维护 `src/apps/gpui-shell`（GPUI 原生桌面壳）相对 Tauri 桌面壳的能力差异、缺口分级与默认切换机制。
它是接力计划 M10 的产出（gpui-frontend 分支，过程计划不随仓库发布）。基线：2026-10-09 @ `7c6586971`
（Phase 2 M7-M9 全部完成）。相关设计：

- 成功标准与不做项：本地接力计划 `.relay/PLAN.md` 的 D6 / D19 / D20（该文件不入库，读者看本文件即可）；
- GUI 宿主选择的产品规则：[产品定制与跨界面组装设计](../architecture/product-customization-blueprint.md) §2 / §7；
- CLI 平替先例：[CLI 产品线设计](../architecture/cli-product-line-design.md)；
- 场景面真相源：`src/web-ui/src/app/scenes/registry.ts`；命令面：`src/apps/desktop/src/api/`。

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
| 1 | 会话对话闭环 | agentic_api(58) 的 turn/session 子集 | 已支持 | 会话级事件流、工具卡、停止、标题齐备 |
| 2 | 会话管理操作（重命名/删除/归档/导出/fork/压缩） | session_api(19)、agentic_api | 可低成本补 | SDK `AgentRuntime` 已暴露 `rename_session` / `delete_session` / `archive_session` / `set_session_archived` / `fork_session`(`_at_turn`/`_before_turn`) / `start_session_compaction` / `get_session_lineage`；导出 = `read_session_transcript` + 序列化。壳侧 = 会话菜单 + 调用 |
| 3 | 模型 / Provider 选择 | `get_ai_model_catalog` + config 模型档案 | 可低成本补 | 壳现仅显示 `model_id`（只读）；SDK 已有 `update_session_model(_selection)`、`list_agent_modes` / `update_session_mode`；catalog 接缝 = `openbitfun_core::get_remote_model_catalog()` / `get_local_models_dev_catalogs()` / `get_models_dev_catalog_status()`（desktop `get_ai_model_catalog` 同款；`model-catalog` 已在壳 feature 闭包）。需选择器 UI + 落盘 |
| 4 | 附件 / 图片上下文（粘贴图片、拖文件、上传） | clipboard_file_api(3)、context_upload_api(1)、file_drop_preview_api(1) | 可低成本补 | turn 请求已带 `attachments: Vec<AgentInputAttachment>`（`remote_image` / `image_context`，dataURL 或路径）；**拖放与剪贴板走 gpui-pre 原生**（`FileDropEvent`：X11 XDND + Wayland wl_data_device；`ClipboardItem::new_image`），arboard 非必需（`file_drop_preview_api` 是 webview 拖拽预览，壳侧不需要）。图片落盘助手 `prepare_image_context_for_storage` 现为 desktop-local（壳内复刻约 30 行或下沉 core）；壳侧 = composer 附件区 + 缩略图 |
| 5 | 交互式提问（ask_user / user question） | tool_api(8) 的 question 两条 | 可低成本补 | SDK 已备 `start_user_question_interaction` / `submit_user_answers` / `cancel_user_question` + `PendingUserQuestion`（含 `pending_question_snapshot(session_id)`）；**装配已通**——壳用的 `CoreProductAgentRuntime::build_with_event_source` 已把 coordinator 作为 `AgentInteractionResponsePort` 注入。壳侧 = 提问对话框 + FIFO 队列，与权限队列同型（同一死锁铁律） |
| 6 | 子代理投影 / BTW 侧会话 / 任务详情 | subagent_api(10)、btw_api(2) | 缺 | 壳当前丢弃子代理事件（D14）；需新视图族 |
| 7 | 语音输入 / 实时通话 | speech_api(18) | 缺 | ASR 模型管理 + 实时音频，大工程 |
| 8 | 工作区打开 / 新建 / 最近工作区 | commands.rs(79) 的 workspace 子集 | 缺 | **壳固定 `current_dir()` 为工作区（`runtime.rs`）**；core `WorkspaceService`/`WorkspaceManager` 能力齐（open/quick_open/create/switch/close/get_recent/get_opened/list/search），多工作区运行时由 `WorkspaceRuntimeService::ensure_runtime_for_workspace_binding` 管；但壳的 ownership（`fixed_workspace`）与 local services（`build_local_runtime_services(root)`）都在启动时按 root 固化 → **切工作区 = 运行时重装配**（比纯 UI 重，主要成本在此）。另注意 `current workspace` 指针与 desktop/CLI 共享，多壳并行时需决定 per-process 选择语义 |
| 9 | 文件树 / 文件查看（code/markdown/image/pdf） | commands.rs、`search_api` | 缺 | core `FileSystemService`（`read_text` / `build_file_tree` / `scan_directory`）现成；viewer 面 + 资源标签是主要成本 |
| 10 | 内容标签页 / Canvas / Mission Control / Quick Look | canvas_api(5) + web-ui 工作台 | 缺 | 多标签工作台，大工程 |
| 11 | 全局搜索（files / sessions / actions / entities） | search_api(3) + web-ui 全局搜索 | 缺 | core `service/search`（`WorkspaceSearchService`：`build_index` / `search_content` / `glob`）+ `product-search`（会话搜索）现成；壳侧搜索面中等 |
| 12 | 编辑器与 inline AI | editor_ai_api(2) + web-ui 编辑器 | 缺 | 壳内无编辑器定位，明确后置 |
| 13 | 终端面板 | terminal_api(14) | 已支持（子集） | 单停靠面板；缺多会话标签 / 终端列表 / profile 编辑 |
| 14 | Git 面板 | git_api(31)、git_agent_api(3)、diff_api(3) | 部分（只读） | 已有：状态 + diff。缺：history / branches / graph / stage / commit / push / 信任提示 |
| 15 | Worktree 管理 | worktree_api(8) | 缺 | core 独立 `service/worktree` 生命周期模块（`list` / `create` / `remove` / `recreate` / `promote` / `bind_session`…）与 `service/workspace` 的 worktree topology 均在；管理 UI 中等 |
| 16 | 设置页（21 页 / 5 类） | config_api(21)、i18n_api(5) | 部分（3/21） | 🔶见 §2.1 设置缺口明细 |
| 17 | 定时任务 / 任务看板 | cron_api(5) | 可低成本补 | core `service/cron` 现成（`CronService` / 变更事件 / 全局服务）；`scheduled-jobs` 已由 `tools-agent-control` 传递启用，在当前壳 feature 闭包内直接可用；看板 UI 中等 |
| 18 | 洞察 / 用量报告 | insights_api(5)、token_usage_api(1) | 可低成本补 | core `agentic::insights::InsightsService`（`generate` / `has_data` / `load_report` / `load_latest_reports` / `cancel`，desktop insights_api 同款）+ SDK `generate_session_usage`；读报告 + 统计展示 |
| 19 | 技能管理 / 技能市场 | skill_api(13) | 缺 | 管理 UI 中等；市场下载属网络流程 |
| 20 | 智能体管理（agents / custom agents / subagents） | custom_agent_api(5)、subagent_api、agentic_api | 缺 | 卡片 + 创建向导，工作量中 |
| 21 | MCP 管理（servers / resources / prompts / OAuth） | mcp_api(23) | 缺 | core `service/mcp` 的 `McpManager`（lifecycle `start/stop/restart`、catalog 资源/提示、OAuth、interaction 应答）现成（`mcp_catalog` 端口本身是能力标记）；管理面 UI + 运行中交互对话框为主要成本 |
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
| 32 | 托盘 / 通知 / 开机自启 / 单实例 / 系统文件对话框 | system_api(18) | 缺 | 通知已可走 **gpui-pre 原生**（`App::show_system_notification`，Linux = XDG D-Bus/notify-rust，带 tag/action 响应回调）；托盘、单实例、开机自启仍需散件重建（rfd / notify-rust 等，M9 已列台账） |
| 33 | 应用内更新 UI（检查 / 下载 / 安装提示） | update_api(3) | 部分 | 引擎已就绪（D32）；缺 UI 与 feed 接入（见 §4.4） |
| 34 | 公告 / 更新提示 | announcement_api(6) | 缺 | 可后置 |
| 35 | 桌面宠物 / 工具条模式 / 浮动 mini chat | — | 缺（明确后置） | 与壳定位无关 |
| 36 | Trinity 认知控制台 | trinity_api(26) | 缺 | 薄 RPC 转发；是否承载属产品决策 |
| 37 | 审查平台 / 深度审查 | review_platform_api(10) + review-team 前端 | 缺 | 后置 |
| 38 | 账号 / 身份（GitHub 登录、市场） | account_identity_api(3) | 缺 | 服务市场能力时一并 |
| 39 | 会话快照 / 回滚（turn 文件 accept/reject） | snapshot_service(22) | 缺 | core `service/snapshot`（`SnapshotSystem` + 文件锁/隔离/清理）现成；与 diff 面同族，可随 Git 面板演进 |
| 40 | 调试 / devtools | debug_api(8) | 不适用 | webview 专属；壳走原生日志 |

### 2.1 设置页缺口明细（第 16 行展开）

| 类别 | 页面 | 状态 |
|---|---|---|
| application | general（启动/更新/睡眠/窗口/通知）、appearance（含外观包区块）、pet、voice、shortcuts、terminal、editor | 缺（壳设置页已覆盖 appearance 的语言/主题与 terminal 的 4 个共享键：`font_size` / `font_family` / `scrollback` / `default_shell`；general / pet / voice / shortcuts / editor 缺） |
| ai | models（providers/keys/proxy/订阅）、memory、trinity engine | 缺（models 属 P0，见 §3） |
| workspace | 会话/工作区搜索、worktrees | 缺 |
| tools | execution（壳已有预设 + 自动批准两项；超时/审查容量缺）、automation（hooks / quick actions）、webSearch、mcp、acp、desktop-control | 部分（2/execution） |
| data | usage statistics、archived sessions、diagnostics/logging | 缺（archived sessions 可低成本） |

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
