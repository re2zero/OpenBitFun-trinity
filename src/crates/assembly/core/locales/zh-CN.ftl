# OpenBitFun 简体中文语言包
# Chinese Simplified (zh-CN) Fluent Translation File

# ==================== 通用 ====================
app-version = 版本 { $version }
loading = 加载中...
welcome = 欢迎使用 OpenBitFun

# ==================== 操作 ====================
action-confirm = 确认
action-cancel = 取消
action-save = 保存
action-delete = 删除
action-edit = 编辑
action-create = 创建
action-add = 添加
action-remove = 移除
action-close = 关闭
action-open = 打开
action-copy = 复制
action-paste = 粘贴
action-undo = 撤销
action-redo = 重做
action-refresh = 刷新
action-search = 搜索
action-retry = 重试
action-stop = 停止
action-start = 开始

# ==================== 状态 ====================
status-loading = 加载中
status-saving = 保存中
status-saved = 已保存
status-success = 成功
status-error = 错误
status-warning = 警告
status-info = 信息
status-pending = 等待中
status-processing = 处理中
status-completed = 已完成
status-failed = 失败
status-cancelled = 已取消
status-ready = 就绪
status-connected = 已连接
status-disconnected = 已断开

# ==================== 文件 ====================
file-not-found = 文件未找到：{ $path }
file-read-error = 读取文件失败：{ $path }
file-write-error = 写入文件失败：{ $path }
file-delete-error = 删除文件失败：{ $path }
file-permission-denied = 权限不足：{ $path }
file-already-exists = 文件已存在：{ $path }
file-saved = 文件已保存：{ $path }
file-created = 文件已创建：{ $path }
file-deleted = 文件已删除：{ $path }

# ==================== 工作区 ====================
workspace-opened = 工作区已打开：{ $path }
workspace-closed = 工作区已关闭
workspace-not-found = 工作区未找到
workspace-open-error = 打开工作区失败

# ==================== Git ====================
git-not-repository = 当前目录不是 Git 仓库
git-commit-success = 提交成功
git-push-success = 推送成功
git-pull-success = 拉取成功
git-clone-error = 克隆仓库失败
git-commit-error = 提交失败
git-push-error = 推送失败
git-pull-error = 拉取失败
git-merge-conflict = 存在合并冲突
git-branch-created = 分支已创建：{ $name }
git-branch-deleted = 分支已删除：{ $name }
git-checkout-success = 已切换到分支：{ $name }

# ==================== AI ====================
ai-connection-error = 连接 AI 服务失败
ai-api-key-invalid = API 密钥无效
ai-model-not-found = 模型未找到：{ $model }
ai-context-too-long = 上下文超出限制
ai-rate-limited = 请求频率超出限制
ai-generation-error = 生成内容失败
ai-thinking = 思考中...
ai-generating = 生成中...

# ==================== 终端 ====================
terminal-created = 终端已创建
terminal-closed = 终端已关闭
terminal-create-error = 创建终端失败
terminal-command-error = 执行命令失败
terminal-shell-not-found = Shell 未找到

# ==================== 配置 ====================
config-loaded = 配置已加载
config-saved = 配置已保存
config-load-error = 加载配置失败
config-save-error = 保存配置失败
config-invalid = 配置格式无效
config-reset = 配置已重置

# ==================== 快照 ====================
snapshot-created = 快照已创建：{ $name }
snapshot-restored = 快照已恢复：{ $name }
snapshot-deleted = 快照已删除
snapshot-create-error = 创建快照失败
snapshot-restore-error = 恢复快照失败
snapshot-not-found = 快照未找到

# ==================== 国际化 ====================
language-changed = 语言已切换为：{ $language }
language-not-supported = 不支持的语言：{ $language }

# ==================== 通知 ====================
notification-copied = 已复制到剪贴板
notification-settings-saved = 设置已保存
notification-connection-established = 连接已建立
notification-connection-lost = 连接已断开

# ==================== 错误 ====================
error-unknown = 发生未知错误
error-network = 网络错误
error-timeout = 请求超时
error-server = 服务器错误
error-unauthorized = 未授权
error-forbidden = 禁止访问

# ==================== 时间 ====================
time-just-now = 刚刚
time-seconds-ago = { $count } 秒前
time-minutes-ago = { $count } 分钟前
time-hours-ago = { $count } 小时前
time-days-ago = { $count } 天前
time-weeks-ago = { $count } 周前
time-months-ago = { $count } 月前
time-years-ago = { $count } 年前

