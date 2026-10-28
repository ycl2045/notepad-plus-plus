# npad — Linux 原生 Notepad++ 类似编辑器（MVP 工作计划）

## 0. 已确认前提（不要再论证）

- 项目是**独立新仓库** `/home/ycl/github/npad`；`/home/ycl/github/notepad-plus-plus` 仅作行为参考，**不得修改**。
- 技术栈：**纯 Rust + gtk4-rs 0.11.5（`features=["v4_10"]`）+ 自绘编辑器 widget + syntect 5.3.0（`default-features=false, features=["default-fancy","yaml-load"]`）**。不使用 Scintilla/Lexilla（GTK4 删除 GtkContainer/GtkBin、官方无 GTK4 后端、Rust 绑定仅 GTK3 且落后 18 个月）；sourceview5 仅作兜底预案。
- 已锁版本：ropey `=1.6.1`、regex 1.13.1、serde 1.0.229、encoding_rs 0.8.42、anyhow 1。
- 已验证事实（勿再复验）：
  - `cargo build` 在依赖齐全下 7m17s（增量 6-8s）。
  - GTK4 + Rust 自绘 widget + `snapshot()` + pango 文本渲染在本机 Wayland 真实工作（900x563 PNG 已验证：行号 gutter、文本行、背景色均正确）。
  - GTK4 widget 关键 API 已确认：crate 名是 `gtk4`（非 `gtk`）；`Snapshot` 在 `gtk4::Snapshot`（含 `append_layout/append_color/translate/save/restore`）；`WidgetImpl::measure` 返回四元组；`font_metrics` 不存在（用 `pixel_size/pixel_extents`）；`Accessible` 需 gtk4 feature `v4_10`；IME 用 `EventControllerKey::set_im_context()` 而非手写 `filter_keypress`；`glib::wrapper!` 顺序是 `@extends` 在前、`@implements` 在后。
  - 截图工具：`tools/screenshot.sh`（应用内 render-to-texture，因 grim 在 GNOME Wayland 下不可用）。所有 UI 变更必须以此产出 PNG 并做像素/目视检查。
  - GApplication 会把 argv 当文件打开，传入参一律走环境变量（`NPAD_SHOT` 截图、`NPAD_AUTOCLOSE` 跑一遍关闭流程、`NPAD_ACTION`/`NPAD_FIND`/`NPAD_REPLACE` 触发单个动作）。

## 1. MVP 范围（必须交付）

1. 多标签页（新建/关闭/切换/未保存标记）
2. 多语言语法高亮（syntect，≥20 种常用语言 + 纯文本）
3. 查找/替换（支持正则、大小写、循环、替换全部）
4. 行号、当前行高亮、书签（切换/跳转）
5. 打开/保存（含 BOM/编码处理：UTF-8、UTF-16、GBK；CRLF/LF）
6. 会话记忆（打开的文件、光标位置、活动标签、最近文件、窗口尺寸）
7. 基础菜单 + 快捷键（文件/编辑/搜索/视图）
8. 明/暗主题切换（即时生效）

backlog（本周期不做）：宏、插件、十六进制、多文件搜索、自动补全、代码折叠、Diff、翻译体系。

## 2. 模块结构与职责

```
src/
  main.rs            启动、日志、panic hook
  app.rs             窗口、菜单/动作/快捷键、状态栏、主题应用
  ui/
    notebook.rs      标签管理（tab label + close button + dirty dot）
    find_replace.rs  查找替换面板（widget + 与 buffer/search 的绑定）
    editor/
      mod.rs
      view.rs        EditorView：GObject 子类、snapshot 绘制、measure/size_allocate、事件
      buffer.rs      Document：ropey 文本 + undo/redo 栈 + 修改计数
      cursor.rs      光标/选区（锚点+光标字节偏移）、移动与扩展操作
      scroll.rs      滚动控制器（自管 viewport，见 §3）
      highlight.rs   syntect 集成 + 行高亮缓存（按 rope 版本号失效）
      rowmap.rs      行→屏幕行映射、软换行折叠计算、gutter 宽度
      ime.rs         IMContext 接线（preedit 渲染与 commit）
    prefs.rs         偏好设置（serde 持久化：主题/字号/字体/换行/tab 宽/会话开关）
    session.rs       会话序列号与恢复
  syntax/mod.rs      扩展名→语法集映射、语言菜单生成、语言列表
  fs/
    io.rs            读/写：BOM 探测、编码转换、EOL 归一化
    detect.rs        BOM 与编码探测（BOM 优先 + UTF-8 严格校验 + GBK 尝试）
tests/
  buffer_test.rs  cursor_test.rs  search_test.rs  io_test.rs  session_test.rs
```

