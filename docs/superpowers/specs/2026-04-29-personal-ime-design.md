# 个人拼音输入法 — 设计文档

## 概述

自研 Windows + macOS 拼音输入法，核心目标：词库完全可控、无动态/AI 干扰、隐私零泄露、用户体验清爽。

## 技术栈

| 层 | 技术 |
|---|------|
| 核心引擎 | Rust |
| Windows IME 接入 | `windows-rs` (TSF) |
| macOS IME 接入 | `objc2` (Input Method Kit) |
| UI 渲染 | `egui` (候选栏，自绘无系统控件依赖) |
| 配置格式 | TOML |
| 构建 | `cargo workspace`，每个平台各自编译所需 crate |

---

## 1. 整体架构

```
┌───────────────────────────────────────────────────────┐
│                   pyrust 进程                          │
│                                                       │
│  ┌───────── 系统回调线程 (platform thread) ─────────┐ │
│  │  platform-adapter                                 │ │
│  │   ┌──────────┐  ┌─────────────┐                   │ │
│  │   │win-adapte│  │ mac-adapter │                   │ │
│  │   │(TSF)    │  │ (IMK)      │                   │ │
│  │   └────┬─────┘  └──────┬──────┘                   │ │
│  │        └──────┬────────┘                          │ │
│  │               ▼                                    │ │
│  │  ┌──────────────────────┐                         │ │
│  │  │  channel: Tx<Request> │                         │ │
│  │  └──────────────────────┘                         │ │
│  └───────────────────────────────────────────────────┘ │
│                          │                              │
│                 ┌────────▼────────┐                    │
│                 │   mpsc channel   │                    │
│                 └────────┬────────┘                    │
│                          │                              │
│  ┌────── 工作线程 (worker thread) ──────┐              │
│  │  ┌──────────────┐   ┌────────────┐  │              │
│  │  │  engine-core  │   │   dict     │  │              │
│  │  │  拼音引擎     │◄──┤  词库引擎   │  │              │
│  │  └──────┬───────┘   └────────────┘  │              │
│  └─────────┼───────────────────────────┘              │
│            │                                            │
│  ┌─────────▼───────────────────────────────────┐      │
│  │            UI 渲染线程 (ui thread)            │      │
│  │  ┌──────────────────────────────┐            │      │
│  │  │  ui-crate (egui 候选栏)       │            │      │
│  │  │  独立窗口，单独的事件循环       │            │      │
│  │  └──────────────────────────────┘            │      │
│  └──────────────────────────────────────────────┘      │
│                                                       │
│  ┌─────────────┐                                      │
│  │ yas-config  │  ← 所有线程只读引用 (Arc<Config>)    │
│  └─────────────┘                                      │
└───────────────────────────────────────────────────────┘
```

### 线程策略 (Thread Strategy)

| 线程 | 职责 | 约束 |
|------|------|------|
| **系统回调线程**（platform thread） | 接收 OS 按键事件，通过 **同步 oneshot channel** 发送请求并**阻塞等待** `ImmediateResponse`，然后立即返回给 OS | **必须返回 bool**（consumed / passthrough）。阻塞等待时间必须 < 1ms。egui 渲染、SQLite 写入、词库构建等操作绝对不在此线程执行 |
| **工作线程**（worker thread） | 引擎核心逻辑：拼音切分 → 词库查询 → 候选排序 → 更新个人词库 | 单线程事件驱动，收到的 Request 按序处理。**保证 handle_key 耗时 < 1ms**（无磁盘 IO、无锁争用、无内存分配）。结果通过 `ImmediateResponse` 同步返回给平台线程，通过独立 channel 异步发给 UI 线程 |
| **UI 渲染线程**（ui thread） | egui 事件循环，管理候选栏窗口 | 可独立卡顿而不影响打字。egui 初始化和帧渲染全在这个线程，即使 GPU 驱动异常也不会阻塞输入。所有 UI 更新都是单向的（fire-and-forget） |

