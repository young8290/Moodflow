# moodflow-cite

文献元数据、CSL/BibTeX 交换、格式化和引用号验证的纯逻辑。禁止网络、Windows、Tauri 依赖。单条 GB/T 兼容格式来自 XSIME（Apache-2.0，出处与许可见 NOTICE.md），正式样式由 Hayagriva 的 CSL 处理器生成，不能用拼字符串冒充 APA/MLA 等正式标准。

缺少标题、作者、年份或对应出处时拒绝格式化；未知 CSL 字段与结构化作者应保留。外部元数据必须有界。DOI 查询在宿主中单次确认、只发 DOI。引文号必须全部落在真实检索结果范围内，引用与参考文献原子交付到 Buffer，不自动上屏。正文不得写日志。