# ==================== GPUI Shell ====================
gpui-shell-subtitle = GPUI 原生前端
gpui-shell-theme = 主题
gpui-shell-language = 语言
gpui-shell-status-starting = 正在启动 OpenBitFun 运行时…
gpui-shell-status-ready = 运行时就绪
gpui-shell-status-failed = 运行时启动失败：{ $error }
gpui-shell-sessions-title = 会话
gpui-shell-sessions-empty = 当前工作区还没有会话。先在 CLI 或桌面端发起对话，然后重新加载。
gpui-shell-sessions-reload = 重新加载
gpui-shell-sessions-new = 新建会话
gpui-shell-sessions-show-archived = 显示已归档会话
gpui-shell-session-rename = 重命名
gpui-shell-session-rename-title = 重命名会话
gpui-shell-session-archive = 归档
gpui-shell-session-unarchive = 取消归档
gpui-shell-session-delete = 删除
gpui-shell-session-delete-title = 删除会话
gpui-shell-session-delete-warning = 将永久删除该会话及其历史记录，且无法恢复。
gpui-shell-session-delete-confirm = 删除
gpui-shell-session-action-confirm = 确定
gpui-shell-session-action-cancel = 取消
gpui-shell-session-fork = 分叉
gpui-shell-session-compact = 压缩上下文
gpui-shell-session-export = 导出会话记录
gpui-shell-fork-at-turn = 从此轮分叉
gpui-shell-fork-before-turn = 在此轮前分叉
gpui-shell-export-title = 导出会话记录
gpui-shell-export-done-prefix = 已导出到
gpui-shell-compaction-started = 正在压缩会话上下文…
gpui-shell-usage-title = 会话用量
gpui-shell-usage-turns = 轮次
gpui-shell-usage-input-tokens = 输入 tokens
gpui-shell-usage-output-tokens = 输出 tokens
gpui-shell-usage-cached-tokens = 缓存 tokens
gpui-shell-usage-total-tokens = 总 tokens
gpui-shell-usage-wall-time = 墙钟时间
gpui-shell-usage-active-time = 活跃轮次时间
gpui-shell-usage-models = 模型
gpui-shell-insights-title = 洞察
gpui-shell-insights-generate = 生成报告
gpui-shell-insights-generating = 正在生成报告…
gpui-shell-insights-loading = 加载中…
gpui-shell-insights-empty = 暂无洞察报告。生成一份即可查看会话趋势。
gpui-shell-insights-sessions = 会话数
gpui-shell-insights-analyzed = 已分析
gpui-shell-insights-messages = 消息数
gpui-shell-insights-days-covered = 覆盖天数
gpui-shell-insights-whats-working = 运转良好
gpui-shell-insights-whats-hindering = 存在阻碍
gpui-shell-insights-quick-wins = 快速改进
gpui-shell-insights-looking-ahead = 展望
gpui-shell-tasks-title = 定时任务
gpui-shell-tasks-empty = 暂无定时任务。可让代理创建，或在桌面端添加。
gpui-shell-tasks-enable = 启用
gpui-shell-tasks-disable = 停用
gpui-shell-tasks-run-now = 立即运行
gpui-shell-files-title = 文件
gpui-shell-files-loading = 正在加载工作区文件…
gpui-shell-files-empty = 工作区没有文件。
gpui-shell-files-reload = 重新加载文件树
gpui-shell-files-failed = 文件：{ $error }
gpui-shell-files-too-large = 文件过大，无法显示
gpui-shell-files-binary = 二进制文件，无法预览
gpui-shell-files-truncated = 仅显示该文件前 1 MB。
gpui-shell-search-title = 搜索
gpui-shell-search-files-tab = 文件
gpui-shell-search-sessions-tab = 会话
gpui-shell-search-run = 搜索
gpui-shell-search-running = 正在搜索…
gpui-shell-search-empty = 没有结果。
gpui-shell-search-failed = 搜索：{ $error }
gpui-shell-search-unavailable = 此处无法使用工作区搜索。
gpui-shell-search-truncated = 还有更多结果，请细化查询。
gpui-shell-terminal-new-tab = 新建终端标签
gpui-shell-terminal-new-tab-default = 默认 Shell
gpui-shell-terminal-profiles-manage = 管理 Profile…
gpui-shell-terminal-profiles = 终端 Profile
gpui-shell-terminal-profiles-desc = 以名称保存的终端：工作目录与可选启动命令，作为终端面板的额外标签打开。
gpui-shell-terminal-profiles-empty = 还没有 Profile，新建一个即可作为标签打开。
gpui-shell-terminal-profile-add = 新建 Profile
gpui-shell-terminal-profile-edit = 编辑
gpui-shell-terminal-profile-delete = 删除
gpui-shell-terminal-profile-title = 终端 Profile
gpui-shell-terminal-profile-name = 名称
gpui-shell-terminal-profile-dir = 工作目录
gpui-shell-terminal-profile-command = 启动命令
gpui-shell-model-picker = 会话模型
gpui-shell-model-picker-empty = 未配置模型
gpui-shell-model-default = 默认模型
gpui-shell-models-title = 模型
gpui-shell-models-default = 默认模型
gpui-shell-models-default-desc = 新会话使用的模型；已有会话保持各自的选择。
gpui-shell-models-configured = 已配置模型
gpui-shell-models-loading = 正在加载模型目录…
gpui-shell-models-empty = 尚未配置模型。请先在桌面端添加模型，再回到这里选择。
gpui-shell-models-enabled = 已启用
gpui-shell-models-disabled = 已停用
gpui-shell-chat-empty = 选择左侧会话，或新建会话开始对话。
gpui-shell-chat-input-placeholder = 输入消息，发送给 Agent…
gpui-shell-chat-send = 发送
gpui-shell-chat-stop = 停止
gpui-shell-chat-busy = 回复中…
gpui-shell-chat-submitting = 提交中…
gpui-shell-chat-jump-latest = 跳到最新
gpui-shell-tool-running = 执行中
gpui-shell-tool-waiting = 等待确认
gpui-shell-tool-done = 完成
gpui-shell-tool-failed = 执行失败
gpui-shell-tool-cancelled = 已中止
gpui-shell-permission-title = 需要权限
gpui-shell-permission-resources = 资源
gpui-shell-permission-project = 项目
gpui-shell-permission-risk = 风险
gpui-shell-permission-subagent = 子代理 { $subagent } 发起
gpui-shell-permission-allow-once = 仅允许一次
gpui-shell-permission-allow-always = 始终允许
gpui-shell-permission-deny = 拒绝

