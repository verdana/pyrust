# pyrust — 个人拼音输入法项目手册

## 项目概述
这是一个追求极致性能、完全隐私可控的跨平台（Windows/macOS）拼音输入法。
- **核心逻辑**：采用 Rust 编写，通过 Cargo Workspace 管理。
- **运行模型**：三线程架构（系统回调线程、工作逻辑线程、UI 渲染线程），确保输入绝对不卡顿。

## 技术栈
- **语言**：Rust (Stable)
- **平台接入**：`windows-rs` (TSF), `objc2` (InputMethodKit)
- **UI 渲染**：`egui` (自绘候选窗)
- **存储**：SQLite (个人词库持久化), mmap (基础词库只读映射)
- **配置**：TOML

## 项目结构 (代码地图)
- `crates/engine-core/`: 拼音切分、DP 路径选择、状态机管理。
- `crates/dict/`: 词库引擎（UserDict, BaseDict）。
- `crates/platform-adapter/`: 操作系统 FFI 接入与 ImeBackend trait 定义。
- `crates/ui-crate/`: egui 渲染逻辑。
- `crates/yas-config/`: 全局配置加载。
- `dict-compiler/`: 离线工具，将文本词表编译为二进制 Trie。
- `pyrust/`: 二进制入口进程，负责线程调度。

## 代码仓库
- 代码仓库在项目根目录（`/home/verdana/workspace/pyrust/`）
- Windows 构建目录：`/mnt/c/Users/Verdana/Desktop/pyrust/`
- 修改 TSF 代码后，需手动复制到 Windows 目录：`cp crates/tsf/src/*.rs /mnt/c/Users/Verdana/Desktop/pyrust/crates/tsf/src/`

### 代码同步规则（NEVER）
- **NEVER** 从 Windows 测试目录（`/mnt/c/Users/Verdana/Desktop/pyrust/`）反向复制代码到 WSL 工作区
- **NEVER** 读取 Windows 目录中的源文件来覆盖 WSL 中的文件
- 代码同步**只能单向**：WSL → Windows
- Windows 目录仅用于编译测试，不是代码源

## 常用命令
```bash
# 检查代码 (Linux)
cargo check
# 运行单元测试
cargo test
# 构建全平台 (通过 feature 切换)
cargo build --release
# 构建词库
cargo run -p dict-compiler -- --input <src> --output <dest>

# TSF crate 交叉编译 (必须在 WSL 中用 Windows target)
cd crates/tsf
cargo check --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

## Rust 编码规范与最佳实践

### 1. 安全性 (Safety)
- **禁止使用 `unwrap()`**：在任何生产代码中禁止直接使用 `unwrap()`。优先使用 `?` 传播错误，或使用 `.expect("详细说明为何此处不可能失败")`。
- **慎用 `unsafe`**：仅在 FFI 调用（如 `windows-rs` 或 `objc2`）时使用。所有 `unsafe` 块必须附带 `// SAFETY:` 注释，说明内存安全性保障逻辑。
- **变量命名**：遵循 Rust 标准（变量 `snake_case`，结构体/Trait `PascalCase`）。

### 2. 性能与内存 (Performance)
- **关键路径无 IO/分配**：工作逻辑线程（Worker Thread）在处理 `handle_key` 时，禁止执行同步磁盘 IO。
- **避免过度 `clone()`**：对于大型词表、配置等，使用 `Arc<T>` 或 `&T` 共享。
- **零拷贝优先**：读取基础词库必须使用 `memmap2` 映射，并在字节流上直接操作 Trie。
- **集合预分配**：在已知大小的情况下，使用 `Vec::with_capacity` 减少重分配。

### 3. 并发与同步 (Concurrency)
- **消息传递优于共享内存**：优先使用 `crossbeam-channel` 或 `flume` 进行线程间通信。
- **原子操作**：对于简单的状态标记，优先使用 `std::sync::atomic` 而非 `Mutex`。
- **死锁防御**：尽量缩小锁的粒度，避免在持有锁的情况下调用外部模块的函数。

### 4. 错误处理 (Error Handling)
- **库错误**：使用 `thiserror` 定义模块特有的错误枚举。
- **应用错误**：在 `pyrust` 二进制入口中使用 `anyhow` 进行顶层错误捕获和日志记录。

