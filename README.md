# pyrust

个人拼音输入法 — 追求极致性能、完全隐私可控的跨平台输入法引擎。

## 特性

- **三线程架构**：系统回调、工作逻辑、UI 渲染完全分离，打字永不卡顿
- **横版候选窗**：现代输入法风格，hover 高亮，点击选词，跟随光标定位
- **完全离线**：零网络依赖，所有词库本地存储，隐私零泄露
- **10 万基础词库**：基于结巴分词词频数据，覆盖日常用词
- **上文联想**：基于 Bigram 模型的上下文候选词加分
- **模糊音支持**：翘舌/平舌、前后鼻音、声母混淆等 8 组规则
- **个人词库**：SQLite 持久化，自动学习用户输入习惯
- **配置热重载**：TOML 配置文件修改即生效
- **跨平台**：支持 Windows (TSF) 和 macOS (Input Method Kit)

## 技术栈

| 层 | 技术 |
|---|------|
| 核心引擎 | Rust |
| 词库存储 | mmap Double-Array Trie (DAT) |
| 个人词库 | SQLite |
| UI 渲染 | Win32 + GDI |
| Windows IME | TSF (Text Services Framework) via windows-rs |
| 配置 | TOML |

## 项目结构

```
pyrust/
├── crates/
│   ├── engine-core/      # 拼音引擎核心：状态机、切分、排序
│   ├── dict/             # 词库引擎：mmap DAT 读取、用户词库
│   ├── ui-crate/         # Win32 + GDI 候选词窗口渲染
│   ├── platform-adapter/ # 操作系统 IME 接入层
│   ├── tsf/              # Windows TSF COM DLL (cdylib)
│   └── yas-config/       # 全局配置管理
├── dict-compiler/        # 离线词库编译工具
├── pyrust/               # 二进制入口 (dev EXE)
├── scripts/              # 辅助脚本（词库生成等）
└── assets/               # 基础数据文件
```

## 构建

### 开发模式 (跨平台)

```bash
cargo build --release
cargo run --release
```

### Windows TSF 输入法 DLL

```powershell
# 在 Windows 上
cd crates\tsf
cargo build --release

# 注册（管理员）
regsvr32 target\release\tsf.dll

# 卸载
regsvr32 /u target\release\tsf.dll
```

### 交叉编译 Windows EXE (从 Linux/WSL)

```bash
rustup target add x86_64-pc-windows-gnu
# Arch: sudo pacman -S mingw-w64-gcc

cargo build --release --target x86_64-pc-windows-gnu
```

### 词库编译

```bash
python3 scripts/convert_jieba.py /path/to/jieba_dict.txt 100000 > assets/words.txt
cargo run -p dict-compiler -- --input assets/words.txt --output base.dict
python3 scripts/generate_bigram.py /path/to/jieba_dict.txt bigram.dat
```

## 使用

### 开发模式 (dev_input_loop)

```bash
cargo run --release
```

```
> nihao          # 输入拼音，候选词窗口弹出
> 1              # 选择第 1 个候选词
> reset          # 清空状态
> zh / en        # 切换中英文
> q              # 退出
```

### 系统输入法 (Windows TSF)

1. 以管理员运行 `regsvr32 tsf.dll` 注册
2. Windows 设置 → 时间和语言 → 语言 → 添加键盘 → pyrust Pinyin
3. Win+Space 切换到 pyrust，在任意应用中输入

## 配置文件

`~/.config/pyrust/config.toml` (Linux) / `%APPDATA%\pyrust\config.toml` (Windows):

```toml
[general]
mode = "zh"

[engine]
fuzzy_pinyin = false
enable_bigram = true

[ui]
font_size = 18
font_family = ""
theme = "auto"         # light / dark / auto
max_candidates = 5

[dict]
base_dict_path = "base.dict"
user_dict_path = "user.db"
bigram_data_path = "bigram.dat"
```

## 词库

基础词库来自[结巴分词](https://github.com/fxsjy/jieba)词频数据，选取前 10 万高频词，通过 [pypinyin](https://github.com/mozillazg/python-pinyin) 转换为拼音后编译为 mmap Double-Array Trie 格式。

- **格式**：汉字 + 拼音 + 词频 + 权重
- **存储**：mmap 零拷贝加载，支持百万级词条
- **体积**：约 5MB（10 万词）

## 开发状态

- [x] 拼音引擎核心（状态机、音节切分、候选排序）
- [x] mmap 词库（DAT 编译与加载）
- [x] Win32 + GDI 候选词窗口（横版、hover、点击选词、跟随光标）
- [x] 个人词库（SQLite 持久化）
- [x] 上文联想 (Bigram)
- [x] 模糊音（8 组规则）
- [x] 配置热重载
- [x] 10 万基础词库
- [x] Windows TSF — DLL 编译 + COM 接口实现（ITfTextInputProcessorEx、ITfKeyEventSink 等 7 个接口）
- [x] Windows TSF — Sink 注册（ITfThreadMgrEventSink + ITfKeyEventSink + ITfContextKeyEventSink + ITfThreadFocusSink）
- [x] Windows TSF — 注册表修复（IME 可常驻 Windows 键盘列表）
- [x] Windows TSF — Explorer 崩溃修复（延迟 UI 线程初始化）
- [x] Windows TSF — 键盘 Compartment + 文本提交（ITfRange::SetText）
- [x] Win32 + GDI 候选框（替换 egui，解决 DLL 环境 OpenGL 问题）
- [x] 候选框跟随光标（TSF ITfContextView::GetTextExt）
- [x] 贪心拼音切分 + 多字候选词回退
- [ ] Windows TSF — 按键路由验证（Windows 11 25H2 兼容性）
- [ ] macOS Input Method Kit 接入
- [ ] 集成测试

### TSF 调试记录

详细调试过程和解决方案见 `docs/tsf-troubleshooting.md`。

## 许可证

MIT License
