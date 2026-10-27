# npad MVP 缺口收口规格 — 书签 / 最近文件菜单 / gutter 宽度

本文件是三个实施单元的唯一权威规格。实施代理先读本文件，再读仓库代码。
已确认的设计决策不再重新论证；照做即可。

## 0. 上下文与硬约束

- 仓库：`/home/ycl/github/npad`（独立仓库，remote `origin` → `git@github.com:ycl2045/notepad-plus-plus.git`，分支 `master`）。
- `/home/ycl/github/notepad-plus-plus` **仅作行为参考，不得修改**。
- 技术栈：Rust edition 2024 + gtk4 0.11.5 + syntect 5.3.0 + ropey 1.6.1。**不得新增依赖**，`Cargo.toml` / `Cargo.lock` 不得变动。
- 门禁（每个单元完成后必须自证）：
  - `cargo fmt --check`
  - `cargo test`（当前 106 个测试，全绿）
  - `cargo clippy --all-targets -- -D warnings`
  - `tools/verify.sh` → 必须打印 `VERIFY OK`
- 代码约定：单元测试写在同文件 `#[cfg(test)] mod tests`；生产路径禁止 `unwrap()`/`expect()`/`unsafe`；禁止 `as any` 式抑制；不重构已工作的子系统；不新增依赖。
- 提交纪律：**代理不提交、不推送**。只改代码 + 自证。所有 commit 由主控统一执行。
- 格式化纪律：**代理不要运行 `cargo fmt`（写模式）**，只可运行 `cargo fmt --check`；若它标记了**你负责的文件**，手动按 rustfmt 风格调整。理由：共享工作树里另一个代理可能正在写文件，写模式的 fmt 会改到别人的半成品。
- 并行纪律：不同单元的文件集互不相交，但共享同一工作树与同一 cargo target 目录。若 `cargo` 报错且错误位于**不属于你**的文件里，那是另一个代理的在途改动 —— 不要改那些文件，稍后重试。

## 1. 已决策的全局事项（不再讨论）

| 问题 | 决策 | 理由 |
|---|---|---|
| 书签状态放哪 | `EditorView`（每标签一个实例）里的 `RefCell<BTreeSet<usize>>` | EditorView 已拥有每标签的 caret/selection/scroll/绘制；`Tab` 里再放一份就要同步。`TabManager` 像读 caret/scroll 一样透过 editor 读写。 |
| 行号基准 | 内部与 session 文件一律 **0-based 行索引**；界面显示 `+1` | 与 `Document::byte_to_line`、`RowMap`、restore 路径一致，避免到处 off-by-one。 |
| 是否进 session | 进：`SessionFile.bookmarks: Vec<usize>`，`#[serde(default)]` | 书签是按路径索引的每文档视图状态，和 caret/scroll 同级。旧 session 文件缺该字段时载入为空。 |
| 无标题标签 | 不持久化 | session 本来就不含无标题标签（没有路径可重开）。仅当次运行有效，需在 README 写明。 |
| 跳转语义 | caret 移到目标行首字节；选区折叠；仅当目标不在视口内才滚动；next/previous 相对 caret 行**严格大于/小于**并在两端**回绕** | `goto_line(line + 1)` 已经通过 `set_caret` + `ensure_caret_visible` 实现了"移到行首 + 滚入视口"。回绕避免末端死键，且与查找栏默认回绕一致。 |
| 编辑导致书签漂移 | 本 MVP **不做**重映射：上方插入/删除行时行索引不重算 | 需求是"一组行号"；把 `Document::Edit` 接进书签重映射是另一个特性。过期行在绘制/跳转时忽略，save/restore 时丢弃。记为已知限制。 |
| 最近文件动作形态 | 一个带 **STRING 路径 target** 的参数化动作 `app.open-recent` | 与既有 `app.language` 完全同一套惯用法；避免注册 N 个动作；缺失文件处理集中一处；重建菜单只换数据不动 action group。 |
| 最近菜单何时刷新 | GTK GMenu 无"子菜单打开"信号，但有 **`submenu-action`** 属性：它跟踪一个 stateful 布尔动作在子菜单打开期间的状态 | 这是 GTK 唯一的"子菜单已打开"机制：不用定时器、不用点击拦截，鼠标与键盘都能触发。 |
| 最近文件缺失 | 每次刷新用 `Path::is_file()` 过滤并从内存最近列表剔除；若文件在菜单建好之后消失，激活时报错并剔除 | 死条目是噪音；最近列表是便捷功能而非文档。`is_file()` 同时排除目录与断链。 |
| 最近列表为空 | "Recent files" 子菜单**仍然存在**，内含一行禁用项 "No recent files"（动作 `app.recent-empty`，禁用） | 菜单位置稳定、标示功能存在。 |
| gutter 的"最大可见行号" | 用 `doc.line_count()`（任何可见行号的稳定上界） | 若用当前视口最后一行，滚动经过 9→10、99→100 时 gutter 宽度会跳变、文本区横向抖动。 |
| 行号对齐 | 按实际测得的数字串宽度**右对齐**：`number_x = gutter_w - pad - number_width` | 只加宽 gutter 而不修锚点（现在硬编码 `gutter_w - 16.0`）并不能消除重叠。右对齐同时腾出固定 8px 左侧留白，正好放书签标记。 |