**关键承认：系统回调线程必须同步阻塞等待工作线程。** Windows TSF 和 macOS IMK 的 `handle_key_event` 回调要求同步返回 boolean（consumed / passthrough）。操作系统不会等你的异步结果。因此：
- 平台线程和工作线程之间使用 **无锁 oneshot channel**（如 `crossbeam::sync::WaitGroup` 或 `tokio::sync::oneshot`）进行同步等待
- "不阻塞"的约束改为 **"阻塞时间 < 1ms"**，超出此阈值应打警告日志
- 工作线程必须保证 handle_key 内无任何 IO（词库查 mmap 内存、LRU 缓存命中、不写磁盘、不分配堆内存）

### 线程间通信

```rust
// 系统回调线程 → 工作线程（同步，阻塞等待结果）
pub enum Request {
    KeyPress { vk: u32, modifiers: Modifiers },
    SelectCandidate(usize),
    MoveCursor(isize),            // 方向键移动拼音光标
    Reset,
}

// 工作线程 → 系统回调线程（同步，通过 oneshot channel）
pub enum ImmediateResponse {
    Consumed,                     // 输入法已消费此按键，OS 不处理
    Passthrough,                  // 透传给应用程序
}

// 工作线程 → UI 渲染线程（异步，fire-and-forget）
pub struct UiUpdate {
    pub candidates: Vec<Candidate>,
    pub pinyin: String,
    pub cursor_position: usize,   // 拼音缓冲区中的光标位置
    pub position: (i32, i32),
    pub visible: bool,
}
```

### 数据流

```
按键事件 → 系统回调线程 (platform thread)
  → oneshot channel 同步发送 Request，阻塞等待
  → 工作线程 (worker thread) 处理 engine-core 逻辑:
      拼音状态机 → 拼音切分 → dict 内存查询候选 → 排序
  → ImmediateResponse 同步返回系统回调线程
  → 系统回调线程立即回复 OS consumed / passthrough
  → 工作线程异步发 UiUpdate 给 UI 线程更新候选栏
  → 用户选词 → 同样路径 → 提交上屏
  → 引擎更新个人词库频率 → 批处理队列（30s 间隔写磁盘）
```

### crate 依赖关系

```
pyrust (bin)
├── engine-core
├── dict
│   └── yas-config (词库路径配置)
├── platform-adapter
│   ├── win-adapter (cfg(windows))
│   └── mac-adapter (cfg(macos))
├── ui-crate
└── yas-config
```

---

## 2. engine-core — 拼音引擎

### 职责

- 维护拼音输入状态机
- 拼音字符串切分（全拼 + 双拼支持预留）
- 调用 dict 获取候选结果
- 候选排序逻辑

### 状态机

```
                ┌─────────┐
    ┌──────────►│  Idle   │◄─────────── 提交/取消/全透传
    │           └────┬────┘
    │    按键(a-z)   │                      ← 方向键可在此
    │                ▼                       状态左右移动光标
    │           ┌─────────┐
    │           │ Pending │◄────── 继续输入/← →
    │           └────┬────┘
    │     空格/数字   │
    │                ▼
    │           ┌─────────┐
    │           │Composing│
    │           └─────────┘
    │                │
    └──── 继续输入 ──┘
```

- **Idle**: 等待输入，英文透传
- **Pending**: 拼音缓冲区非空，显示候选。**←/→ 方向键在缓冲区中移动光标**，用户可以在拼音串中间插入或删除音节
- **Composing**: 候选词已选定，等待下一个词（支持长句连续输入）

### 拼音缓冲区（PinyinBuffer）

```rust
pub struct PinyinBuffer {
    raw_input: String,              // 用户原始输入，如 "jiandanzhijie"
    cursor_position: usize,         // 插入光标在 raw_input 中的位置（字节偏移）
    syllables: Vec<(usize, usize)>, // 每个音节的 [start, end) 范围，指向 raw_input
}

impl PinyinBuffer {
    /// 追加字符到光标位置
    pub fn insert_at_cursor(&mut self, ch: char);

    /// 在光标位置删除（Backspace）
    pub fn delete_before_cursor(&mut self);

    /// 移动光标（← → 方向键）
    pub fn move_cursor(&mut self, delta: isize);

    /// 获取光标前的拼音串
    pub fn before_cursor(&self) -> &str;

    /// 获取光标后的拼音串
    pub fn after_cursor(&self) -> &str;
}
```

### 核心接口

