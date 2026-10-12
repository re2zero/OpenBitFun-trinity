# 插件 UI 贡献架构(R7)设计

状态:**设计稿(R7a,2026-10-12)**,待评审;实现按 R7b(壳内最小闭环)→ R7c(真 Plugin Host 通道)推进,尚未有生产消费方。本文不改变
[产品架构](../product-architecture.md)的任何边界——它把该文第 3 节「OpenBitFun 与插件接口」中已预留的
**界面贡献**能力落成具体契约。

| 关联文档 | 关系 |
|---|---|
| [product-architecture.md](../product-architecture.md) §3 / §3.3 / §4.4 / §7 | 上位边界:接口边界表、入口形态接口规则(GUI 行)、插件调用、完成判定 |
| [plugin-runtime-design.md](plugin-runtime-design.md) | 插件运行时现状(owner、生命周期、OpenCode 基线) |
| [opencode-extension-compatibility.md](opencode-extension-compatibility.md) §6 | 明确限制与延期决策(TUI 原始组件树降级等) |
| `docs/plans/gpui-shell-parity-and-switch-plan.md` R7 档 | 立项指令与验收口径(壳侧) |

## 1. 目标与非目标

**目标**:让插件以**声明式 UI 贡献**进入 OpenBitFun 壳(首先 GPUI 桌面壳),覆盖场景/面板/卡片/动作四类
贡献位;插件不写界面代码、不注入宿主组件、不加载 HTML;壳用**原生渲染器**把声明树渲染为 gpui-kit 组件。
对应公子立项指令:「插件只能做功能,UI 需要深度融合(硬编码进壳/前端),无法基于插件实现」。

**非目标(红线,推翻需重新立项)**:

- **无 webview、无 HTML/CSS 渲染**。GPUI 壳没有也不引入 webview;MiniApps 的 iframe + `compiled_html`
  路线不适用于壳,且 MiniApps 本体维持 Phase 1/2「后置」拍板,不借 R7 复活。
- **不翻译外部生态的界面代码**。OpenCode 插件的 Solid/OpenTUI 组件树在 GUI 侧与 TUI 侧同样「原始组件必须
  明确降级」;OpenCode 兼容协议里本来就没有 GUI 界面贡献。本文契约是 **OpenBitFun 自有扩展契约**,只服务
  于携带 `ui/` 贡献段的 OpenBitFun 插件包;OpenCode 来源插件不带该段即无 UI 贡献,不报错、不降级提示。
- **不新增越权面**。UI 贡献是纯数据;插件获取数据与能力仍只能走既有 `backend.*` 反向方法、工具与权限
  路径。渲染器没有脚本执行、网络加载、文件系统或主题覆盖语义。
- **不改变壳的 i18n/theme 治理**:插件文案不进壳 ftl 目录,插件配色不存在(只有语义角色引用)。

## 2. 架构边界与 owner 分层

按 [product-architecture.md](../product-architecture.md) §3.3「Desktop GUI / Web」行的三栏表执行:

| 该表栏目 | 在本设计中的落地 |
|---|---|
| 可进入稳定接口的内容 | 场景(路由)、面板、卡片、动作四类贡献声明;GUI 主题**语义角色**(`accent`/`danger`/`surface`/`text`/`border`/`muted`);只读状态视图 |
| 必须由宿主决定 | 装载位置(nav/tab 归属最终裁决)、布局约束、焦点与可访问性、语义角色 → 设计 token 映射 |
| 禁止进入插件接口 | 宿主组件实例、可执行界面代码、任意色值/CSS、终端键位、宿主内部状态 |

**Owner 分层**(依赖自上而下,与仓库分层表一致):

| 层 | owner | 职责 |
|---|---|---|
| 契约 | `src/crates/contracts/product-domains/src/plugin_ui.rs`(新增) | `PluginUiContribution` / `PluginUiTree` / `PluginUiEvent` / `PluginUiSlot` 纯数据类型 + schema 校验(白名单、限额、`deny_unknown_fields`)。行为轻,不依赖上层 |
| 投影 owner | `src/crates/assembly/core/src/plugin_ui.rs`(新增,feature `plugin-runtime`) | 把 provider 提供的贡献发布为 generation 化快照;持有每场景当前树缓存;仿 `plugin_capabilities.rs` 的 `PluginCapabilityProjection` 发布模式。唯一边界校验与诊断归并点 |
| 通道 provider | R7b:core 内 fixture(`PluginUiProvider` trait,Rust 实现);R7c:经 `HookFunctionRuntime` 扩展的 host 方法族 | 产出贡献声明与 UI 树;响应事件返回新树 |
| 渲染器 | `src/apps/gpui-shell/src/plugin_ui.rs`(新增) + `tabs.rs` 接缝 | schema 树 → gpui-kit 组件;语义角色 → `cx.theme()`;收集交互事件回投 core。壳**不依赖** `plugin-runtime-client`、Plugin Host 进程或 OpenCode 原始类型(§7 完成判定) |

