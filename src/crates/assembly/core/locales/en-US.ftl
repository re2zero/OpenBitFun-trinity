# OpenBitFun English Language Pack
# English (US) (en-US) Fluent Translation File

# ==================== General ====================
app-version = Version { $version }
loading = Loading...
welcome = Welcome to OpenBitFun

# ==================== Actions ====================
action-confirm = Confirm
action-cancel = Cancel
action-save = Save
action-delete = Delete
action-edit = Edit
action-create = Create
action-add = Add
action-remove = Remove
action-close = Close
action-open = Open
action-copy = Copy
action-paste = Paste
action-undo = Undo
action-redo = Redo
action-refresh = Refresh
action-search = Search
action-retry = Retry
action-stop = Stop
action-start = Start

# ==================== Status ====================
status-loading = Loading
status-saving = Saving
status-saved = Saved
status-success = Success
status-error = Error
status-warning = Warning
status-info = Info
status-pending = Pending
status-processing = Processing
status-completed = Completed
status-failed = Failed
status-cancelled = Cancelled
status-ready = Ready
status-connected = Connected
status-disconnected = Disconnected

# ==================== File ====================
file-not-found = File not found: { $path }
file-read-error = Failed to read file: { $path }
file-write-error = Failed to write file: { $path }
file-delete-error = Failed to delete file: { $path }
file-permission-denied = Permission denied: { $path }
file-already-exists = File already exists: { $path }
file-saved = File saved: { $path }
file-created = File created: { $path }
file-deleted = File deleted: { $path }

# ==================== Workspace ====================
workspace-opened = Workspace opened: { $path }
workspace-closed = Workspace closed
workspace-not-found = Workspace not found
workspace-open-error = Failed to open workspace

# ==================== Git ====================
git-not-repository = Current directory is not a Git repository
git-commit-success = Commit successful
git-push-success = Push successful
git-pull-success = Pull successful
git-clone-error = Failed to clone repository
git-commit-error = Commit failed
git-push-error = Push failed
git-pull-error = Pull failed
git-merge-conflict = Merge conflict exists
git-branch-created = Branch created: { $name }
git-branch-deleted = Branch deleted: { $name }
git-checkout-success = Switched to branch: { $name }

# ==================== AI ====================
ai-connection-error = Failed to connect to AI service
ai-api-key-invalid = Invalid API key
ai-model-not-found = Model not found: { $model }
ai-context-too-long = Context exceeds limit
ai-rate-limited = Rate limit exceeded
ai-generation-error = Failed to generate content
ai-thinking = Thinking...
ai-generating = Generating...

# ==================== Terminal ====================
terminal-created = Terminal created
terminal-closed = Terminal closed
terminal-create-error = Failed to create terminal
terminal-command-error = Failed to execute command
terminal-shell-not-found = Shell not found

# ==================== Config ====================
config-loaded = Configuration loaded
config-saved = Configuration saved
config-load-error = Failed to load configuration
config-save-error = Failed to save configuration
config-invalid = Invalid configuration format
config-reset = Configuration reset

# ==================== Snapshot ====================
snapshot-created = Snapshot created: { $name }
snapshot-restored = Snapshot restored: { $name }
snapshot-deleted = Snapshot deleted
snapshot-create-error = Failed to create snapshot
snapshot-restore-error = Failed to restore snapshot
snapshot-not-found = Snapshot not found

# ==================== I18n ====================
language-changed = Language changed to: { $language }
language-not-supported = Unsupported language: { $language }

# ==================== Notifications ====================
notification-copied = Copied to clipboard
notification-settings-saved = Settings saved
notification-connection-established = Connection established
notification-connection-lost = Connection lost

# ==================== Errors ====================
error-unknown = An unknown error occurred
error-network = Network error
error-timeout = Request timeout
error-server = Server error
error-unauthorized = Unauthorized
error-forbidden = Access forbidden

# ==================== Time ====================
time-just-now = just now
time-seconds-ago = { $count } { $count ->
    [one] second
   *[other] seconds
} ago
time-minutes-ago = { $count } { $count ->
    [one] minute
   *[other] minutes
} ago
time-hours-ago = { $count } { $count ->
    [one] hour
   *[other] hours
} ago
time-days-ago = { $count } { $count ->
    [one] day
   *[other] days
} ago
time-weeks-ago = { $count } { $count ->
    [one] week
   *[other] weeks
} ago
time-months-ago = { $count } { $count ->
    [one] month
   *[other] months
} ago
time-years-ago = { $count } { $count ->
    [one] year
   *[other] years
} ago