gpui-shell-session-turns = { $count } 轮对话
gpui-shell-terminal-title = 终端
gpui-shell-terminal-close = 关闭终端
gpui-shell-terminal-exited = 进程已退出
gpui-shell-settings-open = 打开设置
gpui-shell-settings-appearance = 外观
gpui-shell-settings-permissions = 权限
gpui-shell-settings-language-desc = 界面语言，与桌面端共享。
gpui-shell-settings-theme-desc = 配色方案，与桌面端外观设置共享。
gpui-shell-settings-theme-system = 跟随系统
gpui-shell-settings-theme-unsupported = 本壳暂不支持
gpui-shell-settings-policy = 工具权限策略
gpui-shell-settings-policy-desc = 「询问」在工具执行前请求确认；「完全访问」直接放行全部调用。
gpui-shell-settings-policy-ask = 询问
gpui-shell-settings-policy-full-access = 完全访问
gpui-shell-settings-auto-approve = 自动批准请求
gpui-shell-settings-auto-approve-desc = 自动应答交互式权限请求（仅对「询问」策略生效）。
gpui-shell-settings-terminal = 终端
gpui-shell-settings-terminal-font-size = 字号
gpui-shell-settings-terminal-font-size-desc = 内置终端网格的文字大小。
gpui-shell-settings-terminal-font-family = 字体
gpui-shell-settings-terminal-font-family-desc = 终端字体（可填字体栈）；留空使用内置等宽字体。
gpui-shell-settings-terminal-scrollback = 回滚行数
gpui-shell-settings-terminal-scrollback-desc = 实时屏幕之上保留的历史行数。
gpui-shell-settings-terminal-shell = Shell 命令
gpui-shell-settings-terminal-shell-desc = 留空则跟随 $SHELL；新开的终端生效。
gpui-shell-git-title = 变更
gpui-shell-git-failed = 加载变更失败：{ $error }
gpui-shell-chat-attachment-unsupported = 不支持的图片类型：{ $name }
gpui-shell-notification-turn-done = 轮次已完成
gpui-shell-notification-turn-failed = 轮次失败

gpui-shell-git-refresh = 刷新
gpui-shell-git-loading = 正在加载变更…
gpui-shell-git-empty = 工作区没有变更。
gpui-shell-git-binary = 二进制文件，不显示差异
gpui-shell-git-too-large = 差异过大，无法显示
gpui-shell-git-truncated = 部分文件未列出（快照已截断）。
gpui-shell-git-staged = 已暂存
gpui-shell-git-unstaged = 未暂存
gpui-shell-git-untracked = 未跟踪
gpui-shell-git-select = 选择文件以查看差异。
gpui-shell-git-diff-wrap = 切换 diff 换行


gpui-shell-question-title = 需要你的回答
gpui-shell-question-progress = 问题 { $index } / { $total }
gpui-shell-question-multi = 可多选
gpui-shell-question-submit = 提交
gpui-shell-question-cancel = 取消
gpui-shell-question-unreadable = 无法显示该问题的内容。

gpui-shell-workspace-switch = 切换工作区
gpui-shell-workspace-open-path = 按路径打开…
gpui-shell-workspace-open-path-description = 作为本壳工作区使用的文件夹。它只会登记进共享工作区列表，不改桌面端的选择。