界面贡献不改变既有能力 owner:工具、Hook、权限、审计仍走原链路;UI 贡献只是把「已批准插件存在哪些
界面、当前长什么样」这一事实按快照发布给壳。

## 3. 设计一:manifest 的 UI 贡献段

### 3.1 包布局与版本

`openbitfun.plugin.json` 本体**保持不变**(`schemaVersion=1`、`deny_unknown_fields`、旧读取方零影响,
满足升级兼容铁律)。UI 贡献是包内**独立文件**:

```
<plugin-package>/
  openbitfun.plugin.json      # 现有清单,files[] 中登记 ui/* 文件的 sha256
  ui/
    contributions.json        # 贡献声明(自带 schemaVersion,见 3.2)
    scenes/<scene_id>.json    # 每场景初始 UI 树(按需引用)
```

- `ui/contributions.json` 自带 `schemaVersion`(线上字段名,与 manifest 的 camelCase 惯例一致;起步
  `1`)与 `deny_unknown_fields`(Rust 侧字段 `schema_version`);未知贡献类型/未知组件在**加载期**整体
  拒绝该插件 UI 段并发布类型化诊断(不静默忽略、不半渲染)。
- 升级策略与 manifest 同口径:新字段加默认值可读旧包;`schemaVersion` 只增不减;壳对更高版本的
  `ui` 段显示「需要更新版本的 OpenBitFun」显式态,不猜测渲染。

### 3.2 贡献类型

```jsonc
// ui/contributions.json(v0 示例)
{
  "schemaVersion": 1,
  "scenes": [{
    "id": "board",                      // 包内唯一;全局 id = "<plugin_id>.<scene_id>"
    "title": { "zh-CN": "看板", "en-US": "Board" },   // 声明式本地化,见 3.3
    "icon": "layout-dashboard",         // 封闭图标集,见 5.3
    "slot": { "kind": "tab" },          // 归属意图,宿主裁决,见下
    "entry": "ui/scenes/board.json"     // 初始树文件引用(受 files sha256 锁定)
  }],
  "cards": [],                          // v1:工作台卡片位(位置由宿主决定)
  "panels": [],                         // v1:资源面板位(ResourcePanelKind 扩展)
  "actions": []                         // v1:全局搜索快捷动作(productActionCatalog 位)
}
```

- **slot 声明的是意图,不是结果**:壳按 §3.3 裁决最终装载位置(v0 仅 `tab`,即场景 tab,singleton 语义
  同内置场景 tab;`nav` 候选 v1 再议——侧栏导航五项是桌面同构拍板,不自动扩)。
- 四类贡献中 **R7b 只实现 `scenes`**;`cards`/`panels`/`actions` 是 v0 schema 的**已知字段**,但加载器
  本版本不消费:遇非空表,发布「该贡献类型此版本未支持」类型化诊断并忽略**该表**(scenes 不受牵连,
  照常注册)——不用「整体拒绝」,避免插件因一个未接入位丢失全部场景;接入各自宿主位是 v1 事项,逐位
  评审。

### 3.3 本地化与主题语义角色

- **文案**:插件自带 locale map(键为 BCP-47,起步要求 `en-US` 存在;壳按当前语言选择,取词顺序 =
  精确匹配 → 同语言族回退(`zh-TW` → `zh-CN` 等主子标签匹配)→ `en-US` → map 首项)。不进壳 ftl 体系、
  不进 `i18n:audit` 预算。预算限额见 3.4。
- **配色**:不存在。组件 props 只允许语义角色引用(`"tone": "danger"`);角色 → `cx.theme()` token 的
  映射在渲染器(§5.2),插件无法引入新色值,theme audit 基线零增长。

### 3.4 校验规则(加载期,全部通过才进入注册表)