## 工作流程要求
1. **先规划 (Plan)**：修改核心逻辑前，必须先更新设计文档（`docs/superpowers/specs/`）并同步思路。
2. **三步走 (Step-by-Step)**：实现功能 → 编写单元测试 → 跨平台兼容性检查。
3. **性能回归**：涉及词库查询的修改，必须运行 `cargo bench`（如有）或记录查询耗时。

## 禁忌 (NEVER)
- **NEVER** 引入任何网络相关的依赖（除非明确通过 `net` feature 隔离）。
- **NEVER** 在系统回调线程（Platform Thread）中进行阻塞调用或锁等待。
- **NEVER** 将敏感信息（如密码模式下的按键）记录到日志或个人词库。
- **NEVER** 忽略编译器警告 (`#[deny(warnings)]` 在 CI 中生效)。

### TSF IME 注册表修复 — IME 常驻键盘列表（2026-05-03）

**问题**：IME 在 COM 层面激活成功，但仍从 Windows 键盘列表中消失。

**根因**：TSF 注册表键结构不完整且格式错误：
1. Category 格式错误 — 用了 `Category\Category\{GUID}` 而非标准 `Category\Item\{GUID}\{CLSID}`
2. 缺少 `LanguageProfile\{LangID}\{profile}` 子键 — Windows 不知道 TIP 属于哪个语言
3. 未调用 `ITfCategoryMgr::RegisterCategory` COM API（仅写了原始注册表键）
4. 缺少 `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT` 类别 — Windows 8+ 必需
5. 缺少 HKCU 注册 — Windows 11 需要当前用户的注册

**修复**（`crates/tsf/src/registry.rs`）：
- 注册表格式修正：`Category\Item\{CAT_GUID}\{CLSID}` + `LanguageProfile\{LangID}\{profile}`
- 添加 `ITfCategoryMgr::RegisterCategory` COM 调用（KEYBOARD + IMMERSIVESUPPORT + UIELEMENTENABLED + COMLESS）
- 添加 HKCU 注册
- 移除无效的 `ActivateProfile` 调用

**结果**：IME 可正常添加并常驻 Windows 键盘列表。

详细记录见 `docs/tsf-troubleshooting.md`。

### Explorer 崩溃问题（2026-05-03）— 已修复

**现象**：切换到 pyrust 输入法时，任务栏消失后重新出现（Explorer 重启）。

**根因**：Bridge 线程初始化（egui/winit）在 TSF COM 回调线程中创建窗口，导致 COM 重入死锁。

**修复**：延迟 Bridge UI 线程初始化——仅在 Activate 中初始化 engine，不启动 egui 窗口。

### egui → Win32+GDI 迁移（2026-05-03）

**原因**：egui/eframe/winit 在 TSF DLL 环境中无法正常工作：
- OpenGL 上下文在 DLL 进程中创建失败或渲染黑色
- winit EventLoop 在 DLL 重载后无法重建（`EventLoop can't be recreated`）
- `WS_EX_NOACTIVATE` 与 OpenGL 透明窗口冲突

**方案**：用原生 Win32 + GDI 替换 egui：
- `CreateWindowExW` + `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`
- GDI `TextOutW` + `Rectangle` 渲染候选词
- `ShowWindow(SW_HIDE/SW_SHOW)` 切换显示（永不销毁窗口）
- 全局 `OnceLock` + `Mutex` 跨线程共享状态

**结果**：候选框正常显示，文字可上屏，不抢焦点。

### 候选框位置问题（2026-05-04）— 已修复

**现象**：候选框贴屏幕左上角，不跟随光标。

**根因**：Win32 caret API（`GetGUIThreadInfo`/`GetCaretPos`）在 TSF 环境中不可靠——许多应用不使用 Win32 caret，导致坐标始终为 (0,0)。

**修复**：
- TSF 线程通过 `ITfContextView::GetTextExt`（TSF 标准 API）获取光标屏幕坐标
- 新增 `CaretPosEditSession`（`edit_session.rs`）— 同步 edit session 调用 `GetTextExt`
- `Request::KeyPress` 增加 `caret_pos` 字段，TSF → Worker → UI 传递坐标
- `window.rs` 优先使用 TSF 提供的位置，无效时 fallback 到 Win32 caret API

