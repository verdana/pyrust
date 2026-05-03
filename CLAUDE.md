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
- 代码仓库在 `src/` 目录（`/home/verdana/workspace/pyrust/src/`）
- Windows 构建目录：`/mnt/c/Users/Verdana/Desktop/pyrust-test/src/`
- 修改 TSF 代码后，需手动复制到 Windows 目录：`cp crates/tsf/src/*.rs /mnt/c/Users/Verdana/Desktop/pyrust-test/src/crates/tsf/src/`

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

### Explorer 崩溃问题（2026-05-03）

**现象**：切换到 pyrust 输入法时，任务栏消失后重新出现（Explorer 重启）。

**可能原因**：Bridge 线程初始化（egui/winit）在 TSF 回调线程中创建窗口，或 COM 重入导致死锁。

**排查方向**：暂不启动 Bridge 线程测试；使用 WinDbg 捕获崩溃堆栈。

### 按键不响应问题（2026-05-03）

**现象**：IME 已注册且常驻，但切换后无法输入字符，候选框不出现。

**可能原因**：Windows 11 25H2 的 TextInputHost 不使用传统 `ITfKeyEventSink` 路径。

**排查方向**：在稳定版 Windows 测试；研究 fcitx5-windows/Mozc 的按键处理方式。

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
