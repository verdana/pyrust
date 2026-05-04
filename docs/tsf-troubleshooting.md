# pyrust TSF IME 调试记录

## 概述

本文档记录在 Windows 11 上将 pyrust 注册为 TSF (Text Services Framework) 输入法的完整调试过程。

## 环境

- **OS**: Windows 11 25H2 Insider Preview (Build 26300.8346)
- **架构**: x86_64
- **编译器**: Rust + windows-rs 0.58
- **测试环境**: WSL2 (交叉编译) → Windows (原生编译和测试)

## 问题演进

### 阶段 1：DLL 编译与基础注册

**状态**: DLL 编译成功，`regsvr32` 成功，IME 出现在键盘列表中，但切换后立即消失。

**根因**: `ITfTextInputProcessor::Activate` 中缺少事件接收器注册。

**修复**:
- 添加 `ITfSource::AdviseSink` 注册 `ITfThreadMgrEventSink`
- 添加 `ITfKeystrokeMgr::AdviseKeyEventSink` 注册 `ITfKeyEventSink`
- 在 `Deactivate` 中清理已注册的 Sink

**文件**: `crates/tsf/src/tip.rs`

---

### 阶段 2：容错激活 + 诊断日志

**状态**: IME 仍然消失，但 COM 激活日志显示所有步骤成功。

**根因**: `Activate` 中任何非关键步骤失败都通过 `?` 传播错误，导致 TSF 立即停用 TIP。

**修复**:
- 将所有非关键步骤（Sink 注册、Bridge 初始化）改为容错模式——失败只记日志，不返回错误
- 唯一致命步骤：`ITfThreadMgr` 不存在
- 添加文件日志系统（写入 `C:\Users\Verdana\pyrust_tsf.log`），覆盖：
  - `DllMain`, `DllGetClassObject`, `DllRegisterServer`
  - `Activate` 每步 (Source → AdviseSink → KeystrokeMgr → AdviseKeyEventSink → Bridge)
  - `Deactivate`, 所有回调 (`OnKeyDown`, `OnInitDocumentMgr`, `OnSetFocus` 等)

**文件**: `crates/tsf/src/tip.rs`, `dll_exports.rs`, `registry.rs`

---

### 阶段 3：补充 Context Key Event Sink

**状态**: 容错模式下日志全部 OK，但 `ITfKeyEventSink` 和 `ITfContextKeyEventSink` 从未收到按键。

**尝试**: 实现了 `ITfContextKeyEventSink`，在 `OnPushContext` 时安装到 Context 上。

**结果**: ContextKeyEventSink 安装成功（cookie 确认），但仍无按键事件。Windows 11 25H2 的 TextInputHost 不使用传统的 TSF 按键路由路径。

**文件**: `crates/tsf/src/tip.rs`