## 2. 单元 A — 书签（最高优先级）

### 文件归属

- **新建**：`src/ui/editor/bookmark.rs`
- **修改**：`src/ui/editor/mod.rs`、`src/ui/editor/view.rs`、`src/ui/notebook.rs`、`src/ui/session.rs`、`src/app.rs`
- **不得触碰**：`src/main.rs`、`src/ui/mod.rs`、`src/ui/editor/rowmap.rs`、`src/ui/theme.rs`、`Cargo.toml`、`Cargo.lock`

### 数据模型

`src/ui/editor/bookmark.rs`（纯逻辑，不依赖 GTK，便于单测）：

```rust
pub type BookmarkSet = std::collections::BTreeSet<usize>;

/// 切换 `line` 的书签；返回切换后是否为已加书签。
pub fn toggle(set: &mut BookmarkSet, line: usize) -> bool;

/// 严格大于 `from_line` 的最小有效书签，到末尾回绕到最小有效书签。
/// 行号 >= line_count 为过期行，跳过。
pub fn next(set: &BookmarkSet, from_line: usize, line_count: usize) -> Option<usize>;

/// 严格小于 `from_line` 的最大有效书签，到开头回绕到最大有效书签。
pub fn previous(set: &BookmarkSet, from_line: usize, line_count: usize) -> Option<usize>;

/// 过滤持久化列表：排序、去重、丢弃越界行。
pub fn sanitise(lines: &[usize], line_count: usize) -> BookmarkSet;
```

`src/ui/editor/view.rs` 的 `imp::EditorView` 增加：

```rust
pub bookmarks: RefCell<BookmarkSet>,
```

**不要**给 `Tab` 加 bookmarks 字段。

`src/ui/session.rs` 的 `SessionFile` 增加：

```rust
pub struct SessionFile {
    pub path: PathBuf,
    pub caret: usize,
    pub scroll: f64,
    /// 0-based 行索引，已排序去重。
    #[serde(default)]
    pub bookmarks: Vec<usize>,
}
```

同时更新 `Default` impl（`bookmarks: Vec::new()`）以及 `src/ui/session.rs` 测试里所有手写的 `SessionFile { .. }` 字面量（部分用了 `..SessionFile::default()`，有两个没有）。

### `EditorView` 新增公开 API（`src/ui/editor/view.rs`）

