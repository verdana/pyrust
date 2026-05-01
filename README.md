# inputd

个人拼音输入法 — 追求极致性能、完全隐私可控的跨平台输入法引擎。

## 特性

- **三线程架构**：系统回调、工作逻辑、UI 渲染完全分离，打字永不卡顿
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
| UI 渲染 | egui + winit |
| 配置 | TOML |

## 项目结构

```
inputd/
├── crates/
│   ├── engine-core/      # 拼音引擎核心：状态机、切分、排序
│   ├── dict/             # 词库引擎：mmap DAT 读取、用户词库
│   ├── ui-crate/         # egui 候选词窗口渲染
│   ├── platform-adapter/ # 操作系统 IME 接入层
│   └── yas-config/       # 全局配置管理
├── dict-compiler/        # 离线词库编译工具
├── inputd/               # 二进制入口
├── scripts/              # 辅助脚本（词库生成等）
└── assets/               # 基础数据文件
```

## 构建

```bash
# 编译
cargo build --release

# 运行
cargo run --release

# 编译词库（从 jieba 词表生成）
python3 scripts/convert_jieba.py /path/to/jieba_dict.txt 100000 > assets/words.txt
cargo run -p dict-compiler -- --input assets/words.txt --output base.dict

# 生成上文联想数据
python3 scripts/generate_bigram.py /path/to/jieba_dict.txt bigram.dat
```

### Windows 构建

```powershell
# 运行 build.ps1
.\build.ps1
```

## 使用

### 开发模式

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

### 配置文件

`~/.config/inputd/config.toml` (Linux) / `%APPDATA%\inputd\config.toml` (Windows):

```toml
[general]
mode = "zh"           # zh / en

[engine]
fuzzy_pinyin = false  # 启用模糊音
enable_bigram = true   # 启用上文联想

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
- [x] egui 候选词窗口
- [x] 个人词库（SQLite 持久化）
- [x] 上文联想 (Bigram)
- [x] 模糊音（8 组规则）
- [x] 配置热重载
- [x] 10 万基础词库
- [ ] Windows TSF 系统输入法接入
- [ ] macOS Input Method Kit 接入
- [ ] 集成测试

## 许可证

MIT License