# ==================== GPUI Shell ====================
gpui-shell-subtitle = Native GPUI frontend
gpui-shell-theme = Theme
gpui-shell-language = Language
gpui-shell-status-starting = Starting OpenBitFun runtime…
gpui-shell-status-ready = Runtime ready
gpui-shell-status-failed = Runtime failed: { $error }
gpui-shell-sessions-title = Sessions
gpui-shell-sessions-empty = No sessions in this workspace yet. Start one in the CLI or desktop app, then reload.
gpui-shell-sessions-reload = Reload
gpui-shell-sessions-new = New session
gpui-shell-sessions-show-archived = Show archived sessions
gpui-shell-session-rename = Rename
gpui-shell-session-rename-title = Rename session
gpui-shell-session-archive = Archive
gpui-shell-session-unarchive = Unarchive
gpui-shell-session-delete = Delete
gpui-shell-session-delete-title = Delete session
gpui-shell-session-delete-warning = This permanently deletes the session and its history. This cannot be undone.
gpui-shell-session-delete-confirm = Delete
gpui-shell-session-action-confirm = Confirm
gpui-shell-session-action-cancel = Cancel
gpui-shell-session-fork = Fork
gpui-shell-session-compact = Compact context
gpui-shell-session-export = Export transcript
gpui-shell-fork-at-turn = Fork from this turn
gpui-shell-fork-before-turn = Fork before this turn
gpui-shell-export-title = Export transcript
gpui-shell-export-done-prefix = Exported to
gpui-shell-compaction-started = Compacting session context…
gpui-shell-usage-title = Session usage
gpui-shell-usage-turns = Turns
gpui-shell-usage-input-tokens = Input tokens
gpui-shell-usage-output-tokens = Output tokens
gpui-shell-usage-cached-tokens = Cached tokens
gpui-shell-usage-total-tokens = Total tokens
gpui-shell-usage-wall-time = Wall time
gpui-shell-usage-active-time = Active turn time
gpui-shell-usage-models = Model
gpui-shell-insights-title = Insights
gpui-shell-insights-generate = Generate report
gpui-shell-insights-generating = Generating report…
gpui-shell-insights-loading = Loading…
gpui-shell-insights-empty = No insight reports yet. Generate one to see session trends.
gpui-shell-insights-sessions = Sessions
gpui-shell-insights-analyzed = Analyzed
gpui-shell-insights-messages = Messages
gpui-shell-insights-days-covered = Days covered
gpui-shell-insights-whats-working = What's working
gpui-shell-insights-whats-hindering = What's hindering
gpui-shell-insights-quick-wins = Quick wins
gpui-shell-insights-looking-ahead = Looking ahead
gpui-shell-tasks-title = Scheduled jobs
gpui-shell-tasks-empty = No scheduled jobs. Ask the agent to create one, or add it in the desktop app.
gpui-shell-tasks-enable = Enable
gpui-shell-tasks-disable = Disable
gpui-shell-tasks-run-now = Run now
gpui-shell-files-title = Files
gpui-shell-files-loading = Loading workspace files…
gpui-shell-files-empty = No files in this workspace.
gpui-shell-files-reload = Reload file tree
gpui-shell-files-failed = Files: { $error }
gpui-shell-files-too-large = File is too large to display
gpui-shell-files-binary = Binary file, no preview
gpui-shell-files-truncated = Showing the first 1 MB of this file.
gpui-shell-search-title = Search
gpui-shell-search-files-tab = Files
gpui-shell-search-sessions-tab = Sessions
gpui-shell-search-run = Search
gpui-shell-search-running = Searching…
gpui-shell-search-empty = No results.
gpui-shell-search-failed = Search: { $error }
gpui-shell-search-unavailable = Workspace search is unavailable here.
gpui-shell-search-truncated = More results available; refine the query.
gpui-shell-terminal-new-tab = New terminal tab
gpui-shell-terminal-new-tab-default = Default shell
gpui-shell-terminal-profiles-manage = Manage profiles…
gpui-shell-terminal-profiles = Terminal profiles
gpui-shell-terminal-profiles-desc = Named shells the terminal panel offers as extra tabs: working directory and optional startup command.
gpui-shell-terminal-profiles-empty = No profiles yet. Add one to open it as a new tab.
gpui-shell-terminal-profile-add = Add profile
gpui-shell-terminal-profile-edit = Edit
gpui-shell-terminal-profile-delete = Delete
gpui-shell-terminal-profile-title = Terminal profile
gpui-shell-terminal-profile-name = Name
gpui-shell-terminal-profile-dir = Working directory
gpui-shell-terminal-profile-command = Startup command
gpui-shell-model-picker = Session model
gpui-shell-model-picker-empty = No configured models
gpui-shell-model-default = Default model
gpui-shell-models-title = Models
gpui-shell-models-default = Default model
gpui-shell-models-default-desc = Model new sessions start on; existing sessions keep their own selection.
gpui-shell-models-configured = Configured models
gpui-shell-models-loading = Loading model catalog…
gpui-shell-models-empty = No models configured. Add models in the desktop app to pick one here.
gpui-shell-models-enabled = enabled
gpui-shell-models-disabled = disabled
gpui-shell-chat-empty = Pick a session on the left, or create a new one.
gpui-shell-chat-input-placeholder = Type a message to send to the agent…
gpui-shell-chat-send = Send
gpui-shell-chat-stop = Stop
gpui-shell-chat-busy = Responding…
gpui-shell-chat-submitting = Submitting…
gpui-shell-chat-jump-latest = Jump to latest
gpui-shell-tool-running = Executing
gpui-shell-tool-waiting = Awaiting approval
gpui-shell-tool-done = Finished
gpui-shell-tool-failed = Errored
gpui-shell-tool-cancelled = Aborted
gpui-shell-permission-title = Permission required
gpui-shell-permission-resources = Resources
gpui-shell-permission-project = Project
gpui-shell-permission-risk = Risk
gpui-shell-permission-subagent = Requested by subagent { $subagent }
gpui-shell-permission-allow-once = Allow once
gpui-shell-permission-allow-always = Always allow
gpui-shell-permission-deny = Deny