```rust
/// 切换 caret 所在行的书签。不移动 caret、不滚动。
pub fn toggle_bookmark(&self) -> bool;

/// 已排序的、有效的（过滤掉过期行）0-based 书签行号。
pub fn bookmarks(&self) -> Vec<usize>;

/// 用 `lines` 的有效子集替换书签集合；重绘。
pub fn set_bookmarks(&self, lines: &[usize]);

/// 相对 caret 行的下一个/上一个目标行；不移动。
pub fn next_bookmark_line(&self) -> Option<usize>;
pub fn previous_bookmark_line(&self) -> Option<usize>;

/// 把 caret 移到下一个/上一个书签行的行首，折叠选区，滚入视口，返回目标行。
pub fn jump_to_next_bookmark(&self) -> Option<usize>;
pub fn jump_to_previous_bookmark(&self) -> Option<usize>;
```

实现要点：

- `toggle_bookmark`：行号 = `doc.byte_to_line(self.caret())`（caret 是字节偏移，0-based 行）；调用 `bookmark::toggle`；`queue_draw()`；返回新状态。
- `bookmarks`：`borrow().iter().copied().filter(|&line| line < self.line_count()).collect()`。
- `set_bookmarks`：`*self.imp().bookmarks.borrow_mut() = bookmark::sanitise(lines, self.line_count()); self.queue_draw();`
- `next_bookmark_line` / `previous_bookmark_line`：把 caret 行与 `self.line_count()` 传给纯函数。
- `jump_to_*`：有目标则 `self.goto_line(target + 1)`（`goto_line` 内部 `set_caret` → `ensure_caret_visible`），返回 `Some(target)`；否则 `None`。

### 动作 / 快捷键 / 菜单

| 动作名 | 菜单 | 快捷键 | 行为 |
|---|---|---|---|
| `toggle-bookmark` | Edit → Toggle Bookmark | `<Primary>b` | 切换 caret 行 |
| `bookmark-next` | Edit → Next Bookmark | `F2` | 跳下一个，回绕 |
| `bookmark-previous` | Edit → Previous Bookmark | `<Shift>F2` | 跳上一个，回绕 |

- `build_actions`（`src/app.rs` ~350 行）：三个 `gio::SimpleAction::new(name, None)`，闭包里用 `shared.tabs.current_editor()` 拿到当前 editor，调用对应方法，然后 `shared.update_status()`。
- `install_accels`（`src/app.rs` ~332 行）：三个 `app.set_accels_for_action("app.<name>", &[...])`。
- `build_menu`（`src/app.rs` ~285 行）：Edit 菜单在 search 段之后新增一段：

```
Edit
  Undo, Redo
  ---
  Cut, Copy, Paste, Select All
  ---
  Find..., Go to Line...
  ---
  Toggle Bookmark, Next Bookmark, Previous Bookmark
```

`Ctrl+B` 能到达应用层，是因为 `EditorView::handle_key` 对未处理的 Ctrl 组合返回 `false`（view.rs ~404-416），事件继续冒泡到应用加速键。`F2` 无 Unicode，同样冒泡。

### gutter 绘制锚点

- 函数：`impl WidgetImpl for EditorView::snapshot`（`src/ui/editor/view.rs` ~748 行）。
- 在 `for row in first..` 循环**之前**借用一次 `self.imp().bookmarks`（注意 `RefCell` 借用不要与循环内的其它借用冲突）。
- 循环内，在 `if line == caret_line { current_line ... }` 块**之后**、`let num = format!("{}", line + 1);` **之前**绘制：

```rust
if bookmarks.contains(&line) {
    snapshot.append_color(
        &theme.bookmark,
        &graphene::Rect::new(3.0, y + 2.0, 4.0, row_h_f - 4.0),
    );
}
```

`y` 是既有行顶（`row as f32 * row_h_f - scroll.scroll_y as f32 + pad`），`row_h_f` 是既有行高。定义模块常量 `BOOKMARK_X: f32 = 3.0`、`BOOKMARK_W: f32 = 4.0`。

