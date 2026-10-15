# db-grep

库扫描（产品书 12 第 2 节、17 第 4 节）：只读打开 `xinqing.db`，在所有表的所有列里搜索测试句，验证用户打过的原文没有落库（NFR-PRI-04 / TC-PRI-01）。

- `db-grep --db <xinqing.db> --phrases <测试句.txt> [--fragment 6] [--strict]`
  - 测试句每行一句，空行和 `#` 开头的行跳过。仓库里的 50 句特征句：`eval/datasets/pri_sentences.txt`。
  - 每列都按文本比较（`CAST(col AS TEXT)`），不只看声明为 TEXT 的列。
  - 除整句外还搜每句中 `--fragment` 个字一段的连续片段（默认 6），截断或改了标点存下的原文也查得出；`--fragment 0` 只搜整句。
  - 默认放过用户确认的日程标题、地点与待办标题（NFR-PRI-04 的例外）；`--strict` 连这些也查。
  - 输出每处命中的 `表.列`、行号和第几句，最后一行是命中总数；有命中时退出码为 1。Hub 运行时也可以扫（只读、看得到 WAL 里已提交的内容）。
- 扫描逻辑在 `xinqing_hub/core/src/infra/store/grep.rs`，自动化测试 `xinqing_hub/core/tests/privacy_db.rs` 用同一份：把 50 句当作上屏文字连同打字节奏喂给实时感知和日程识别（Jev 都说“是”、大模型不可用，命中初筛的句子走本地规则抽取存为待确认），再扫全库；同时检查发给 Jev 的 Q-STATE 请求里没有汉字（NFR-PRI-03）。
- TC-PRI-01 真机步骤：在记事本里用心晴输入法打完 50 句（同意 ①～③；dev 构建的目录是 `XinQingDev`），退出 Hub 或等 30 秒，`cargo run -p db-grep -- --db "%LOCALAPPDATA%\XinQing\hub\xinqing.db" --phrases eval/datasets/pri_sentences.txt`，命中数应为 0。