```rust
pub struct EngineCore {
    pinyin_buffer: PinyinBuffer,
    dict: Arc<Dict>,
    config: Arc<Config>,
}

impl EngineCore {
    /// 处理按键，返回输入法动作
    pub fn handle_key(&mut self, key: KeyEvent) -> Action;

    /// 获取当前候选列表
    pub fn candidates(&self) -> &[Candidate];

    /// 选择候选（数字键或鼠标点击）
    pub fn select_candidate(&mut self, index: usize) -> Action;

    /// 清除当前输入
    pub fn reset(&mut self);
}

pub enum Action {
    /// 透传按键（英文模式或无效按键）
    Passthrough(KeyEvent),
    /// 提交文本上屏
    Commit(String),
    /// 更新候选栏（无提交）
    UpdateCandidates,
    /// 无操作
    Noop,
}
```

### 拼音切分算法

采用 **全切分 + 动态规划** 选择最优路径。

#### 切分策略

- 建立拼音音节表（约 400+ 个标准拼音，含 `a, o, e, ... , zhuang`）
- 将音节表组织为 Trie 树，支持前缀匹配
- 对用户输入字符串生成**所有可能的切分路径**（有向无环图 DAG）

#### 最优路径选择（DP / Viterbi）

对 DAG 中的每条边（即每个切分出来的音节）赋予权重，用动态规划求最短/最优路径：

```
f[i] = min_{j < i, pinyin[j..i] ∈ lexicon} (f[j] + cost(pinyin[j..i]))
```

- **基线原则**：音节数越少越优先（即 "xian" 优先作为一个音节而非 "xi" + "an"）
- **优先级**：完整音节 > 零声母拆分
- 歧义消解示例：`xian` → 优先 `/xian/`，而非 `/xi/an/`；`fangan` → 优先 `/fang/an/`，而非 `/fan/gan/`（通过词库验证 + 音节数最少原则自动选择）
- MVP 阶段实现上述 DP + 最大匹配原则即可，**不引入语言模型消歧**

```rust
impl PinyinSyllabler {
    /// 全切分，返回所有合法切分路径
    pub fn all_segmentations(&self, input: &str) -> Vec<Vec<&str>>;

    /// 选择最优路径（DP + 音节数最少 + 词频加权）
    pub fn best_segmentation(&self, input: &str) -> Vec<&str>;

    /// 获取当前缓冲区的所有切分支
    pub fn ambiguous_syllables(&self) -> Vec<Vec<Syllable>>;
}
```

### 候选排序规则

1. **精确命中**：用户词库精确匹配 > 基础词库精确匹配 > 拼音候选
2. **频率排序**：同一层级内按使用频率降序
3. **新鲜度提升**：最近 7 天使用过的词临时提高权重（+30%）
4. **上文关联**：若处于 Composing 状态，参考前一个已选词做 bigram 排序

---

## 3. dict — 词库引擎

### 数据架构

三个词库分层，从上到下优先级递减：

```
┌────────────────────────┐
│   个人词库 (user.dict)  │  ← 读写，用户打字自动积累
│   Trie 树 + SQLite      │
├────────────────────────┤
│   基础词库 (base.dict)   │  ← 只读，随发行版提供
│   开源现代汉语词表       │
├────────────────────────┤
│   拼音字典 (pinyin.db)   │  ← 只读，汉字→拼音映射
│   单字 + 多音字          │
└────────────────────────┘
```

### 词条结构

```rust
pub struct DictEntry {
    pub text: String,           // 词文本，如 "输入法"
    pub pinyin: Vec<String>,    // 拼音，如 ["shu1", "ru4", "fa3"]
    pub frequency: u32,         // 使用频率
    pub weight: i32,            // 静态权重（稀有词负值，高频词正值）
    pub is_user: bool,          // 是否来自个人词库
    pub updated_at: u64,        // 时间戳（用于新鲜度提升）
}
```

### 个人词库 (SQLite + 内存缓存)

```sql
CREATE TABLE user_dict (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL UNIQUE,
    pinyin TEXT NOT NULL,       -- JSON 数组 ["shi4","ru4","fa3"]
    frequency INTEGER DEFAULT 0,
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch())
);

CREATE INDEX idx_user_dict_text ON user_dict(text);
```