- 绘制顺序：gutter 背景 → 当前行高亮（起点在 `gutter_w`，不覆盖 gutter）→ 书签标记（只在 gutter 内）→ 行号 → 正文 → caret。
- 单元 C 产出的 gutter 公式会腾出固定 8px 左侧留白，这个 4px 竖条正好落在里面，不需要额外 gutter 槽位。
- `Theme.bookmark` 现已有值（暗色 `#E0A040`、亮色 `#B86A00`），**不要改 `theme.rs`**。

### session 往返

- `TabManager::session_snapshot`（`src/ui/notebook.rs` ~184 行）：在 `SessionFile` 字面量里加 `bookmarks: tab.editor.bookmarks(),`。
- 恢复：`App::new`（`src/app.rs` ~202 行）在 `editor.set_scroll_offset(file.scroll);` 之后加 `editor.set_bookmarks(&file.bookmarks);`（不改 caret、不滚动，顺序安全）。
- `build_session` 已在关闭与打开后写盘，无需新增持久化路径。
- 文件已不存在时由 `prune_missing` 整条移除，其书签随之消失。

### 单元测试

`src/ui/editor/bookmark.rs`（8 个）：

| 测试名 | 断言 |
|---|---|
| `toggle_adds_then_removes_a_line` | `toggle(&mut set, 3)` 返回 true 且含 3；再调一次返回 false 且集合为空 |
| `next_moves_to_the_following_bookmark` | 集合 `{2,5,9}`：0→2、2→5、4→5、8→9 |
| `next_wraps_to_the_first_bookmark` | 集合 `{2,5}`：5→2、99→2 |
| `previous_moves_to_the_preceding_bookmark` | 集合 `{2,5,9}`：9→5、5→2、8→5 |
| `previous_wraps_to_the_last_bookmark` | 集合 `{2,5}`：2→5、0→5 |
| `navigation_returns_none_for_an_empty_set` | 空集合两者都返回 `None` |
| `navigation_skips_stale_lines` | 集合 `{2,99}`、`line_count=10`：只返回 2；集合只有 `{99}` 时返回 `None` |
| `sanitise_sorts_dedupes_and_drops_out_of_range_lines` | 输入 `[7,1,1,99]`、`line_count=10` → `{1,7}`；`line_count=0` → 空 |

`src/ui/session.rs`（2 个新增 + 更新既有）：

| 测试名 | 断言 |
|---|---|
| `session_file_roundtrips_bookmarks` | `bookmarks: vec![0,4,9]` 序列化/反序列化相等 |
| `session_file_without_bookmarks_field_defaults_to_empty` | 只有 path/caret/scroll 的 JSON 解析后 `bookmarks == []`（旧 session 兼容） |

更新既有 `roundtrips_through_json` 与 `save_then_load_round_trips` 使其覆盖 bookmarks（或改用 `..SessionFile::default()`）。

`EditorView` 的绘制/跳转不做 GTK 单测，用截图与纯函数测试覆盖。

### 验收

- 8 个 bookmark 测试 + 2 个新 session 测试通过。
- `Ctrl+B` 切换 caret 行书签；`F2`/`Shift+F2` 按下表跳转并回绕：

| 情形 | Next | Previous |
|---|---|---|
| 空集合 | 无动作，`None` | 无动作，`None` |
| caret 在首个书签之前 | 跳首个 | 跳最后一个（回绕） |
| caret 在某书签上 | 跳严格更大的下一个，回绕到首个 | 跳严格更小的上一个，回绕到末个 |
| caret 在末个书签之后 | 跳首个（回绕） | 跳末个 |
| 过期行（>= line_count） | 跳过 | 跳过 |