## 3. 关键设计决策（含被否方案）

### 3.1 滚动：自管 viewport（否决 GtkScrolledWindow 托管内容）
- 决定：EditorView 的 `measure` 返回**视口相关**的尺寸，`size_allocate` 里保存 viewport 宽高并**自己更新 `GtkAdjustment`**（`page_size=视口高`、`upper=内容总高`、clamp value）。`snapshot()` 里按 `scroll_y = vadjustment.value()` 计算首行并只画可见行。
- 理由：GTK4 无 text view gutter API；若把整篇内容交给 ScrolledWindow，行号 gutter 与文本的对齐/剪裁会与 compositor transform 耦合，且大文件会按整篇内容 measure。
- 被否：依赖 ScrolledWindow 自动平移 child（行号对齐难控、measure 阶段代价高）。

### 3.2 缓冲：ropey Rope + 结构化 undo 栈
- 决定：`Document { rope, undo: Vec<Edit>, redo: Vec<Edit>, epoch: u64 }`，`Edit { start: usize, old: String, new: String, kind: EditKind }`。连续同类型输入按时间窗(800ms)合并。所有坐标用**字节偏移**。
- 被否：Vec<String>（大文件不可行）；逐字符 undo（会话不可用）。

### 3.3 选择/光标
- 决定：`Selection { anchor: usize (字节), head: usize }`，`head` 为光标位置。列块选区 backlog（表示法已预留：暂存 `block: Option<(Range,Range)>` 字段不实现）。移动操作全部走「按行取行起点/终点 + 列 clamp」。
- 被否：(line,col) 索引（换行/Tab 宽度导致列计算漂移）。

### 3.4 行→屏幕行映射与 gutter
- 决定：`rowmap` 缓存「每个缓冲行在无换行模式下占 1 屏幕行；软换行模式下按 width 预算计算子行数」，缓存按 `Document.epoch` 失效。gutter 宽度由最大行号的 pango `pixel_size` 决定，随内容变化重算。
- 被否：每帧重算（性能）；硬编码固定 gutter 宽（100k 行文件会溢出）。

### 3.5 高亮：syntect 行缓存
- 决定：`HighlightCache { epoch, lines: Lru<usize, Vec<(Range<usize>, HighlightStyle)>> }`，只算可见行 ± 余量；每次编辑 `epoch++` 后失效相关行。语法按扩展名→`SyntaxSet.find_syntax_by_extension`，按 shebang/第一行二次兜底。
- 被否：整篇一次性着色（10MB 文件卡死）；不缓存（每帧着色卡顿）。
- 已落地：`Highlighter` 增加 `dark` 字段 + `set_dark()`，切换 syntect 主题（`base16-ocean.dark` / `base16-ocean.light`）并清缓存；`EditorView` 按 span 逐段绘制，各自取色，`set_theme` 时同步切换。

### 3.6 编码与换行
- 决定：读时按 BOM（UTF-8/UTF-16LE/BE）→ 否则 UTF-8 严格校验 → 否则 GBK(`encoding_rs`)；EOL 归一化为 `\n` 保存于内存，写回时按原 EOL 还原；`Document.eol` 记录 `\n|"\r\n"|"\r"`。保存时永远带 BOM iff 原文件有 BOM（除新建 UTF-8 无 BOM）。
- 被否：静默丢弃无效字节（数据损坏）；全仓 iconv（依赖重）。

### 3.7 IME
- 决定：`EventControllerKey` + `set_im_context(Some(im))`；`connect_commit` 插入文本；preedit 渲染在光标处（MVP：commit 后插入，preedit 仅画下划线文本）。按键未消费时走 `key_pressed` 的 Return/BackSpace/方向键/Home/End/PageUp/PageDown/快捷键。
- 已验证：GTK4 需 `set_im_context`，手写 `filter_keypress` 的签名不可用。

