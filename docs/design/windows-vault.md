# Windows 密码库

当前为待安全评审的实现；尚未作为正式安装包发布。Android 未接入。

## 持久化格式

`moodflow-vault` 不依赖 Windows、Tauri 或网络。条目使用独立随机 nonce 的 XChaCha20-Poly1305，AAD 绑定格式版本、库 UUID 和条目 UUID；整份快照再加密，磁盘不暴露标题、账号和索引。写入执行 SHA-256 revision CAS、目录锁和原子替换，拒绝覆盖并发修改和自动重建损坏文件。

主密钥为随机 256 位。口令用 Argon2id v0x13（64 MiB、3 次、1 通道）派生包装密钥；这些参数由 KeyWrap v1 固定，改变参数必须迁移格式。包装记录再经当前 Windows 用户的 DPAPI 保护。目录 DACL 只授予当前用户。库不参与普通资料搜索、AI、Buffer 或默认同步。

当前格式绑定 Windows 用户配置文件。仅复制库文件与知道主口令不足以在另一用户或重装后的配置文件恢复。可迁移的加密备份与同步另行实现，在接入前不能宣传跨设备恢复。

## 解锁与剪贴板

原生层持有不续期的 60 秒解锁租约，后台每 500 ms 清扫；暂停、非交互桌面、切换页面及退出均锁定。字段显示最多 15 秒，返回明文前再次检查隐私和租约。密码、口令和 TOTP 密钥不实现 Debug；Rust 临时缓冲和 Argon2 内存清零。WebView、IPC 和系统的复制不能承诺取证层面的彻底清零，安全评审须覆盖这一边界。

复制只由原生层执行，先发布 ExcludeClipboardContentFromMonitorProcessing、CanIncludeInClipboardHistory=0 和 CanUploadToCloudClipboard=0，再发布文本。剪贴板锁内记录 sequence；到期或锁定后先后两次核对 sequence，只清除仍属于本次操作的内容。打开失败则重试，不清除用户后来复制的内容。不自动填充或模拟粘贴。

## 验证与待办

- 纯逻辑 7 项测试覆盖认证加密、口令包装、CAS/篡改、真实密钥租约、随机密码和 RFC 6238 的 18 个 TOTP 向量。
- Windows Hub 54 项测试通过；定向原生测试覆盖 DPAPI 往返、屏蔽密码读取、保留旧秘密、修改 TOTP 参数、磁盘不含明文和隐私锁定。
- Hub 前端 377 项测试、静态检查和生产构建通过；密码库测试覆盖显式复制、15 秒隐藏、60 秒锁定与暂停。
- 真实系统剪贴板验证只在全新 Windows Actions runner 运行；测试断言 CI 环境后才触碰剪贴板。本机未操作用户剪贴板。
- 待完成桌面交互验收、可迁移加密备份与 E 的安全评审。Windows Hello 为可选能力，当前未提供。

评审入口：`crates/moodflow-vault/`、`moodflow_hub/src-tauri/src/vault*.rs`、`commands/vault.rs`、`VaultPage.vue`。