- gutter 只在已加书签的可见行显示 `Theme.bookmark` 竖条，行号仍可读。
- session 往返：`NPAD_AUTOCLOSE=1` 关闭后 `session.json` 含 `"bookmarks": [...]`，重启后竖条仍在。
- 旧 session JSON（无 bookmarks）载入不报错且无竖条。
- `cargo fmt --check` / `cargo test` / `cargo clippy --all-targets -- -D warnings` / `tools/verify.sh`（`VERIFY OK`）全过。

## 3. 单元 B — 最近文件菜单

### 文件归属

- **新建**：`src/ui/recent.rs`
- **修改**：`src/ui/mod.rs`（加 `pub mod recent;`）、`src/app.rs`、`src/main.rs`
- **不得触碰**：`src/ui/session.rs`（schema 已存在）、`src/ui/editor/*`、`src/ui/notebook.rs`、`Cargo.toml`、`Cargo.lock`

### 菜单结构

```
File
  New
  Open...
  Recent files ▸          [submenu-action = app.recent-open]
      <最近路径>
      ...
      No recent files     [app.recent-empty，禁用，仅空列表时]
  ---
  Save
  Save As...
  Close Tab
  ---
  Quit
```

- 最近子菜单是**存活的** `gio::Menu`，放在 `Shared` 里（`recent_menu: gio::Menu`），不是临时对象。
- 用 `gio::MenuItem` 挂载以便设置 `submenu-action`：

```rust
let item = gio::MenuItem::new_submenu(Some("Recent files"), &recent_menu);
item.set_attribute_value("submenu-action", Some(&"app.recent-open".to_variant()));
file_menu.append_item(&item);
```

- 条目写法（照抄 Language 菜单惯用法，app.rs ~320 行）：

```rust
let item = gio::MenuItem::new(Some(&label), None);
item.set_action_and_target_value(
    Some("app.open-recent"),
    Some(&glib::Variant::from(path.to_string_lossy().as_ref())),
);
recent_menu.append_item(&item);
```

- 空状态：一行 `gio::MenuItem::new(Some("No recent files"), Some("app.recent-empty"))`；`app.recent-empty` 是禁用的 `SimpleAction`，无处理器。子菜单不隐藏。

### 刷新机制（关键）

- 注册 `gio::SimpleAction::new_stateful("recent-open", None, &glib::Variant::from(false))`。
- 连接 `change_state`：请求值为 `true` 时调用 `Shared::refresh_recent_menu()`；**不要**阻止默认处理（默认处理器负责记录状态）。
- GTK 在子菜单打开/关闭时把该状态置 true/false，鼠标与键盘一致。
- 另外在这些时机也调用 `refresh_recent_menu()`：启动时（action group 插入后、`window.present()` 之前）、`open_file` 成功后、`open-recent` 成功或剔除失败项后。
- `Shared::refresh_recent_menu(&self)`：
  1. 克隆 `session.recent_files`；
  2. 用 `recent::menu_entries` 生成条目（过滤非文件、保持最近优先顺序）；
  3. 把过滤后的列表写回 `session.recent_files`；
  4. `recent_menu.remove_all()` 后重新 append 条目（或禁用占位项）；
  5. 绝不重建整个 menubar 或 action group。
- 若在 `change_state` 里直接改模型触发 GTK critical 或闪烁，退化为 `glib::idle_add_local_once` 延迟一帧重建，并记录这一帧陈旧；首选直接重建。

### 缺失文件行为

- 刷新时 `Path::is_file()` 过滤；目录、断链、非存在路径全部剔除。
- 若文件在菜单建好后消失：激活时报错（状态栏）并从最近列表剔除、刷新菜单。

### 动作

| 动作名 | 类型 | 行为 |
|---|---|---|
| `open-recent` | 参数化，STRING（路径） | 打开该路径到新标签，成功后把它移到最近列表首位并刷新菜单；失败则报状态栏错误并剔除 |
| `recent-open` | stateful BOOL | 子菜单打开探针（见上） |
| `recent-empty` | 禁用 | 空列表占位行 |