gpui-shell-session-turns = { $count ->
  [one] 1 turn
  *[other] { $count } turns
}
gpui-shell-terminal-title = Terminal
gpui-shell-terminal-close = Close terminal
gpui-shell-terminal-exited = process exited
gpui-shell-settings-open = Open settings
gpui-shell-settings-appearance = Appearance
gpui-shell-settings-permissions = Permissions
gpui-shell-settings-language-desc = Interface language, shared with the desktop app.
gpui-shell-settings-theme-desc = Color palette, shared with the desktop appearance setting.
gpui-shell-settings-theme-system = Follow system
gpui-shell-settings-theme-unsupported = unsupported here
gpui-shell-settings-policy = Permission policy
gpui-shell-settings-policy-desc = Ask prompts before tools run; full access allows every call.
gpui-shell-settings-policy-ask = Ask
gpui-shell-settings-policy-full-access = Full access
gpui-shell-settings-auto-approve = Auto-approve requests
gpui-shell-settings-auto-approve-desc = Answer interactive approval requests automatically (Ask policy only).
gpui-shell-settings-terminal = Terminal
gpui-shell-settings-terminal-font-size = Font size
gpui-shell-settings-terminal-font-size-desc = Text size of the built-in terminal grid.
gpui-shell-settings-terminal-font-family = Font family
gpui-shell-settings-terminal-font-family-desc = Font stack for the terminal; empty follows the shell default.
gpui-shell-settings-terminal-scrollback = Scrollback lines
gpui-shell-settings-terminal-scrollback-desc = History kept above the live screen.
gpui-shell-settings-terminal-shell = Shell command
gpui-shell-settings-terminal-shell-desc = Empty follows $SHELL. A new terminal picks this up.
gpui-shell-git-title = Changes
gpui-shell-git-failed = Failed to load changes: { $error }
gpui-shell-chat-attachment-unsupported = Unsupported image type: { $name }
gpui-shell-notification-turn-done = Turn finished
gpui-shell-notification-turn-failed = Turn failed

gpui-shell-git-refresh = Refresh
gpui-shell-git-loading = Loading changes…
gpui-shell-git-empty = No workspace changes.
gpui-shell-git-binary = Binary file, diff not shown
gpui-shell-git-too-large = Diff too large to display
gpui-shell-git-truncated = Some files are not listed (snapshot truncated).
gpui-shell-git-staged = staged
gpui-shell-git-unstaged = unstaged
gpui-shell-git-untracked = untracked
gpui-shell-git-select = Select a file to see its diff.
gpui-shell-git-diff-wrap = Toggle diff line wrapping
gpui-shell-git-stage-all = Stage all changes
gpui-shell-git-stage = Stage file
gpui-shell-git-unstage = Unstage file
gpui-shell-git-commit = Commit
gpui-shell-git-commit-placeholder = Commit message
gpui-shell-git-push = Push to remote
gpui-shell-git-history = History
gpui-shell-git-files-view = Changes
gpui-shell-git-no-commits = No commits yet.
gpui-shell-git-untrusted-title = Repository ownership is not trusted
gpui-shell-git-untrusted-action = Trust repository


gpui-shell-question-title = Your answer is needed
gpui-shell-question-progress = Question { $index } / { $total }
gpui-shell-question-multi = Select all that apply
gpui-shell-question-submit = Submit
gpui-shell-question-cancel = Cancel
gpui-shell-question-unreadable = This question could not be displayed.

gpui-shell-workspace-switch = Switch workspace
gpui-shell-workspace-open-path = Open workspace by path…
gpui-shell-workspace-open-path-description = Folder to use as this shell's workspace. It is registered in the shared workspace list without changing the desktop's selection.