### 3.8 主题
- 决定：`prefs.theme = dark|light`；应用层用 `gtk::CssProvider` 设窗口样式，编辑器颜色（bg/fg/gutter/cursor/selection/current-line/number）由 EditorView 从当前主题读取渲染。切换即时 `queue_draw()` 全量重绘。
- 被否：仅靠系统暗色跟随（无法手动切换）；把颜色写死在 widget（换主题要改代码）。
- 已落地：CSS 必须同时覆盖 `windowbackground`（toplevel 背景来自该节点，不是 `window.npad-window`）与 `menubar`，并给 `box` 设背景色；否则亮色主题下窗口与菜单栏仍是黑色。GTK4 默认主题给按钮画了背景图，改背景色要同时写 `background-image: none`。

## 4. 工作单元、依赖与验收

| # | 单元 | 依赖 | 分派 | 验收标准（可测量） |
|---|---|---|---|---|
| U1 | 脚手架与文档结构（Cargo.toml feature、`src/` 骨架、AGENTS.md） | 无 | quick | `cargo build` 过；模块树与 §2 一致；空 shell 能起窗口 |
| U2 | `buffer.rs` Document + undo/redo + epoch | U1 | deep-low | 单测：插入/删除/撤销/重做/合并窗口/大文件(50MB 读入) 全过；`cargo test buffer_` 通过 |
| U3 | `cursor.rs` 光标选区与移动 | U2 | deep-low | 单测：word/line 移动、Home/End、扩展/收缩选区、越界 clamp、多字节(中文)边界 |
| U4 | `fs/io.rs` + `detect.rs` 编解码/EOL | U1 | deep-low | 单测：UTF-8/UTF-16LE/BE/GBK 双向、BOM 保留、CRLF↔LF、无效字节不丢失、写后读回逐字节一致 |
| U5 | `syntax/mod.rs` + `highlight.rs` | U1 | deep-low | 单测：≥20 扩展名映射命中、未知回退 plain、epoch 失效正确；对 5MB 文件只算可见行，`highlight` 耗时 < 16ms/帧 |
| U6 | `rowmap.rs` + `scroll.rs` | U2 | deep-low | 单测：行↔屏行映射、gutter 宽度、软换行预算；滚动 clamp、page_size 与内容高度一致 |
| U7 | EditorView 接线：绘制+输入+IME+滚动 | U2,U3,U6 | deep-high | 可运行：打开文件可见行号/文本/当前行/光标；输入/删除/回车/退格正确；中文 IME 可输入；100k 行文件拖动滚动流畅（截图 + 手动） |
| U8 | 查找/替换（regex/大小写/循环/替换全部） | U3 | unspecified-high | 单测：正则、转义、大小写、循环、替换全部、跨行；面板行为手动验收 |
| U9 | Notebook 多标签 + dirty 标记 | U7,U4 | unspecified-high | 手动+截图：新建/关闭/切换/未保存点/关闭前确认；≥10 标签正常 |
| U10 | 会话 + 偏好 | U4,U9 | unspecified-high | 单测 round-trip；手动：重启恢复所有打开文件、光标、活动标签、最近文件、窗口尺寸 |
| U11 | 菜单 + 快捷键 + 动作 | U7,U8,U9,U10 | unspecified-high | 手动：文件/编辑/搜索/视图全部动作可用、快捷键无冲突、常用键（Ctrl+S/F/B/G）符合预期 |
| U12 | 主题切换 + 状态栏 | U7,U10 | visual-engineering | 截图：暗/亮切换即时生效、颜色对比度可读、状态栏显示位置/编码/EOL/语言 |
| U13 | 打包：.desktop/metainfo/图标 + 可复现验证脚本 | U12 | quick | `tools/verify.sh` 一键：build + test + screenshot；`cargo build --release` 产 binary |

并行关系：U1 完成后 U2/U3/U4/U5/U6/U8 可并行；U7 需 U2+U3+U6；U9-U12 串行依赖 U7；U13 最后。