### 多音节候选词回退（2026-05-04）— 已修复

**现象**：输入 `woyao` 时，输入到 `a` 后候选框消失（0 candidates）。

**根因**：`update_candidates()` 将整个拼音串 `"wo yao"` 作为单个 key 查词库，但词库只存储单音节 key（`"wo"`, `"yao"`）。

**修复**（`engine-core/src/lib.rs`）：多音节查询返回 0 candidates 时，回退到第一个音节单独查找。

### 贪心拼音切分 + 候选词回退（2026-05-04）— 已修复

**现象**：输入任意字母组合（如 `jjjj`、`asdf`）时，候选框为空或只有单字。

**根因**：
1. `best_segmentation()` 要求完整切分，无法切分的输入（如 `jjjj`，`j` 不是合法音节）返回空
2. 回退逻辑只尝试第一个音节，不支持多字组合

**修复**（4 个文件）：
- `dict/src/trie.rs`：`is_end()` 和 `root()` 改为 `pub`
- `dict/src/pinyin_table.rs`：新增 `shortest_syllable_for_char()` — BFS 找某字母开头的最短音节（如 `'j'` → `"ju"`）
- `engine-core/src/pinyin.rs`：新增 `greedy_segmentation()` — 贪心左切回退，无法切分时用最短音节代理
- `engine-core/src/lib.rs`：`update_candidates()` 三级回退策略：
  1. 完整 n-gram 查词（如 `"ju ju ju ju"`）
  2. 单字组合（每个音节单独查词，拼成 N 字结果，如 `"据据据据"`）
  3. 缩短 n-gram 窗口（如 `"ju ju"`）

**结果**：任意字母组合都能产生候选词，字数与输入音节数匹配。

### Shift 中英切换 + 回车上屏（2026-05-05）— 已修复

**功能 1：Shift 切换时先上屏拼音**
- 中文模式输入 `nihao`，按 Shift → 先将 `"nihao"` 上屏，再切换到英文模式
- 实现：`ShiftResult::CommitThenToggle` — OnKeyUp 先发 `KeyPress(VK_ENTER)` 上屏，再发 `ToggleMode`
- `TsfBridge` 新增 `has_pinyin: Arc<AtomicBool>` 跨线程跟踪拼音缓冲区状态

**功能 2：回车键将拼音原样上屏**
- 中文模式输入 `nihao`，按回车 → `"nihao"` 作为普通文字上屏，保持中文模式
- 实现：`Action::CommitRaw(String)` — Enter 键返回缓冲区原始内容

**变更文件**：
- `engine-core/src/lib.rs`：新增 `Action::CommitRaw`，Enter 键处理改为返回原始拼音
- `tsf/src/tip.rs`：新增 `ShiftResult` 枚举，`handle_shift_key` 返回 `CommitThenToggle`
- `tsf/src/bridge.rs`：新增 `has_pinyin` 原子标记，Worker 每次状态变更后更新

### 线程生命周期管理 + IPC 超时优化（2026-05-05）

**问题 1：辅助线程无法退出**
- `forwarder` 阻塞在 `action_rx` 迭代上，`config_watcher` 用 `park()` 阻塞
- DLL 卸载时线程泄露，可能导致宿主程序崩溃

**修复**：
- `TsfBridge` 新增 `shutdown: Arc<AtomicBool>` 标志
- `config_watcher` 改为 `sleep-loop` 检查 shutdown 标志（替代 `park()`）
- `forwarder` 改为 `recv_timeout(500ms)` + 检查 shutdown 标志
- `shutdown()` 设置标志后发送 `Request::Shutdown`

**问题 2：oneshot 超时过长**
- `oneshot::Receiver::recv()` 超时 5 秒，宿主 UI 线程会挂起

**修复**：超时从 5s 缩短至 200ms，超时返回 `None` → `Response::Passthrough`

### 中文标点自动映射（2026-05-05）— 已修复

