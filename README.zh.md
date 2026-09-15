# Sagaline —— 桌面客户端

开源 AI 视频生成器。Rust + GPUI Kit,Apache 2.0。
[English →](./README.md)

这是桌面引擎。配套的 Web 项目在 `../homeweb/`（TanStack Start + Cloudflare Workers）。

## 设计理念 —— Markdown 作为真相之源

Sagaline 的故事不是数据库里的一行。它是一个 **Markdown 文件目录** ——和你在任意编辑器里打开、用 git 版本管理、打包成 zip 分享、丢给别的工具处理的，是同一批文件。没有私有格式，没有厂商锁定，没有迁移路径要操心。

```text
<story-slug>/
├── story.md                         # 故事入口（id / type / slug / title）
├── bible/<slug>.md                  # 世界观 / 时间线 / 规则 / 设定集
├── characters/<slug>/character.md
├── environments/<slug>/environment.md
├── props/<slug>.md
├── chapters/<NNN-slug>/
│   ├── chapter.md
│   └── scenes/<NNN-slug>.md
└── assets/                          # 生成的图片 / 视频 / 音频
```

每个文件都是 **Markdown + YAML front matter**：

```markdown
---
id: scene_001_laboratory
type: scene
slug: 001-laboratory
characters: [lin-mo, su-yan]
environment: laboratory
props: [energy-core]
---

# 实验室初遇

林默第一次见到陈博士……
```

Front matter 是机器可读的（可校验、可索引）；body 是人类可读的（自由 Markdown）。两半互不干扰。

### 这意味着什么

| 关注点 | 当前（本轮） | 后续阶段 |
|---|---|---|
| **故事内容** | 仅文件系统 —— Markdown + YAML | 永远留在文件里 |
| **生成任务 / 资产 / 向量** | 不涉及 | 可放在 `~/.sageline/data/index.db`（SQLite）。**故事内容不会搬进去** |
| **BYOK 密钥** | 不涉及 | `~/.sageline/data/keys.db`，age 加密，机器绑定 |
| **跨机器同步** | 不涉及 | 可选 —— 与 `homeweb/` 的托管云构建联动 |

规则很简单：**文件系统就是故事本身的数据库。** SQL 只为那些没有叙事意义的运行时状态重新出现 —— 跑着的生成任务、缓存的向量、provider 成本。

### 为什么这是 AI-native

未来在故事上工作的 AI agent，不需要专用 API、也不需要迁移工具。它就开文件：

- `read_file("characters/lin-mo/character.md")`
- `write_file("chapters/001-the-beginning/scenes/002-laboratory.md", ...)`
- `search_files("characters/**/*.md")`

这正是每个 Coding Agent 已经具备的工具集。故事工作区在结构上就是**一个代码工程** —— 同样的形状、同样的操作、同样的可操作性。

## 技术栈

- **语言：** Rust 1.80+
- **UI：** [`gpui-kit`](https://crates.io/crates/gpui-kit) —— 单一门面 crate，重新导出 GPUI + gpui-base + gpui-component + 默认资源。固定 `0.6`。
- **核心数据模型：** 纯 Rust，无数据库。`sagaline-core` 走一遍故事目录，构造一份内存图。

## 项目结构

```text
client/
├── Cargo.toml              workspace 根（resolver = "2"）
├── README.md               英文版
├── README.zh.md            你在这里（中文）
├── AGENTS.md               规约、校验命令、待办清单
└── crates/
    ├── sagaline-core/      故事工作区模型 + 校验
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── story_root.rs    # 路径校验 + story.md 加载
    │       ├── path.rs          # 目录布局 ↔ entity 映射
    │       ├── entity.rs        # EntityType / ParsedEntity / Reference
    │       ├── frontmatter.rs   # 类型化 YAML 访问器
    │       ├── markdown.rs      # front matter / body 切分
    │       ├── schema.rs        # JSON Schema 注册表
    │       ├── graph.rs         # StoryGraph::load + validate
    │       └── error.rs
    ├── sagaline-ui/        gpui-kit 视图层（绑在 core 图上）
    │   ├── Cargo.toml
    │   └── src/lib.rs      # WorkspaceState + WorkspaceView 占位
    └── sagaline/           桌面二进制
        ├── Cargo.toml
        └── src/
            ├── main.rs     # hello-world 占位（下一阶段：开窗口）
            └── lib.rs      # App 占位
```

## 构建 & 运行

```bash
# 在本目录下（client/）
cargo check --workspace         # 全 workspace 类型检查
cargo test  -p sagaline-core    # 跑 36 个 core 测试
cargo build -p sagaline-core    # 链接 core crate

# 启动桌面二进制（当前还是占位）
cargo run -p sagaline
```

`core` 的改动以 `cargo test -p sagaline-core` 通过为完成标志（36 个内联测试覆盖路径分类、Markdown 切分、scene 引用解析、图校验）。

## 下一步（待办）

按优先级排序 —— 完整待办清单见 `AGENTS.md`：

1. **`crates/sagaline-ui/`** —— 接入真实视图：文件树、Markdown 编辑面板、scene 引用预览。
2. **`crates/sagaline/src/main.rs`** —— 把 `println!` 占位换成真正的顶层 `App`，开窗口。
3. **`crates/sagaline-providers/`** —— `trait ModelAdapter`，按 provider 实现（OpenAI、Google Gemini、Kling、Runway、ComfyUI、Ollama）。
4. **BYOK 密钥库** —— 加密存放在 `~/.sageline/data/keys.db`。
5. **托管云同步** —— 可选客户端对接 `homeweb/`。

## 许可证

Apache 2.0。详见顶层 `LICENSE` 或 [`AGENTS.md`](../AGENTS.md) 的「Common constraints」一节。