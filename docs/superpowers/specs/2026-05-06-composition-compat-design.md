# TSF Composition 兼容性修复设计

**日期**: 2026-05-06
**状态**: 待审核
**问题**: 拼音内联显示在不同应用中表现不一致（Windows Terminal、微信等应用中拼音叠加）

---

## 1. 问题描述

### 现象
在微信聊天框、Windows Terminal 等应用中，持续输入拼音时出现叠加：
- 按 `a` → 显示 `a`
- 按 `b` → 显示 `aab`（应为 `ab`）
- 按 `c` → 显示 `aababc`（应为 `abc`）

Notepad/Word 中无此问题。

### 根因
当前实现使用手动 `ITfRange` 跟踪 + `ShiftStart` 展开 range 的方式更新拼音文本：
1. `SetText` 写入文本后 range 折叠为 0 字符
2. `ShiftStart(-N)` 尝试展开 range 覆盖已写入文本
3. 下次 `SetText` 用展开后的 range 替换旧文本

步骤 2 在微信、Windows Terminal 等应用中失败（返回 0），导致 range 始终折叠。步骤 3 变成插入而非替换，产生叠加。

### 参考实现
- **libIME2**（EasyIME/libIME2）：`setCompositionString` 使用 `composition_->GetRange()` + `SetText`，不使用 `ShiftStart`
- **Weasel**（rime/weasel）：`CInlinePreeditEditSession` 使用 `composition_->GetRange()` + `SetText`，不使用 `ShiftStart`

两者的核心模式一致：**依赖 `ITfComposition` 对象管理 range，而非手动跟踪。**

---

## 2. 设计方案

### 核心思路
改为 Weasel 模式：先创建 `ITfComposition`，再通过 `composition.GetRange()` 获取 range 进行文本操作。完全消除对 `ShiftStart` 的依赖。

### 当前流程（有兼容性问题）
```
按键 → GetSelection → Collapse → SetText(拼音) → ShiftStart(-N) → 存储 range
下次按键 → 用存储的 range → SetText(新拼音) → ShiftStart(-N) → 更新 range
```

### 新流程（Weasel 模式）
```
首次按键 → GetSelection → Collapse → StartComposition(sink) → GetRange → SetText(拼音)
后续按键 → GetRange → SetText(新拼音)  // SetText 自动替换 range 内容
提交/清除 → GetRange → SetText(最终文字或空) → EndComposition
```

### 关键变更

#### 2.1 修复 `StartComposition` 调用

**当前代码**（`edit_session.rs:246`）：
```rust
ctx_comp.StartComposition(ec, range, None)  // sink = None → E_INVALIDARG
```

**修复**：传入 `ITfCompositionSink` 实现：
```rust
ctx_comp.StartComposition(ec, range, self.sink.as_ref())  // 传入 sink
```

`PyrustTip` 已实现 `ITfCompositionSink`，只需将 sink 正确传递到 `CompositionEditSession`。

#### 2.2 重构 `CompositionEditSession`

**移除的字段**：
- `preedit_range: Rc<RefCell<Option<ITfRange>>>` — 不再手动跟踪 range

**保留的字段**：
- `context: ITfContext`
- `text: String`
- `end_composition: bool`
- `composition: Rc<RefCell<Option<ITfComposition>>>` — 通过 composition 管理 range
- `sink: Option<ITfCompositionSink>` — 用于 StartComposition

**`DoEditSession` 新逻辑**：

```
1. 如果 composition 不存在且是更新操作：
   a. GetSelection 获取光标 range
   b. Collapse(TF_ANCHOR_START)
   c. StartComposition(ec, range, sink) 创建 composition
   d. 如果 StartComposition 失败，回退到直接 SetText（无 composition 模式）

2. 从 composition.GetRange() 获取 range（或回退时用 selection range）

3. range.SetText(ec, 0, text) 替换内容

4. 如果不是结束：
   a. 应用 display attribute
   b. Collapse(TF_ANCHOR_END) 移动光标到末尾
   c. SetSelection

5. 如果是结束：
   a. SetText(最终文字)
   b. EndComposition
   c. 清除 composition
```

#### 2.3 更新 `PyrustTip` 调用点

**移除**：
- `preedit_range: Rc<RefCell<Option<ITfRange>>>` 字段
- 所有传递 `preedit_range` 的代码

**修改**：
- `handle_keypress` 中创建 `CompositionEditSession` 时，传入 `self.composition` 和 `self`（作为 sink）
- `commit_text` 中，传入 `self.composition`（不再传 `preedit_range`）
- `OnCompositionTerminated` 清除 `composition`（已有）

#### 2.4 回退策略

如果 `StartComposition` 仍然失败（某些极端环境），回退到直接 `SetText` 模式：
- 无下划线（display attribute 无法应用）
- 但仍能正确替换文本（因为每次从 selection 获取新 range）
- 不会出现叠加问题

---

## 3. 影响范围

| 文件 | 变更类型 |
|------|---------|
| `crates/tsf/src/edit_session.rs` | 重构 `CompositionEditSession`，移除 `preedit_range`，修改 `DoEditSession` 逻辑 |
| `crates/tsf/src/tip.rs` | 移除 `preedit_range` 字段，更新所有创建 `CompositionEditSession` 的调用 |
| `crates/tsf/src/bridge.rs` | 无变更 |

### 不受影响的组件
- 引擎核心（engine-core）：不涉及
- 候选窗 UI（ui-crate）：不涉及
- 词库（dict）：不涉及
- 配置（yas-config）：不涉及

---

## 4. 验证计划

### 4.1 编译验证
```bash
cd crates/tsf && cargo check --target x86_64-pc-windows-gnu
```

### 4.2 功能验证（Windows 测试）
1. **Notepad**：输入 `nihao`，确认拼音内联显示、选词后正确替换
2. **Windows Terminal**：同样测试，确认不再叠加
3. **微信聊天框**：同样测试，确认不再叠加
4. **Shift 切换**：中文模式输入拼音后 Shift，确认先上屏再切换
5. **回车上屏**：输入拼音后回车，确认原样上屏
6. **退格删除**：输入拼音后退格，确认逐字删除

### 4.3 回归验证
- 确认 display attribute（下划线）仍然生效（在 StartComposition 成功的应用中）
- 确认候选框位置跟随光标
- 确认中英文标点映射正常

---

## 5. 风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| `StartComposition` 在某些应用中仍失败 | 回退到直接 SetText 模式，保证文本替换正确 |
| `composition.GetRange()` 返回的 range 与预期不符 | 每次操作前验证 range 有效性 |
| `EndComposition` 清除文本失败 | `OnCompositionTerminated` 回调兜底清理 |