**功能**：中文模式下标点按键自动映射为全角中文标点。

**映射表**：
| 按键 | 标点 | 按键 | 标点 |
|------|------|------|------|
| `,` | ， | `Shift+,` | 《 |
| `.` | 。 | `Shift+.` | 》 |
| `;` | ； | `Shift+1` | ！ |
| `?` | ？ | `=` | ＝ |
| `"` | ""（配对） | `-` | — |
| `'` | ''（配对） | `\` | 、 |
| `(` | （ | `)` | ） |

**行为规则**：
- 仅中文模式生效，英文模式不映射
- 有未上屏拼音时，先上屏拼音再上屏标点（合并为一次 Commit）
- 引号支持配对交替（开/闭引号自动切换，`last_quote_was_open` 状态跟踪）

**架构**：
- `engine-core/src/lib.rs`：`handle_punctuation(vk, shift)` — 标点映射表 + 引号配对状态
- `tsf/src/tip.rs`：`should_consume_key` 新增标点 VK 码消费
- `tsf/src/bridge.rs`：`char_from_vk` 新增标点 VK→ASCII 字符转换

### TSF Composition String（拼音上屏 + 下划线）（2026-05-05）

**功能**：拼音直接写入应用文档（带下划线），候选词选中后替换为最终文字。现代输入法标准行为。

**输入流程**：
```
用户键入 n → 应用显示 "n"（实线下划线，composition 状态）
用户键入 i → 应用显示 "ni"（实线下划线）
用户按空格 → "ni" 替换为 "你"（点状下划线，已选候选词）
最终确认 → "你"（无下划线，普通文字）
Esc 取消 → 拼音文字从应用文档中清除
```

**架构**：
- `engine-core/src/lib.rs`：`Action::UpdatePreedit` 携带拼音文本，`Action::ClearPreedit` 表示拼音清空，`Action::CommitAndPreedit` 表示提交+新输入
- `tsf/src/lib.rs`：`Response::ConsumedWithText(String)` 和 `Response::CommittedWithPreedit(String, String)`
- `tsf/src/bridge.rs`：`Action::UpdatePreedit` → `ConsumedWithText`，`CommitAndPreedit` → `CommittedWithPreedit`
- `tsf/src/edit_session.rs`：`CompositionEditSession` — 管理 `StartComposition` / `SetText` / `EndComposition` 生命周期
- `tsf/src/display_attrs.rs`：`InputDisplayAttr`（`TF_LS_SOLID`）和 `ConvertedDisplayAttr`（`TF_LS_DOT`），`PyrustEnumDisplayAttr` 枚举器
- `tsf/src/tip.rs`：`handle_keypress` 匹配 `ConsumedWithText` → 更新 composition，`Committed` → 结束 composition

**关键 API**：
- `ITfContextComposition::StartComposition(ec, range, None)` → 返回 `ITfComposition`
- `comp.GetRange()` → 获取 composition 范围
- `range.SetText(ec, 0, &text)` → 替换 composition 文字
- `comp.EndComposition(ec)` → 结束 composition（文字留在文档中）

**生命周期管理**：
- `self.composition` 在 `StartComposition` 成功后赋值 `Some(comp)`
- `OnCompositionTerminated` 回调中置为 `None`
- `Deactivate` 中清理
- `handle_keypress` 结束时检查引擎 `pinyin_buffer_empty()`，清理残留 composition

### 按键处理状态（2026-05-03）

**已解决**：
- `ITfContextKeyEventSink::OnKeyDown` 现在正确创建 edit session 插入文字
- 空格键选第一候选词，数字键选对应候选词
- 光标移到插入文字末尾

### windows-rs 升级 0.58 → 0.62（2026-05-03）

**变更**：
- `Option<&T>` 参数改为 `windows::core::Ref<'_, T>`（COM 接口方法签名变化）
- `Win32::Foundation::BOOL` 改为 `windows::core::BOOL` 或 `bool`
- `HINSTANCE` 改为 `HMODULE`
- `implement` feature 不再需要显式声明（内置）
- 新增 `Win32_System_Variant` feature（`VARIANT` 类型支持）

**注意**：`crates/tsf/` 独立于 workspace，依赖版本冲突需单独管理。