| 项 | 限制(v0) | 超限行为 |
|---|---|---|
| 场景数 | ≤ 8 / 插件 | 拒绝 UI 段 + 诊断 |
| 贡献 id | `^[a-z][a-z0-9-]{0,63}$`,包内唯一 | 拒绝 |
| 文案键 | 每插件 ≤ 128 键,单值 ≤ 256 字符,≤ 8 locale | 拒绝 |
| 图标 | 必须在封闭图标集内(§5.3) | 拒绝 |
| `entry` 引用 | 必须命中 `files[]` 清单(sha256 校验同包体) | 拒绝 |

## 4. 设计二:UI 树与数据通道

### 4.1 树节点 schema

```jsonc
// ui/scenes/board.json(初始树)与事件响应返回的树同构
{
  "schemaVersion": 1,
  "root": {
    "type": "column",                   // 组件名,封闭枚举(§5.1)
    "props": { "gap": 2 },              // 白名单 props,按组件校验
    "children": [
      { "type": "heading", "props": { "text": "@title", "level": 2 } },
      { "type": "button", "props": { "label": "@btn.refresh", "tone": "primary" },
        "on": { "press": "refresh" } }  // 事件 → 动作名(包内唯一字符串)
    ]
  }
}
```

- 文案引用用 `"@key"` 前缀指向插件 locale map;字面字符串也允许(但受长度限额),方便最小插件。
- 无条件渲染、无表达式、无循环、无模板——**插件端负责算好最终树**。壳是哑渲染器,这是安全模型
  (§4.5)与实现复杂度的共同下限。

### 4.2 状态模型:清单快照 + 按场景树,全量替换

- core 投影 owner 持有 `PluginUiSnapshot { generation, contributions }`(贡献清单,activation/
  信任变化时整代替换,复用现有 generation fencing 语义)与 `BTreeMap<SceneRef, PluginUiTree>`(每场景
  当前树,打开时经 provider 拉取,事件后整树替换)。
- **全量替换、无差量**:与壳内 `ResizablePanelGroup::children` 单次全量、`TabsState::remove` 程序化
  迁移的既有语义一致;256KiB 树上界内全量替换成本可忽略,差量协议不做(v0 决策,避免引入树 diff
  一致性问题)。
- 树限额:节点 ≤ 512、深度 ≤ 16、单帧 ≤ 256KiB、单节点文本 ≤ 4KiB;树文件的 `schemaVersion` 必须与
  `ui/contributions.json` 同版本族(v0 皆 `1`),不一致 = 拒绝该树。超限/不一致 = 拒绝 + 类型化诊断 +
  **保留上一棵树**(保旧不崩壳,同升级兼容的「degrade loudly」口径)。
- **诊断必须可见**:拒绝、超时、贡献类型未支持等类型化诊断落在该场景 tab 内的显式错误态(横幅/空态,
  渲染器自带),不静默吞。诊断文案是**壳文案**(非插件文案),进壳 ftl 三语、按惯例数键对账——R7b 若
  出新键,§6.4 的「无新键」预期随之修正。
- **贡献被撤的打开中 tab**:generation 失配(插件停用/撤销/版本变更撤下该场景)时,已打开的场景 tab
  切「贡献已移除」显式态、事件回环禁用;tab 保留由用户手动关闭(不自动关用户打开的 tab,同「永不删除
  用户数据」精神)。R7b fixture 不触发此臂,R7c 实例关闭时必测。

### 4.3 事件回环

```
壳渲染器                    core 投影 owner                 provider(R7b fixture / R7c host)
   │ 交互(按下/提交/变更)      │                                    │
   ├─ PluginUiEvent ────────→ │ 校验事件归属(场景 ref 属该插件)      │
   │                          ├─ dispatch(action, payload) ───────→ │
   │                          │ ←──────────── 新树 / Unchanged ─────┤
   │ ←─ 整树替换(新 generation)│ 校验限额 → 更新缓存                  │
```

- 事件只携带 `{ scene_ref, action, payload }`。`scene_ref` = (插件来源标识, 包内场景 id) 二元组
  (Rust 侧 `#[derive(Copy)]`,壳 `SceneId::External(SceneRef)` 全链 Copy 传递);`payload` v0 仅控件值
  (字符串/数值/布尔),字符串 ≤ 4KiB(与树文本同额),无文件、无任意 JSON 树(防 payload 夹带)。
- 渲染器侧:**单飞合并**——同一场景同时至多一个在途回环,回环期间新事件入队合并(同 action 覆盖);
  回环超时(2s)显示保守禁用态并发布诊断,不假死、不静默丢交互。