**性能关键：不直接查 SQLite。** 每敲一个字母就查数据库是不可接受的。策略：

1. **启动时全量加载入内存**：个人词库规模通常 < 10 万条，全量加载到内存 Trie 树中，以 MB 计，亚毫秒级查询
2. **LRU 热词缓存**：最近 1000 个查询结果缓存在 `HashMap<(String, Vec<String>), Vec<DictEntry>>` 中
3. **写入批处理**：频率更新先缓存在内存中，每 30 秒或累积 50 条变更后批量写入 SQLite，避免高频磁盘 IO
4. **SQLite 仅作为持久化层**：用于进程重启后的数据恢复，运行时完全不依赖

```rust
pub struct UserDict {
    mem_trie: Trie<DictEntry>,           // 内存主副本，查询全走这里
    query_cache: LruCache<QueryKey, Vec<DictEntry>>, // LRU 热词缓存
    pending_writes: Vec<PendingWrite>,   // 待写入的变更
    db: Option<Connection>,              // SQLite 连接（仅写入时打开）
}
```

自动学习规则：
- 用户完整上屏一个词 → 频率 +1
- 用户从候选中选择 → 频率 +2
- 用户删除候选词 → 频率归零或移除条目
- 写入批处理：每 30 秒或累积 50 条变更→合并写入 SQLite

### 基础词库 (mmap Double-Array Trie)

来源：使用开源词库（如 `pinyin-data`、`chinese-xinhua`、`Thunlp/THUOCL`）

预处理：使用独立的 **dict-compiler** 工具将原始词表转为二进制 mmap 就绪格式。

**数据结构关键选择：指针地址不可写死到 mmap。**

Rust 的 `Box`/`&`/`*const` 在进程地址空间中指向特定内存位置。不能将包含这些指针的数据结构直接写入文件再 mmap，因为下一次加载时基地址会变，所有指针都会失效。

**解决方案：Double-Array Trie (DAT) + 定长 u32 offset**

```rust
// 在 mmap 区域内，所有"指针"都是相对于文件头部的 u32 偏移量
// 这是一个完全扁平化的数据结构，无任何地址依赖
#[repr(C)]
struct MmapTrieHeader {
    magic: u32,                  // 文件魔数，格式校验
    version: u32,                // 版本号
    base_offset: u32,            // base 数组起始偏移
    check_offset: u32,           // check 数组起始偏移
    tail_offset: u32,            // tail 数组起始偏移
    entry_offset: u32,           // DictEntry 数组起始偏移
    node_count: u32,             // 节点数
    entry_count: u32,            // 词条数
}

impl BaseDict {
    // mmap 映射后，所有访问通过 offset + base_ptr 计算
    pub fn load(path: &Path) -> Result<Self> {
        let mmap = unsafe { Mmap::map(&File::open(path)?)? };
        let ptr = mmap.as_ptr();
        // 偏移量计算: *(ptr + offset as usize) 而非 *(ptr + delta(ptr))
        // 编译器不会在这里做重定位
    }
}
```

- **Double-Array Trie** 是经典的高效 Trie 实现，只有两个数组（base 和 check），完全由 u32 offset 寻址，无指针
- 查询耗时 O(词长)，约 50-200ns
- mmap 由操作系统管理 page cache，不占用进程 RSS
- 词条规模：约 50 万 ~ 100 万词

dict-compiler 是一个独立的二进制 crate，不参与运行时。职责：
- 从原始文本词表（每行: `词 拼音 频率`）解析词条
- 构建 Trie 树 → 序列化为紧凑二进制格式
- 输出 `base.dict` 和 `pinyin.db` 两个文件
- 支持增量编译（只处理有变动的源文件）

```bash
# 使用方式
cargo run -p dict-compiler -- \
    --input assets/source/cedict.txt \
    --input assets/source/thuocl.txt \
    --output assets/base.dict \
    --pinyin-output assets/pinyin.db
```

```rust
// 基础词库在磁盘上已序列化为紧凑的 Trie 树
// 查询时直接 mmap 映射到虚拟内存，零拷贝
pub struct BaseDict {
    mmap: Mmap,                         // 内存映射文件
    trie: MmapTrie,                     // 在 mmap 区域上直接操作的 Trie 树
}
```

