# 用户词库故障与恢复（Windows）

遇到用户词、短语或词频不再保存，先保留现场。不要删除 `userdata.redb`，也不要用空白数据库覆盖它。数据库打开失败时，输入可能仍然可用，但本轮学习结果不会持久化。

## 先备份整个目录

1. 用 Win+Space 切换到其他输入法，关闭使用 Moodflow 的编辑窗口，在 Moodflow 菜单中退出服务；便携版还须退出启动器。恢复命令遇到正在使用的库会拒绝操作。
2. 把整个用户目录复制到一个新目录，保留 `userdata.redb`、配置、方案、主题及已有导出包。后续使用备份副本操作。默认目录为 `%APPDATA%\Moodflow`，开发版为 `%APPDATA%\MoodflowDev`；自定义目录和便携版以实际用户目录为准。
3. 记录故障时间与错误提示。分享诊断时不要上传原库或恢复包，它们包含用户词和短语正文。

## 服务仍能读取时

在安装目录打开终端，使用 CLI 包装器导出一个新的整机备份文件：

```powershell
.\moodflow_cli.bat backup create "D:\MoodflowBackup\before-recovery.zip" --stats --state
.\moodflow_cli.bat backup inspect "D:\MoodflowBackup\before-recovery.zip"
```

`create` 和 `restore` 需要服务在线，`inspect` 不需要。这里的示例路径应改成你的备份目录。

## 服务打不开原库时

先退出服务，再对保留下来的数据库副本执行离线恢复导出。输出文件必须不存在：

```powershell
.\moodflow_cli.bat backup recover "D:\MoodflowBackup\original\userdata.redb" "D:\MoodflowBackup\recovered.zip"
.\moodflow_cli.bat backup inspect "D:\MoodflowBackup\recovered.zip"
```

命令以只读句柄打开原库并尝试加共享锁，完整复制到隔离临时目录，然后在临时副本上打开 redb 并调用现有备份导出。redb 2.6.3 的打开过程需要可写后端，因此任何修复、建表或索引补建均只发生在临时副本。原库不会被自动删除、隔离、重建或覆盖。

成功后会显示原库 SHA-256 和恢复包条目数。恢复包包含能读取的数据库内容，包括各方案的用户词、临时词、词频、候选调整、全局短语和统计；配置文件、方案文件、主题文件仍依赖第一步的整个目录备份。

空库、无法解析的损坏库、正在使用的库会失败并保留原文件。读取或写入失败时不发布半个恢复包；已有同名输出也不会被覆盖。该命令无法保证抢救严重损坏的数据库。失败后继续保留原库、整个目录备份和错误信息，不要反复覆盖现场。

## 把导出结果导入可用环境

先用 `backup inspect` 确认内容，并在另外一个可用的 Moodflow 用户环境中验证恢复。服务在线时执行：

```powershell
.\moodflow_cli.bat backup restore "D:\MoodflowBackup\recovered.zip"
```

默认合并；不要在尚未确认结果时使用 `--replace`。恢复后检查用户词和短语，重启服务确认数据仍然保留，再决定是否手动替换故障环境。恢复工具不会替你移动、删除或替换故障数据库。
