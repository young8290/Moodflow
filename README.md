<p align="center">
  <img src="assets/logo.png" alt="Moodflow" width="128">
</p>

<h1 align="center">Moodflow</h1>

<p align="center">
  懂你心情的 AI 输入陪伴：一个 Windows 中文输入法，加上一位陪你打字的伙伴“晴晴”。
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11-brightgreen" alt="Platform">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

## Moodflow 是什么

Moodflow 由两部分组成：

- **Moodflow 输入法**：五笔、拼音、五笔拼音混输都能用的中文输入法，在系统语言栏里叫“Moodflow 输入法”。
  打字的同时，它只把节奏类的统计（速度、停顿、删改的多少，不含你打的字）交给 Hub。
- **Moodflow Hub**：桌面上的陪伴应用。晴晴以桌宠的样子站在桌面上，按你的打字节奏和心情换天气、换动作；
  需要的时候陪你聊几句、写一封信、提醒你歇一歇，也能帮你润色一句话。

所有数据默认只存在本机。第一次打开时会说明会记录什么、不会记录什么，并要你同意后才开始。
隐私说明全文在 Hub 的“设置 → 隐私”里，也在 [hub_templates/privacy_notice.md](hub_templates/privacy_notice.md)。

## 功能

- **输入法**：五笔 86、全拼、双拼、五笔拼音混输；方案文件驱动，亮暗主题随系统切换。
- **心情感知**：根据打字节奏推断当下的状态，用天气表示（晴、风、云、雨、雷、夜）；可随时暂停，暂停期间什么都不记录。
- **桌宠**：内置晴晴，也能导入 Codex / petdex 格式的桌宠；可以拖、扔，会沿屏幕底边散步。
- **陪伴**：聊天、信件、专注时段与专注计时、适时的小提醒；连续情绪低落时给出求助入口。
- **改写**：在任意输入框里选中一句话，按快捷键让 AI 换个说法（第一次使用前会征得同意）。
- **数据可控**：一键导出、导入备份，一键删除全部数据；撤回同意时同时删掉心情数据。

## 安装与运行

目前没有公开的安装包，需要从源码构建（Windows 10/11，64 位）：

```powershell
.\scripts\dev.ps1          # 打开菜单：构建、部署、运行
.\scripts\dev.ps1 p1       # 构建并部署开发版
```

部署后按 `Win + Space` 切换到 “Moodflow 输入法（开发版）”，Hub 会随输入法一起启动。

## 仓库结构

| 目录 | 技术 | 内容 |
|---|---|---|
| `moodflow_input/` | Rust | 输入法核心服务：引擎、词库、候选、界面渲染、IPC |
| `moodflow_tsf/` | C++ | Windows TSF 接口，捕获键盘事件 |
| `moodflow_hub/` | Tauri（Rust + Vue） | Hub 桌面应用：桌宠、聊天、看板、设置 |
| `crates/` | Rust | Hub 与输入法共用的协议与工具，如 MFP 协议（`crates/mfp`） |
| `hub_templates/` | TOML / Markdown | 界面文案、提示语、隐私说明等内容资产 |
| `protocol/` | JSON Schema | MFP 协议定义 |
| `tools/`、`eval/` | Python / Rust | 开发工具、模拟器、评测 |
| `moodflow_macos/`、`moodflow_linux/` | Swift / C++ | macOS、Linux 端（不在当前计划内，未维护） |
| `docs/` | Markdown | 架构、设计、ADR、交接文档 |

需求、设计与测试以产品书为准（[young8290/xinqing](https://github.com/young8290/xinqing/tree/main/docs/product-book)），
本仓的说明从 [docs/moodflow/README.md](docs/moodflow/README.md) 读起，协作约定见 [AGENTS.md](AGENTS.md)。

## 从源码构建

- **Windows**：Rust stable + Visual Studio 2022（C++ 桌面开发）+ CMake + Node.js 与 pnpm，运行 `.\scripts\dev.ps1`
- **Linux（交叉编译 Windows 产物、跑测试）**：`scripts/dev.sh`

构建脚本会自动下载第三方词库数据并生成数据目录。测试与检查的完整命令见 [AGENTS.md](AGENTS.md)。

## 致谢与许可证

Moodflow 的输入法部分基于开源输入法项目 [WindInput](https://github.com/huanfeng/WindInput)（MIT 许可证），
原项目的版权声明保留在 [LICENSE](LICENSE) 中。Moodflow 是独立维护的分支，与原项目及其作者无关。

输入法的设计参考了 [RIME](https://rime.im)、[Fcitx5](https://github.com/fcitx/fcitx5) 等开源项目；
词库数据来自 [白霜拼音](https://github.com/gaboolic/rime-frost)、
[极点五笔](https://github.com/KyleBing/rime-wubi86-jidian)、[pinyin-data](https://github.com/mozillazg/pinyin-data)、
[OpenCC](https://github.com/BYVoid/OpenCC) 等项目，适用各自的许可证，完整声明见 [NOTICE.md](NOTICE.md)。

本项目源代码采用 [MIT 许可证](LICENSE)。