- 查询不产生任何堆分配，直接在 mmap 区域遍历节点
- 词条规模：约 50 万 ~ 100 万词
- mmap 由操作系统管理 page cache，不占用进程 RSS

---

## 4. platform-adapter — 平台适配层

### ImeBackend trait

```rust
#[cfg_attr(windows, path = "win/adapter.rs")]
#[cfg_attr(macos, path = "mac/adapter.rs")]
mod platform;

pub trait ImeBackend: Send {
    /// 初始化 IME（注册窗口、建立通信）
    fn initialize(&mut self) -> Result<(), ImeError>;
    /// 处理系统 key event
    fn handle_key_event(&mut self, vk: u32, modifiers: Modifiers) -> Action;
    /// 显示候选栏
    fn show_ui(&self, candidates: &[Candidate], position: (i32, i32));
    /// 隐藏候选栏
    fn hide_ui(&self);
    /// 提交文字上屏
    fn commit(&self, text: &str);
    /// 设置候选栏位置（跟随光标）
    fn set_candidate_position(&self, x: i32, y: i32);
}

pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}
```

### Windows 实现 (TSF)

**核心接口**：实现 `ITfTextInputProcessorEx`、`ITfKeyEventSink`、`ITfDisplayAttributeProvider`

**光标跟随**：
- 通过 `ITfContext::GetSelection` 获取当前选区位置
- 调用 `ITfContext::GetTextExt` 将文档坐标转换为屏幕坐标
- 两种降级策略：若选区为空则取 `ITfInsertAtSelection` 的位置；若全部失败则固定屏幕左下角

**Composition (编码区/预编辑区)**：
- 输入过程中，在编辑器中显示带下划线的未确认拼音字符串
- 使用 `ITfComposition` 和 `ITfRange` 管理编码区
- 通过 `ITfDisplayAttributeProvider` 设置预编辑文本样式（下划线 + 灰色背景）
- 用户确认后提交 `ITfCommit`

**候选栏窗口**：
- 独立顶层窗口，样式：`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT`
- 使用 `UpdateLayeredWindow` 实现透明背景
- 窗口焦点：`WS_EX_NOACTIVATE` 保证候选栏弹出时不抢焦点

### macOS 实现 (Input Method Kit)

**核心接口**：创建 `IMKInputController` 子类，实现 `inputText:client:`、`commitComposition:`、`insertText:replacementRange:` 等方法

**光标跟随**：
- 通过 `[client selectedRange]` 获取当前选区
- 调用 `[client firstRectForCharacterRange:]` 获取屏幕坐标
- 降级策略：获取失败时使用 NSEvent 的鼠标位置 + 固定偏移

**Composition (编码区/预编辑区)**：
- 输入未确认时，调用 `[client setMarkedText:selectionRange:replacementRange:]` 在编辑器内显示带下划线的编码区
- 确认后调用 `[client insertText:replacementRange:]` 提交

**候选栏窗口 (NSPanel)**：
- 使用 `NSPanel` 并设置 `styleMask: .nonactivatingPanel`、`level: .floating`
- `NSPanel` 处理非常繁琐，特别注意：
  - **无焦点击**：候选栏不能抢走编辑器的焦点。`setBecomesKeyOnlyIfNeeded: true`
  - **层级**：`NSWindowLevel.floating`，但不能盖过全屏应用。需监听 `NSWindowDidChangeOcclusionStateNotification` 动态调整
  - **跟随光标**：每次显示前重新计算位置，应对滚动、窗口移动等场景
- **Accessibility 兜底**：当 `client` 不支持 `firstRectForCharacterRange:` 时（常见于某些 Electron 应用），降级使用 `AXUIElementCopyAttributeValue` 获取光标位置

---

## 5. ui-crate — 候选栏

### 技术选择

使用 `egui` 自绘候选栏窗口，理由：
- Rust 原生，跨平台一致渲染
- 不依赖系统 UI 控件
- 易于自定义样式（字体、颜色、圆角、暗黑模式）

### 候选栏样式

```
┌─────────────────────────────────┐
│ 1. 输入法  2. 输入  3. 舒服  4. 殊  │
│ shu1 ru4 fa3                   │
│                   [下一页 ▸]     │
└─────────────────────────────────┘
```

