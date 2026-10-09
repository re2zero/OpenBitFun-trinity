# OpenBitFun 繁體中文語言包
# Chinese Traditional (zh-TW) Fluent Translation File

# ==================== 通用 ====================
app-version = 版本 { $version }
loading = 載入中...
welcome = 歡迎使用 OpenBitFun

# ==================== 操作 ====================
action-confirm = 確認
action-cancel = 取消
action-save = 儲存
action-delete = 刪除
action-edit = 編輯
action-create = 建立
action-add = 新增
action-remove = 移除
action-close = 關閉
action-open = 開啟
action-copy = 複製
action-paste = 粘貼
action-undo = 撤銷
action-redo = 重做
action-refresh = 重新整理
action-search = 進行搜尋
action-retry = 重試
action-stop = 停止
action-start = 開始

# ==================== 狀態 ====================
status-loading = 正在載入
status-saving = 儲存中
status-saved = 已儲存
status-success = 成功
status-error = 錯誤
status-warning = 警告
status-info = 資訊
status-pending = 等待中
status-processing = 處理中
status-completed = 已完成
status-failed = 失敗
status-cancelled = 已取消
status-ready = 就緒
status-connected = 已連接
status-disconnected = 已斷開

# ==================== 檔案 ====================
file-not-found = 檔案未找到：{ $path }
file-read-error = 讀取檔案失敗：{ $path }
file-write-error = 寫入檔案失敗：{ $path }
file-delete-error = 刪除檔案失敗：{ $path }
file-permission-denied = 權限不足：{ $path }
file-already-exists = 檔案已存在：{ $path }
file-saved = 檔案已儲存：{ $path }
file-created = 檔案已建立：{ $path }
file-deleted = 檔案已刪除：{ $path }

# ==================== 工作區 ====================
workspace-opened = 工作區已開啟：{ $path }
workspace-closed = 工作區已關閉
workspace-not-found = 工作區未找到
workspace-open-error = 開啟工作區失敗

# ==================== Git ====================
git-not-repository = 目前目錄不是 Git 倉庫
git-commit-success = 提交成功
git-push-success = 推送成功
git-pull-success = 拉取成功
git-clone-error = 克隆倉庫失敗
git-commit-error = 提交失敗
git-push-error = 推送失敗
git-pull-error = 拉取失敗
git-merge-conflict = 存在合併衝突
git-branch-created = 分支已建立：{ $name }
git-branch-deleted = 分支已刪除：{ $name }
git-checkout-success = 已切換到分支：{ $name }

# ==================== AI ====================
ai-connection-error = 連接 AI 服務失敗
ai-api-key-invalid = API 密鑰無效
ai-model-not-found = 模型未找到：{ $model }
ai-context-too-long = 上下文超出限制
ai-rate-limited = 請求頻率超出限制
ai-generation-error = 生成內容失敗
ai-thinking = 思考中...
ai-generating = 生成中...

# ==================== 終端 ====================
terminal-created = 終端已建立
terminal-closed = 終端已關閉
terminal-create-error = 建立終端失敗
terminal-command-error = 執行命令失敗
terminal-shell-not-found = Shell 未找到

# ==================== 設定 ====================
config-loaded = 設定已載入
config-saved = 設定已儲存
config-load-error = 載入設定失敗
config-save-error = 儲存設定失敗
config-invalid = 設定格式無效
config-reset = 設定已重設

# ==================== 快照 ====================
snapshot-created = 快照已建立：{ $name }
snapshot-restored = 快照已恢復：{ $name }
snapshot-deleted = 快照已刪除
snapshot-create-error = 建立快照失敗
snapshot-restore-error = 恢復快照失敗
snapshot-not-found = 快照未找到

# ==================== 國際化 ====================
language-changed = 語言已切換為：{ $language }
language-not-supported = 不支持的語言：{ $language }

# ==================== 通知 ====================
notification-copied = 已複製到剪貼板
notification-settings-saved = 設置已儲存
notification-connection-established = 連接已建立
notification-connection-lost = 連接已斷開

# ==================== 錯誤 ====================
error-unknown = 發生未知錯誤
error-network = 網絡錯誤
error-timeout = 請求超時
error-server = 伺服器錯誤
error-unauthorized = 未授權
error-forbidden = 禁止訪問

# ==================== 時間 ====================
time-just-now = 剛剛
time-seconds-ago = { $count } 秒前
time-minutes-ago = { $count } 分鐘前
time-hours-ago = { $count } 小時前
time-days-ago = { $count } 天前
time-weeks-ago = { $count } 周前
time-months-ago = { $count } 月前
time-years-ago = { $count } 年前