### 测试钩子

- `src/main.rs`：现有 `NPAD_ACTION` 保持可用；新增 `NPAD_ACTION_TARGET`（环境变量，字符串），使 `NPAD_ACTION=open-recent NPAD_ACTION_TARGET=/tmp/file.txt` 能带参数激活。
- 相应地给 `App` 增加一个带参数的激活入口（形如 `App::activate_with(&self, name: &str, target: Option<&str>) -> bool`），`App::activate` 复用它传 `None`。

### 单元测试

`src/ui/recent.rs`（4 个，用 `crate::ui::paths::temp_dir`）：

| 测试名 | 断言 |
|---|---|
| `menu_entries_keep_recency_order` | 两个存在的文件按输入顺序（最近优先）返回 |
| `menu_entries_drop_files_that_are_gone` | 一存在一缺失 → 只返回存在的那个，标签是完整路径 |
| `menu_entries_are_empty_when_nothing_exists` | 只有缺失路径 → 返回空 |
| `menu_entries_drop_directories` | 目录路径被 `is_file()` 过滤掉 |

`refresh_recent_menu`、`submenu-action` 接线与弹窗属于 GTK 层，走手动矩阵 + 动作截图（项目既定策略：菜单弹层不纳入离屏截图验证）。

### 验收

- 4 个 recent 测试通过。
- `File > Recent files` 位于 `Open...` 之后；子菜单最多 `RECENT_MAX`（20）条，最近优先，标签为完整路径。
- 打开子菜单触发 `app.recent-open` 的 `change_state(true)` 并重建列表；运行期间被删掉的文件在下次打开时消失。
- 空列表显示且仅显示一行不可点的 "No recent files"，子菜单仍能打开。
- 点击条目在新标签打开该文件、把它移到最近列表首位并刷新菜单；已消失的文件点下去在状态栏报错并被剔除。
- `NPAD_ACTION=open-recent NPAD_ACTION_TARGET=<存在的文件>` 产出的 PNG 显示新标签且状态栏为 `Ln 1, Col 1`。
- `cargo fmt --check` / `cargo test` / `cargo clippy --all-targets -- -D warnings` / `tools/verify.sh`（`VERIFY OK`）全过。

## 4. 单元 C — gutter 宽度与行号对齐

### 文件归属

- **修改**：`src/ui/editor/rowmap.rs`、`src/ui/editor/view.rs`
- **不得触碰**：`src/app.rs`、`src/ui/theme.rs`、`src/ui/session.rs`、`Cargo.toml`、`Cargo.lock`

### 公式与输入

- `view.rs` 新增常量 `DIGIT_WIDTH_SAMPLE = "0000000000"`，用既有 editor `pango::Layout` 量一个数字的步进：

```text
digit_w = layout.pixel_size().0 as f32 / DIGIT_WIDTH_SAMPLE.chars().count() as f32
```

  测量失败（非正）时回退到既有 `char_w`（再回退 8.0）。结果存进 `imp::EditorView` 新字段：

```rust
pub digit_w: std::cell::Cell<f32>,
```

- 数字位数：`digits = doc.line_count().max(1).to_string().len()`。用 `doc.line_count()`（任何可见行号的稳定上界），**不要**用视口最后一行，否则滚动时宽度跳变。
- 宽度：

```text
gutter_w = ceil(digits as f32 * digit_w) as i32 + 2 * GUTTER_PAD
```

  `GUTTER_PAD: i32 = 8` 为模块常量。
- 行号 x：

```text
number_x = gutter_w as f32 - GUTTER_PAD as f32 - number_width
```

  `number_width` 是 `layout.set_text(&num)` 之后 `layout.pixel_size().0` 的实测宽度。这样行号右对齐，右侧留 `GUTTER_PAD`，最宽的行号左侧也留 `GUTTER_PAD`。

### 计算位置