### 内联拼音显示修复（2026-05-05）— 已修复

**现象**：
- 键入拼音（如 `nihao`）时，Notepad 中**不显示任何 inline 文本**
- 候选框正常弹出、候选词显示正常
- 回车后文字正常上屏（Commit 路径正常）
- 拼音叠加：输入 `a`→`a`，`b`→`aab`，`c`→`aababc`

**根因**：
1. `StartComposition` 在所有测试条件下都返回 `E_INVALIDARG`（参数错误）。原因不明，可能与 Notepad 的 TSF 文本存储实现有关
2. 因为没有 composition，每次按键都从光标位置重新插入**完整拼音缓冲**（如 "ab"、"abc"），而非替换前一次文本

**修复**（`edit_session.rs` + `tip.rs`）：
- 放弃依赖 `ITfContextComposition::StartComposition`
- 新增 `preadit_range: Rc<RefCell<Option<ITfRange>>>` 字段，在 PyrustTip 和 CompositionEditSession 之间共享
- 首次按键：`GetSelection` → 光标范围 → `SetText` 插入 → 存储 `ITfRange` 到 `preadit_range`
- 后续按键：用存储的 range → `SetText` **替换**（而非重新插入）
- 提交/清除：`SetText` 最终文本 → 清除 `preadit_range`
- `handle_keypress` 中 bridge 借用改为块作用域，避免与 cleanup 段冲突
- 所有 `RequestEditSession` 调用添加错误日志（不再静默丢弃）

**关键教训**：
- `ITfRange::SetText` 后 range 被折叠到文本末尾（0 字符），需要 `ShiftStart(-N)` 展开
- `GUID_PROP_COMPOSING` 是 TSF 只读属性，TIP 调用 `SetValue` 会返回 `E_INVALIDARG`
- `ITfCategoryMgr::RegisterCategory` 在 `DllRegisterServer` 阶段成功，在 `Activate` 阶段失败——category 注册必须在注册服务器时完成

### 拼音下划线（未解决）— 已知限制

**状态**：display attribute 的 category 注册和 GUID atom 注册均已成功，`SetValue(GUID_PROP_ATTRIBUTE)` 返回成功，`GetDisplayAttributeInfo` 被 TSF 调用。但下划线不渲染。

**推测根因**：TSF 的 display attribute 渲染依赖 active composition（通过 `StartComposition` 创建）。由于 `StartComposition` 始终返回 `E_INVALIDARG`，display attribute 被忽略。

**保留的代码**：
- `display_attrs.rs`：`InputDisplayAttr`（TF_LS_DOT 虚线）和 `ConvertedDisplayAttr`
- `registry.rs`：`GUID_TFCAT_DISPLAYATTRIBUTE_PROVIDER` 和 `GUID_TFCAT_DISPLAYATTRIBUTE` 类别注册
- 后续只需解决 `StartComposition` 问题，下划线即可自动生效

## 调试经验 (Lessons Learned)

### TSF IME 在系统键盘列表中消失（2026-05-02）

**问题**：`regsvr32 tsf.dll` 成功，IME 出现在 Windows 键盘列表中，但切换后立即消失。

**根因**：`ITfTextInputProcessor::Activate` 中缺少两个关键的 COM Sink 注册：
1. **未调用 `ITfSource::AdviseSink(IID_ITfThreadMgrEventSink)`** — TIP 没有注册线程管理器事件接收器，导致 TSF 认为 IME 无响应
2. **未调用 `ITfKeystrokeMgr::AdviseKeyEventSink`** — TIP 没有注册按键事件接收器，无法接收输入

**修复**（`crates/tsf/src/tip.rs`）：
- `Activate` 中：通过 `tm.cast::<ITfSource>()` 获取 source，调用 `AdviseSink` 注册 `ITfThreadMgrEventSink`，保存 cookie
- `Activate` 中：通过 `tm.cast::<ITfKeystrokeMgr>()` 获取 keystroke_mgr，调用 `AdviseKeyEventSink` 注册 `ITfKeyEventSink`
- `Deactivate` 中：在释放 thread_mgr 之前，先调用 `UnadviseSink(cookie)` 和 `UnadviseKeyEventSink(tid)` 清理
- 使用 `self.as_interface_ref::<I>()` (需要 `use windows::core::ComObjectInterface`) 获取 `&self` 对应的 COM 接口引用