# ==================== GPUI Shell ====================
gpui-shell-subtitle = GPUI 原生前端
gpui-shell-theme = 主題
gpui-shell-language = 語言
gpui-shell-status-starting = 正在啟動 OpenBitFun 執行時…
gpui-shell-status-ready = 執行時就緒
gpui-shell-status-failed = 執行時啟動失敗：{ $error }
gpui-shell-sessions-title = 會話
gpui-shell-sessions-empty = 目前工作區還沒有會話。先在 CLI 或桌面端開啟對話，然後重新載入。
gpui-shell-sessions-reload = 重新載入
gpui-shell-sessions-new = 新建會話
gpui-shell-chat-empty = 從左側選擇會話，或新建會話開始對話。
gpui-shell-chat-input-placeholder = 輸入訊息，傳送給 Agent…
gpui-shell-chat-send = 傳送
gpui-shell-chat-stop = 停止
gpui-shell-chat-busy = 回覆中…
gpui-shell-chat-submitting = 提交中…
gpui-shell-chat-jump-latest = 跳到最新
gpui-shell-tool-running = 進行中
gpui-shell-tool-waiting = 等待確認
gpui-shell-tool-done = 完成
gpui-shell-tool-failed = 執行失敗
gpui-shell-tool-cancelled = 已中止
gpui-shell-permission-title = 需要許可權
gpui-shell-permission-resources = 資源
gpui-shell-permission-project = 專案
gpui-shell-permission-risk = 風險
gpui-shell-permission-subagent = 子代理 { $subagent } 發起
gpui-shell-permission-allow-once = 僅允許一次
gpui-shell-permission-allow-always = 永遠允許
gpui-shell-permission-deny = 拒絕

gpui-shell-session-turns = { $count } 輪對話
gpui-shell-terminal-title = 終端
gpui-shell-terminal-close = 關閉終端
gpui-shell-terminal-exited = 處理程序已結束
gpui-shell-settings-open = 開啟設定
gpui-shell-settings-appearance = 外觀
gpui-shell-settings-permissions = 權限
gpui-shell-settings-language-desc = 介面語言，與桌面端共用。
gpui-shell-settings-theme-desc = 配色方案，與桌面端外觀設定共用。
gpui-shell-settings-theme-system = 跟隨系統
gpui-shell-settings-theme-unsupported = 本殼暫不支援
gpui-shell-settings-policy = 工具權限策略
gpui-shell-settings-policy-desc = 「詢問」會在工具執行前要求確認；「完全存取」直接放行所有呼叫。
gpui-shell-settings-policy-ask = 詢問
gpui-shell-settings-policy-full-access = 完全存取
gpui-shell-settings-auto-approve = 自動批准請求
gpui-shell-settings-auto-approve-desc = 自動回應互動式權限請求（僅對「詢問」策略生效）。
gpui-shell-settings-terminal = 終端機
gpui-shell-settings-terminal-font-size = 字級
gpui-shell-settings-terminal-font-size-desc = 內建終端機網格的文字大小。
gpui-shell-settings-terminal-font-family = 字型
gpui-shell-settings-terminal-font-family-desc = 終端機字型（可填字型堆疊）；留空使用內建等寬字型。
gpui-shell-settings-terminal-scrollback = 回捲行數
gpui-shell-settings-terminal-scrollback-desc = 即時畫面之上保留的歷史行數。
gpui-shell-settings-terminal-shell = Shell 指令
gpui-shell-settings-terminal-shell-desc = 留空則跟隨 $SHELL；新開的終端機生效。
gpui-shell-git-title = 變更
gpui-shell-git-failed = 載入變更失敗：{ $error }
gpui-shell-git-refresh = 重新整理
gpui-shell-git-loading = 正在載入變更…
gpui-shell-git-empty = 工作區沒有變更。
gpui-shell-git-binary = 二進位檔案，不顯示差異
gpui-shell-git-too-large = 差異過大，無法顯示
gpui-shell-git-truncated = 部分檔案未列出（快照已截斷）。
gpui-shell-git-staged = 已暫存
gpui-shell-git-unstaged = 未暫存
gpui-shell-git-untracked = 未追蹤
gpui-shell-git-select = 選擇檔案以查看差異。
gpui-shell-git-diff-wrap = 切換 diff 換行

