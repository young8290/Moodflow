# Moodflow

> 懂你心情的 AI 输入陪伴。一个 Windows 中文输入法，加上一位陪伴角色“晴晴”；另有 Android 输入法（预览，只有输入法与设置，不含陪伴，ADR 0056）。
> 输入法部分由开源项目 [WindInput](https://github.com/huanfeng/WindInput)（MIT）分支而来，现已独立维护（ADR 0045）。

本仓同时放着输入法（`moodflow_input/`、`moodflow_tsf/` 等）和陪伴部分（Hub、MFP 协议、开发工具、模板、评测）。
目录、构建、测试与协作约定统一写在根目录 [AGENTS.md](../../AGENTS.md)，陪伴部分见其中「Moodflow 部分」一节。

## 产品书

需求、设计、测试与计划以产品书为准，产品书**只放在** [young8290/xinqing](https://github.com/young8290/xinqing/tree/main/docs/product-book)，本仓不复制。
代码注释和文档里的“08 FR-AIG-02”“17 第 2.5 节”这类编号都指产品书的章节。

| 你是 | 先读 |
|---|---|
| 新加入的组员 | [00 总览与阅读指南](https://github.com/young8290/xinqing/blob/main/docs/product-book/00_总览与阅读指南.md) → 01 → 02 → 03 |
| 准备写代码 | [14 开发环境搭建](https://github.com/young8290/xinqing/blob/main/docs/product-book/14_开发环境搭建与开发指南.md) → [16 任务分解](https://github.com/young8290/xinqing/blob/main/docs/product-book/16_任务分解与工作量估算.md) → [17 详细设计](https://github.com/young8290/xinqing/blob/main/docs/product-book/17_详细设计说明.md) |
| 负责测试 | [12 测试与验收](https://github.com/young8290/xinqing/blob/main/docs/product-book/12_测试与验收.md) → [18 测试用例与追溯矩阵](https://github.com/young8290/xinqing/blob/main/docs/product-book/18_测试用例清单与追溯矩阵.md) |
| 改模板或词表 | [15 内容资产与模板说明](https://github.com/young8290/xinqing/blob/main/docs/product-book/15_内容资产与模板说明.md) |
| 试用者 | [19 用户手册](https://github.com/young8290/xinqing/blob/main/docs/product-book/19_用户手册.md)、[21 隐私说明与知情同意书](https://github.com/young8290/xinqing/blob/main/docs/product-book/21_隐私说明与知情同意书.md) |

实现与产品书不一致的地方记在 [docs/adr/](../adr/)（0007 起），每份 ADR 的“影响”列出产品书要改的章节。

## 现状与计划

- 总体现状与全部未完成项：[docs/ROADMAP.md](../ROADMAP.md)。
- 从 XSIME 吸收的输入功能与 Android 端：[xsime-feature-port-plan.md](../design/xsime-feature-port-plan.md)（§9 进度、§10 未完成项）。
- 插件系统与插件市场：[plugin-system.md](../design/plugin-system.md)（§10 进度、§11 未完成项）。

## 本目录

- [identity.md](identity.md)：Windows 上的标识（与上游输入法共存），以及改名对已有安装的影响。
- [handover/](handover/)：各角色的交接文档（任务状态、代码地图、进行中的工作、已知问题），随各自的 PR 更新。
- [reviews/](reviews/)：评审记录（模板见产品书 16 第 6.2 节），例如内容资产评审（E-08）。