## 5. 构建与验证

- `cargo build`（增量 ~7s）、`cargo test`、`cargo clippy -- -D warnings`、`cargo fmt --check` 作为提交门槛。
- UI 验证：`tools/screenshot.sh /tmp/x.png` + 像素分析脚本（检查 gutter 边界/文本带数量/背景色）+ 人工目视。
- 不可单测（IME、Wayland 行为、拖放、菜单弹层）走手动矩阵，记录在 `docs/manual-qa.md`。
- 仅做单测会漏的问题（IJ 性能、IME 组合）必须给手动步骤，不许用「已单测」代替。

## 6. 风险登记（按严重度）

1. **自管滚动与 GTK4 分配模型的交互**（viewport 尺寸、adjustment 更新时机错位导致空白/跳滚）→ 预警：snapshot 里出现内容跳动或 allowance 为 0。缓解：size_allocate 里写 adjustment 后立即 clamp + queue_draw；仍不行则把「内容高度」改为 widget `height_request` 驱动。
2. **IME preedit 与光标位置**（中文输入时光标不动、候选位置错位）→ 预警：输入中文后光标停在行尾。缓解：preedit 期间用临时文本注入光标处渲染。
3. **syntect 大文件帧耗时** → 预警：>16ms/帧、拖动掉帧。缓解：Lru 行缓存 + 限制单帧着色行数 + 低于阈值退化为 plain 着色。
4. **undo 粒度导致体验差**（一次撤销删一段）→ 预警：单次退格删掉整段。缓解：时间窗合并 + 结构编辑不合并 + 单测覆盖。
5. **ropey 2.0 beta 引入** → 预警：API 不兼容。缓解：Cargo.toml 锁 `=1.6.1`（已锁）。
6. **GTK4 无 gutter API 导致行号与文本不同步滚动** → 已由 §3.1 自管滚动规避；预警：行号列与文本行错位（截图可见）。

## 7. 需用户确认的 3 个问题

1. 默认字体（如 `monospace 11` / `JetBrains Mono`）与是否内置字体回退链？
2. 「会话记忆」是否默认开启（每次退出全量保存）还是仅在正常退出时保存？
3. 打包只交付 `target/release/npad` + `.desktop`，还是要出 `.deb`/flatpak？

## 8. 进展状态（2026-10-27）

已完成并推送（`master`，`tools/verify.sh` 全绿：106 tests + clippy 无告警 + 截图）：

- U1–U10、U13 全量完成；U11 菜单/快捷键/动作已可用；U12 主题切换即时生效、状态栏显示 `Ln/Col | encoding | EOL | language`。
- 语法高亮已从「只算 span」推进到**逐 span 绘制**，配色跟随明暗主题。
- 明暗两套 chrome（toplevel 背景、菜单栏、header bar、状态栏）已随主题切换，亮色主题不再残留黑底。
- 查找/替换栏、查找上一个、正则替换全部、选择与多行选区绘制已接线。
- **书签**（§1 第 4 项）已落地：每标签一组书签行号，`Ctrl+B` 切换、`F2`/`Shift+F2` 前后跳转（两端回绕），gutter 用 `Theme.bookmark` 画琥珀色竖条，书签随 session 存取（无标题标签仅当次运行有效；行号不随上方增删行重映射）。
- **最近文件菜单**已落地：`File → Recent files` 子菜单用 GTK 的 `submenu-action` 钩子在每次打开时重建行，消失的文件被剔除，空列表显示禁用的占位行。
- **gutter 宽度**已修正：宽度按实测数字步进 × 位数向上取整 + 两侧留白计算，行号在 gutter 内右对齐，3 位以上行号不再压正文。

三个缺口的决策与验收规格见 `.omo/plans/npad-gap-closure.md`。

仍待完成（下一批）：

1. **最近文件菜单的手工验收**：GTK 弹层不在离屏截图范围内，`File → Recent files` 的弹出内容需在真实会话里目视确认（`submenu-action` 刷新、空态占位行、点击条目打开文件）。
2. 编码 / EOL 选择器、书签随增删行重映射、宏与插件等仍在本 MVP 范围之外。