- 插件想要产生副作用(调工具、发请求)走既有 `backend.*` / 工具权限路径;**事件回环本身不授予任何新
  能力**。

### 4.4 通道绑定

- **R7b(fixture,不经 Plugin Host)**:core 定义 `PluginUiProvider` trait(`describe() -> contributions`
  / `tree(scene) -> PluginUiTree` / `dispatch(event) -> PluginUiTree | Unchanged`)。fixture 以 Rust 实现
  同一契约,证明 schema 校验、注册、渲染与回环;壳仅消费 core 快照。
- **R7c(真 host,独立立项)**:沿既有 OpenCode Plugin Host 通道(loopback TCP + JSON-RPC,握手 capabilities
  交集)扩展方法族 `host.ui.describe` / `host.ui.event`(host 侧),复用 generation fencing 与实例生命
  周期(实例关闭即撤树);握手能力新增 `ui-contributions-v1`,**未协商成功 = 该插件 UI 贡献整体不激活**,
  在扩展面显示显式降级原因。`extension-host/src/protocol.ts` Zod schema、`protocol.schema.json`、
  `PROTOCOL.md` 与 Rust 实现逐字一致同步。这属于跨运行时大项,R7b 闭环验证后另行拍板。

### 4.5 安全模型

- **信任级别完全复用现有 trust store**(`Unknown / SourceApproved / Denied / Revoked` + activation
  epoch fencing):非 `SourceApproved` 的 UI 贡献不进注册表;撤销/降级即整代撤树。
- 渲染器零执行语义:无脚本、无 URL/资源加载、无系统对话框、无剪贴板(v0);组件集封闭,props 白名单,
  `deny_unknown_fields`。
- 事件回环按场景归属校验,插件收不到自己场景之外的交互;core 是唯一裁决点,壳与 provider 都不能互相
  直靠。
- 资源额度(§4.2 限额)防的是失控插件刷屏壳主循环;单飞合并 + 超时兜底保证壳可交互性不被拖死。
- **远程面显式声明**:插件 UI 贡献仅在有原生渲染器的面可用(当前 = GPUI 壳;web-ui 未接入)。Remote
  Command、移动 web、IM bot 遇「列出/打开插件场景」返回显式 unsupported,不做投影降级,防止把未验证
  渲染路径暴露给远程控制面。

## 5. 设计三:壳原生渲染器映射表

渲染器 = 纯函数式树遍历:`fn render(node: &PluginUiNode, events: &EventSink, cx) -> AnyElement`。
组件集 v0 全部有 gpui-component 0.7.1 现成对应物(`pub mod` 已核实),壳已大量复用同一组件族。

### 5.1 v0 组件集 → gpui 映射

| schema `type` | gpui-kit 落点(`gpui_kit::component::*`) | 备注 |
|---|---|---|
| `column` / `row` | `div().flex().flex_col()/flex_row()` + gap/padding props | 基础布局 |
| `scroll` | `scrollable()`(壳内先例) | 单一滚动子 |
| `divider` | `separator` 组件 | |
| `spacer` | 定高/定宽空 div | |
| `text` | 文本元素(`text_color`/`text_size`/truncate) | `tone` 角色、`truncate` 布尔 |
| `heading` | 文本元素 + 级联字号字重(level 1-3) | |
| `label` | `label` 组件 | 表单配文 |
| `button` | `Button`(variant/tone/disabled) | `on.press` |
| `input` | `TextInput` 单行 | `on.submit`、placeholder(@key) |
| `textarea` | `TextareaState`(壳 composer 同组件) | `on.submit` |
| `select` | `select` 组件(popover 下拉) | 选项 ≤ 32,`on.change` |
| `checkbox` / `switch` | `checkbox` / `switch` 组件 | `on.change` |
| `badge` / `tag` | `badge` 组件 | tone 角色 |
| `icon` | `gpui_kit::assets::IconName`(§5.3) | |
| `card` | 壳卡片语言(popover 底 + border_1 + radius token + shadow_sm,R2 welcome 同款) | |
| `progress` / `spinner` | `progress` / `spinner` 组件 | 只读展示 |
| `empty` | `empty` 组件(brand_empty 同款) | |
| `list` | 简单行列表(行渲染递归子树) | ≤ 200 行;虚拟化 v1 |
| `group` | 无视觉容器(布局组合用) | |

### 5.2 主题语义角色 → `cx.theme()` 映射