- 第一行：候选词（数字键 1-9）
- 第二行：当前拼音串
- 翻页：+/- 键或 > 按钮
- 跟随光标位置
- 支持多行候选（最多 3 行）

### 接口

```rust
pub struct CandidateUI {
    pub visible: bool,
    pub candidates: Vec<Candidate>,
    pub pinyin: String,
    pub page: usize,
    pub position: (i32, i32),   // 屏幕坐标
}

impl CandidateUI {
    pub fn new(egui_ctx: Context) -> Self;
    pub fn show(&mut self);
    pub fn set_position(&mut self, x: i32, y: i32);
    pub fn hide(&mut self);
}
```

### 配置项（样式相关）

```toml
[ui]
font_size = 18
font_family = "Microsoft YaHei"   # Windows
# font_family = "PingFang SC"     # macOS
theme = "light"                   # light / dark / auto
opacity = 0.95
max_candidates = 5
show_pinyin = true
```

---

## 6. yas-config — 配置管理

### 配置文件位置

| 平台 | 路径 |
|------|------|
| Windows | `%APPDATA%/pyrust/config.toml` |
| macOS | `~/Library/Application Support/pyrust/config.toml` |
| Linux(预留) | `~/.config/pyrust/config.toml` |

### 配置结构

```toml
[general]
mode = "zh"                    # zh / en 默认中/英文
switch_key = "Shift"           # 中英切换键
candidate_key = "Space"        # 上屏键（Space / 数字）

[engine]
fuzzy_pinyin = false           # 模糊音
enable_bigram = true           # 开启上文联想
personal_learning = true       # 开启个人词库学习

[dict]
base_dict_path = "base.dict"
user_dict_path = "user.db"
auto_learn = true
max_user_dict_size = 100000    # 个人词库上限

[ui]
font_size = 18
font_family = ""
theme = "auto"
max_candidates = 5
vertical = false               # 候选排列方向
```

```rust
pub struct Config {
    pub general: GeneralConfig,
    pub engine: EngineConfig,
    pub dict: DictConfig,
    pub ui: UiConfig,
}

impl Config {
    pub fn load() -> Self;              // 从默认路径加载
    pub fn save(&self);                 // 保存配置
    pub fn hot_reload(&mut self);       // 监听文件变更热重载
}
```

配置变更热重载：使用 `notify` crate 监听配置文件变更，自动重新加载。

---

## 7. 错误处理策略

| 错误类型 | 处理方式 |
|---------|---------|
| 词库文件损坏 | 自动重建/恢复到出厂词库，日志告警 |
| IME 注册失败 | 弹出错误提示，退出进程 |
| UI 渲染异常 | 关闭候选栏，退化为无 UI 模式（键入即上屏） |
| 配置解析错误 | 使用默认配置，日志记录错误位置 |
| 个人词库写入失败 | 降级为只读，日志告警 |

---

## 8. MVP 范围与迭代计划

### Phase 1 — 核心引擎可打字 (预计 2-3 周)

- `engine-core`：拼音切分 + 单字/双字词候选 + 空格上屏
- `dict`：基础词库（开源数据子集，约 10 万词）+ 拼音字典
- `yas-config`：基础配置加载
- 一个平台上跑通（建议先 macOS，IMKit 比 TSF 简单）
- 候选栏能用 egui 显示基本信息

**验证标准**：能在文本编辑器中打字，候选正确，选词上屏正常。

### Phase 2 — 个人词库 + 体验打磨

- 个人词库 SQLite 存储 + 自动学习
- 词频排序、新鲜度提升
- 候选栏样式完善
- 中英切换热键
- 双拼支持

### Phase 3 — 跨平台 + 完善

- 移植到第二个平台
- TSF Windows 完整实现
- 模糊音、多音字优化
- bigram 上文联想
- 配置修改热重载
- 基础词库扩充至 50 万+

### Phase 4 — 进阶功能（可选）

- 云词库同步（端到端加密）
- 自定义短语/快捷码
- 皮肤主题
- 输入统计

---

## 9. 隐私设计

### 编译级网络隔离