**第二次尝试（2026-05-02）——容错模式 + 文件日志**：
- 将 `Activate` 中所有非关键步骤（Sink 注册、Bridge 初始化）改为非致命——失败只记日志，不返回错误
- 唯一致命的步骤：`ITfThreadMgr` 不存在（`ptim` 为 None）
- 添加文件日志 `C:\Users\Verdana\pyrust_tsf.log` 到 `tip.rs`、`dll_exports.rs`、`registry.rs`，诊断每步的成功/失败
- 日志覆盖：`DllMain`、`DllGetClassObject`、`DllRegisterServer`、`Activate` 每步、`Deactivate`、所有回调（`OnKeyDown`、`OnInitDocumentMgr`、`OnSetFocus` 等）

**诊断方法**：在 Windows 上运行后查看 `C:\Users\Verdana\pyrust_tsf.log`

**教训**：
1. TSF 框架不会自动注册任何事件接收器——TIP 必须主动注册
2. `ITfThreadMgr` 可 cast 为 `ITfSource` 和 `ITfKeystrokeMgr`（它们在同一 COM 对象上）
3. 注销 Sink 必须在释放 thread_mgr 之前进行
4. windows crate 的 `#![cfg(windows)]` 意味着在 Linux 上整个 crate 为空，必须用 `--target x86_64-pc-windows-gnu` 交叉编译
5. **`Activate` 中任何一步返回错误都会导致 TSF 立即停用 TIP**——所有非关键步骤必须容错

### TSF crate 编译注意事项

- `crates/tsf/` **不在 workspace members 中**（因为依赖 windows 0.58，与 workspace 的 windows 0.48/0.52 冲突）
- 编译 tsf 需进入该目录单独执行：`cd crates/tsf && cargo check --target x86_64-pc-windows-gnu`
- `HINSTANCE` 初始值必须是 `HINSTANCE(std::ptr::null_mut())`，不能用 `HINSTANCE(0)`（类型不匹配）

### dev_input_loop 与 UI 渲染的时序问题（2026-04-30）

**问题**：候选词窗口始终为空，但日志显示 engine 找到了候选词。

**根因**：`dev_input_loop` 在每行输入末尾自动发送 Enter 键清空缓冲区。所有按键（包括清空的 Enter）在数毫秒内处理完毕，UI 渲染时只能看到已清空的状态。

**教训**：
1. **dev_input_loop 中不要在行末自动发送会改变状态的控制键**（如 Enter、Esc）。保持输入缓冲区在用户未明确操作前不变。
2. **诊断 UI 不更新时，先确认数据是否真的到达了 UI**。用 `eprintln!` 分别在发送端和接收端打印，确认 channel 是否畅通。
3. **egui 的 `update()` 调用频率（~20Hz）远低于 keystroke 频率**。`try_recv()` 的 while 循环会一次消费所有积压消息，最终状态取决于最后一条消息。
4. **`crossbeam_channel::unbounded()` 的 `Receiver` 支持 Clone**，但每条消息只投递给一个 receiver。确保只有一个 receiver 在消费。

### egui 在 Windows 上的注意事项

- **非主线程运行需要 `with_any_thread(true)`**（winit Windows 扩展），否则 `eframe::run_native` 不会启动事件循环。
- **默认字体不支持中文**，需在 `eframe::CreationContext` 中加载系统字体（如 `msyh.ttc`）。
- **透明窗口 (`with_transparent(true)`) 可能导致窗口不可见**，调试阶段建议使用 `with_decorations(true)`。

### 字典文件格式

`BaseDict::load_from_file` 期望格式为 **`汉字 拼音 频率 权重`**（空格分隔），而非 `拼音 汉字 频率 权重`。文件扩展名 `.dict` 会被 DAT 加载器尝试以 mmap 格式打开（`Invalid magic` 警告可忽略）。

## 持续优化
如果发现重复出现的 Bug 或不优雅的模式，请立即更新此文档。