| 角色 | theme 字段 |
|---|---|
| `text` / `muted` | `foreground` / `muted_foreground` |
| `accent` / `accent-foreground` | `primary` / 主题对应前景 |
| `danger` | `danger` |
| `surface` / `border` | popover 底 / `border`(卡片语言) |

映射表唯一存在于渲染器;新增角色 = 改渲染器映射 + 本文表格,插件侧无感知。

### 5.3 图标封闭集

manifest 与树中的图标名是字符串,加载期校验 ∈ `gpui_kit::assets::IconName`(build.rs 生成集 + 壳
ExtraIcons 扩展;壳铁律:图标一律用 assets 枚举,不用组件内枚举)。集外名字 = 拒绝 UI 段(§3.4)。

### 5.4 事件语义(v0)

`press`(按钮)、`submit`(input/textarea Enter)、`change`(select/checkbox/switch 提交值)。**不做**
hover/焦点/滚动/键盘快捷键事件——频度与可访问性语义都是宿主职权(§3.3),回环成本不成立。

### 5.5 明确不支持(v0 拒绝渲染,类型化诊断)

`table` / `tree` / `chart` / `image` / 自定义绘制 / 嵌入 webview / 远程资源 / 树内弹窗组件(`dialog`/
`popover` 作为**插件树可控组件**)。注意区分:§3.3 允许「对话框」作为**贡献类型**进入稳定接口——即插件
声明一个对话框贡献、由宿主对话框队列编排打开(既有 permission dialog 同源语义),这条 v1+ 评审后再接;
v0 不做的是把弹窗组件交进插件树里自由弹出。其余留 v1+ 逐项评审。

## 6. R7b 壳内最小闭环(下一会话计划)

1. **契约**:`product-domains/src/plugin_ui.rs`——类型 + 校验纯函数(`deny_unknown_fields`、§3.4/§4.2
   全部限额、组件/props 白名单、`@key` 引用闭合检查),单测锚校验矩阵。
2. **投影 owner**:`assembly/core/src/plugin_ui.rs`(feature `plugin-runtime`)——`PluginUiProvider`
   trait + 快照发布 + generation;fixture provider(Rust 硬编码一个两控件场景:标题 + 计数按钮/输入框,
   `dispatch` 返回变更树)。
3. **壳**:`tabs.rs` 开 `SceneId::External(SceneRef)`(渲染按贡献分发,不逐场景长 match arm——兑现
   tabs.rs 头注释的接缝承诺);`plugin_ui.rs` 渲染器(v0 全集)+ 事件单飞回环 + i18n locale 选择;
   TabKind/TabsState/关闭语义测试扩展。
4. **验证口径**:`cargo test -p openbitfun-gpui-shell` + core 侧契约测试;`pnpm run fmt:rs`;
   `check:core-boundaries`(新契约归位检查);theme/audit 零新增(无新色无新键,预期不跑;i18n 若壳
   出现新键才跑)。Xvfb :97 冒烟:fixture 场景 tab 打开 → 树渲染(debug_bounds 量测)→ 点击/输入
   → 回环换树 → 关 tab 撤挂载。
5. **不假装**:fixture 是测试贡献,不进产品装配;产品插件来源接入(谁调用 provider)属 R7c 后的产品
   事项。

## 7. R7c 真 host 通道展望(独立立项,另行拍板)

`extension-host` 方法族扩展 + 握手 `ui-contributions-v1` + 实例生命周期挂接 + `PROTOCOL.md`/Zod/JSON
schema 三方同步 + 真插件 fixture(Bun)端到端。前置:R7b 闭环验收;同步推进 opencode-extension-
compatibility §6 已记录的安全补课(TOCTOU 快照、激活确认)避免 UI 贡献先于信任收口上线。

## 8. 升级兼容

- 旧包(无 `ui/` 段):零影响;`openbitfun.plugin.json` 未动。
- 旧壳读新包:manifest 可读,`ui/contributions.json` 的 `schemaVersion` 更高 → 显式「需更新版本」态,
  不猜渲染。
- 信任记录、activation epoch、包校验限额全部沿用,无数据迁移。
- 壳侧 `SceneId` 扩展为含 `External` variant 属编译期变更,无持久化形状(壳 tab 不落盘),无兼容负担。

## 9. 验收口径(本文自身)

- 设计经公子过目后,R7b 按第 6 节计划实现;实现 PR 需给出:契约测试清单、boundaries 结果、Xvfb 冒烟
  证据、以及本文与实现的偏差记录(有偏差先改本文)。