**状态**: 此问题尚未完全解决，See [剩余问题](#剩余问题)。

---

### 阶段 4：注册表修复（✅ 已解决 —— IME 常驻）

**状态**: IME 仍然从键盘列表中消失。

**根因**: TSF 注册表键结构不完整且格式错误，导致 Windows 无法正确识别和常驻 IME。

**发现的具体问题**:

1. **Category 注册格式错误**
   - 错误格式: `Category\Category\{GUID}` — 空键，无 CLSID 关联
   - 正确格式: `Category\Item\{CAT_GUID}\{CLSID}` — Windows 标准 TSF 格式
   - 影响: TSF 无法确定 TIP 属于哪个功能类别

2. **缺少 LanguageProfile 注册**
   - 缺失: `LanguageProfile\{LangID}\{profile_guid}` 子键
   - 影响: Windows 不知道 IME 与哪种语言关联，无法在语言设置中正确显示

3. **未调用 ITfCategoryMgr COM API**
   - 仅写了原始注册表键，未通过 COM 正式注册
   - 修复: 添加 `ITfCategoryMgr::RegisterCategory` 调用，注册以下类别：
     - `GUID_TFCAT_TIP_KEYBOARD` — 键盘输入法类别（必须）
     - `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT` — Windows 8+ UWP 支持（必须）
     - `GUID_TFCAT_TIPCAP_UIELEMENTENABLED` — 现代候选窗口支持
     - `GUID_TFCAT_TIPCAP_COMLESS` — 现代 TIP 注册

4. **缺少 HKCU 注册**
   - 仅注册了 HKLM（系统全局）
   - Windows 11 也需要 HKCU（当前用户）注册

5. **ActivateProfile 调用失败**
   - `ActivateProfile` 需要有效的 HKL 句柄，而不能传 NULL
   - `RegisterProfile` 已足够，移除了多余的 `ActivateProfile` 调用

**修复后的注册表结构**:
```
HKLM\SOFTWARE\Microsoft\CTF\TIP\{CLSID}
├── Category\Item\
│   ├── {GUID_TFCAT_TIP_KEYBOARD}\{CLSID}
│   ├── {GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT}\{CLSID}
│   └── {GUID_TFCAT_TIPCAP_UIELEMENTENABLED}\{CLSID}
├── LanguageProfile\0x00000804\{profile_guid}
│   ├── (Default) = "Zero Pinyin"
│   ├── Profile = ""
│   ├── Description = "Zero Pinyin IME"
│   ├── IconFile = ""
│   └── IconIndex = "0"
└── Profile\{profile_guid}
    ├── (Default) = "Zero Pinyin"
    └── Description = "Zero Pinyin IME"
```

**文件**: `crates/tsf/src/registry.rs`

---

## 关键教训

1. **TSF 注册表格式极其严格**：`Category\Item\{GUID}\{CLSID}` 格式是必须的，`Category\Category\{GUID}` 不工作
2. **LanguageProfile 是必须的**：没有它 Windows 不知道 TIP 属于哪个语言
3. **COM 注册优先于原始注册表**：`ITfCategoryMgr::RegisterCategory` 比手动写注册表更可靠
4. **HKCU + HKLM 双重注册**：Windows 11 需要两者
5. **GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT 是必须的**：Windows 8+ 的 UWP/现代应用依赖此类别
6. **诊断日志至关重要**：在 DLL 中没有控制台输出，文件日志是唯一调试手段
7. **Activate 必须容错**：任何非关键步骤失败不应阻止激活
8. **交叉编译注意点**：`#![cfg(windows)]` 导致 Linux 下 crate 为空，必须用 `--target` 交叉编译

## 剩余问题

### Explorer 崩溃

**现象**: 切换到 pyrust 输入法时，Windows 任务栏短暂消失后重新出现（Explorer 重启）。

**可能原因**:
- TIP 的 Bridge 初始化在多线程环境下导致 COM 重入
- UI 线程（egui/winit）在 TSF 回调线程中创建窗口导致死锁
- 线程退出时未正确处理 COM 引用计数

**排查方向**:
- 暂不启动 Bridge 线程，测试是否仍然崩溃
- 检查 COM 初始化是否正确（`CoInitializeEx` 参数）
- 使用 WinDbg 附加到 Explorer 进程捕获崩溃堆栈

### 按键不响应

**现象**: 切换到 pyrust 后无法输入任何字符，候选框不出现。

**可能原因**:
- Windows 11 25H2 的 TextInputHost 不使用传统 `ITfKeyEventSink` / `ITfContextKeyEventSink`
- 可能需要实现 `ITfFnKeyDown` 函数提供者接口
- 或使用 `ITfThreadMgr::SetFocus` 主动获取焦点

**排查方向**:
- 在稳定版 Windows 10/11 上测试传统路径是否可用
- 研究 fcitx5-windows / Mozc 等开源项目在 Windows 11 上的按键处理方式

## 构建与测试

```powershell
# Windows 上构建（需管理员）
cd Desktop\pyrust-test\src\crates\tsf
.\..\..\..\build_tsf.bat

# 查看诊断日志
notepad C:\Users\Verdana\pyrust_tsf.log

# 手动注册/卸载
regsvr32 target\release\tsf.dll          # 注册
regsvr32 /u target\release\tsf.dll       # 卸载
```

## 相关文件

| 文件 | 作用 |
|------|------|
| `crates/tsf/src/tip.rs` | TIP COM 实现（Activate/Deactivate/事件回调） |
| `crates/tsf/src/registry.rs` | 注册表写入 + COM Profile/Category 注册 |
| `crates/tsf/src/dll_exports.rs` | DLL 入口 + COM 服务器函数 |
| `crates/tsf/src/display_attrs.rs` | ITfDisplayAttributeProvider 空实现 |
| `crates/tsf/src/bridge.rs` | TSF ↔ pyrust Engine 桥接（线程 + 消息通道） |
| `build_tsf.bat` | Windows 构建脚本（含进程清理 + 自动注册） |

## 版本历史

| 日期 | 阶段 | 状态 |
|------|------|------|
| 2026-05-02 | DLL 编译 + COM 接口实现 | ✅ 完成 |
| 2026-05-02 | Sink 注册 + 容错激活 + 日志 | ✅ 完成 |
| 2026-05-02 | ITfContextKeyEventSink 实现 | ✅ 完成（但无效） |
| 2026-05-03 | 注册表修复（IME 常驻） | ✅ 完成 |
| 2026-05-03 | 按键路由 / Explorer 崩溃 | ⚠️ 待解决 |