`src/ui/editor/rowmap.rs` 持有纯函数：

```rust
pub fn gutter_width(line_count: usize, digit_px: f32, pad: i32) -> i32;
pub fn number_x(gutter_w: f32, pad: i32, number_w: f32) -> f32;
```

`gutter_width` 用 `line_count.max(1).to_string().len()` 取位数并对分数乘积做 `ceil`；`number_x` 是纯减法，便于无 GTK 单测。

`EditorView::snapshot`（view.rs ~748 行）：

- 既有的 `char_w` 测量保持，再测 `digit_w` 并写入 `self.digit_w`。
- 把 `let gutter_w = RowMap::gutter_width(total_rows, 8, 8) as f32;` 换成：

```rust
let gutter_w = RowMap::gutter_width(doc.line_count(), digit_w, GUTTER_PAD) as f32;
let pad = GUTTER_PAD as f32;
```

- 把行号的 translate 换成：

```rust
layout.set_text(&num);
let (num_w, _) = layout.pixel_size();
let x = RowMap::number_x(gutter_w, GUTTER_PAD, num_w as f32);
snapshot.translate(&graphene::Point::new(x, y));
```

- 正文/选区/caret 的 x 仍是 `gutter_w + pad` 与 `char_w`，不动。
- `RowMap::ensure` 与换行逻辑不变。

### 失效与重绘

- `snapshot` 每帧重算 `digit_w`、`gutter_w` 和每个行号宽度，没有需要失效的缓存。
- 所有编辑路径本来就会 `queue_draw()`，主题切换、尺寸变化、滚轮滚动同理；行数跨过 10 的幂时下一帧自动生效。
- gutter 宽度用 `doc.line_count()` 而非 `total_rows`，软换行不会撑大行号列。
- 单元 A 的书签竖条落在固定 8px 左侧留白里，不需要额外槽位或重绘钩子。

### 单元测试

`src/ui/editor/rowmap.rs`：

| 测试名 | 断言 |
|---|---|
| `gutter_width_scales_with_digits`（更新） | `gutter_width(9, 8.0, 8) == 24`；`gutter_width(1000, 8.0, 8) == 48` |
| `gutter_width_rounds_up_the_measured_digit_advance` | `gutter_width(999, 9.6, 8) == 45`（ceil 28.8=29 +16）；`gutter_width(1000, 9.6, 8) == 55`（ceil 38.4=39 +16） |
| `gutter_width_uses_at_least_one_digit` | `gutter_width(0, 9.6, 8) == 26` |
| `number_x_right_aligns_inside_the_gutter` | `number_x(45.0, 8, 28.8) ≈ 8.2` 且 `x + 28.8 == 37.0 == gutter_w - pad`；一位数时 `x >= 8.0`（书签竖条止于 7.0）。浮点用 epsilon 比较 |

pango 实测由截图验证，不写 GTK 单测。

### 验收

- 4 个 rowmap gutter 测试通过（含 9.6px 分数步进）。
- 一个 150 行的文件滚到三位数行号可见时，每个行号完整落在 gutter 内，没有任何数字像素越过 gutter/正文边界；正文首列起始于 `gutter_w + 8`。
- gutter 宽度为 `ceil(3 * digit_w) + 16`（`digit_w ≈ 9.6` 时约 45px），不再是旧的 40px。
- 单元 A 的书签竖条仍在左侧留白里，不与行号重叠。
- `cargo fmt --check` / `cargo test` / `cargo clippy --all-targets -- -D warnings` / `tools/verify.sh`（`VERIFY OK`）全过。

## 5. 执行顺序

```
Wave 1: 单元 A（书签）
Wave 2: 单元 C（gutter）与单元 B（最近文件）并行 —— 单元 A 落地并提交后开始；两者文件集不相交
Wave 3: 主控统一跑门禁、审计 diff、提交、推送、更新 README 与 .omo/plans/npad-mvp.md
```