```toml
# Cargo.toml — 通过 feature flags 确保无网络
[features]
default = ["local-only"]
local-only = []

# 所有网络相关依赖 (reqwest, hyper 等) 完全不加入依赖树
# cargo build --no-default-features 应编译失败或明确警告
```

- 在 `build.rs` 中检查：如果启用了非 `local-only` 的 feature，打印编译警告
- CI 中增加 `cargo check --no-default-features` 确保不意外引入网络依赖
- 如果需要在 pyrust 和外部工具（如 dict-compiler）间共享代码，网络相关代码用 `#[cfg(feature = "net")]` 隔离

### 密码框安全

输入法必须正确处理密码场景：

```rust
impl platform_adapter {
    fn handle_key_event(&mut self, vk: u32, modifiers: Modifiers) -> Action {
        // 在 Windows 上: 通过 TSF 的 ITfContext::GetInputScope 检测 IS_PASSWORD
        // 在 macOS 上: 通过 IMK 的 [client inputSourceIdentifier] 或
        //              NSTextInputClient.attributedSubstringForProposedRange 推断
        if self.is_password_field {
            // 密码框下：强制切换到 English Passthrough
            // 不记录任何按键日志
            // 不上屏拼音候选
            return Action::Passthrough(KeyEvent::new(vk, modifiers));
        }
        // 正常处理...
    }
}
```

- **不记录进入密码框的时间、频率、按键序列**
- **密码框检测失败时（安全兜底）**：
  - macOS 上：依赖 `NSTextInputClient` 的 `SecureEventInput` 系统通知，该通知由系统 Security Agent 精确管理，非常可靠
  - Windows 上：依赖 TSF 的 `ITfContext::GetInputScope → IS_PASSWORD`，大多数现代应用都会正确报告
  - 当系统**未明确报告** IS_PASSWORD 时，**默认当作普通输入框**（而非密码框）。这是为了覆盖游戏引擎（Unity/UE）、终端模拟器、老旧应用等不实现 Context 的场景
  - 安全性由操作系统的安全输入机制保证（如 macOS Secure Event Input 自动禁止其他进程读取按键），输入法层面不做过度保护
- **核心约束**：在任何模式下，如果无法 100% 确定当前是普通输入框，则**禁止将按键序列记录到个人词库**。安全在操作系统层保障，隐私（不学习）在输入法层保障

### 其他隐私承诺

- **纯本地**：不需要网络权限，无任何网络请求
- **个人词库仅存本地**：SQLite 文件在用户目录，不外传
- **无遥测、无崩溃上报**：不包含任何第三方 SDK
- **开源**：全部代码可审查

---

## 10. 项目结构

```
pyrust/
├── Cargo.toml                 # workspace 根
├── crates/
│   ├── engine-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pinyin.rs       # 拼音切分
│   │       ├── state_machine.rs # 状态机
│   │       └── sorter.rs       # 候选排序
│   ├── dict/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── trie.rs         # Trie 树
│   │       ├── base_dict.rs
│   │       ├── user_dict.rs    # SQLite 个人词库
│   │       └── pinyin_table.rs # 拼音→汉字索引
│   ├── platform-adapter/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs          # ImeBackend trait
│   │       ├── win/
│   │       │   └── adapter.rs
│   │       └── mac/
│   │           └── adapter.rs
│   ├── ui-crate/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── candidate_window.rs
│   │       └── theme.rs
│   └── yas-config/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           └── types.rs
├── dict-compiler/
│   ├── Cargo.toml             # 独立 bin，不参与运行时
│   └── src/
│       ├── main.rs
│       ├── parser.rs          # 解析原始词表
│       └── serialize.rs       # 序列化为 mmap-ready 二进制 Trie
├── pyrust/
│   ├── Cargo.toml             # 二进制入口
│   └── src/
│       ├── main.rs
│       └── logger.rs
├── assets/
│   ├── source/                # 原始词表文件（git LFS）
│   │   ├── cedict.txt
│   │   └── thuocl.txt
│   ├── base.dict              # 编译后的 mmap 基础词库
│   └── pinyin.db
└── scripts/
    ├── build-dict.sh          # 调用 dict-compiler 构建词库
    └── install.sh             # 安装脚本
```
