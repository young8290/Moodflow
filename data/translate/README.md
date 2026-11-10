# CC-CEDICT 离线中英词典

来源：[MDBG 的 CC-CEDICT 下载](https://www.mdbg.net/chinese/dictionary?page=cc-cedict)，2026-11-08 08:49:26 UTC，125,394 条。
Copyright (C) 1997, 1998 Paul Andrew Denisowski；后续贡献者见原始文件头部和 MDBG。

数据按 CC BY-SA 4.0 单独分发，许可全文见 `CC-BY-SA-4.0.txt`。压缩源文件逐字保留上游头部；`source.json` 固定日期、条数和压缩前后 SHA-256。上游下载地址会更新，因此保留这份快照保证构建可复现。

Moodflow 的修改：解析繁体、简体、拼音和释义，增加中文双向索引与英文释义/词项索引，按 UTF-8 前缀压缩为 `cedict.mfcd`。派生索引仍适用 CC BY-SA 4.0，与程序代码分开保存。重建：

```powershell
cd moodflow_input
cargo run -p moodflow-tools --bin gen_translation -- --src ../data/translate --out ../build_dev/data/translate/cedict.mfcd
```

查词仅提供词典匹配。长文本逐段标记覆盖情况，缺词不猜测、不发送网络；歧义以原始释义供选择。
