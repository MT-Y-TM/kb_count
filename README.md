# kb_count

Windows 便携式键盘鼠标输入统计工具（MVP）。

## 构建

```powershell
cargo fmt
cargo check
cargo build --release
```

发布产物为 `target/release/kb_count.exe`。将 EXE 复制到具有写入权限的目录后运行，`config.toml` 与 `stats.db` 会创建在 EXE 同级目录。

当前版本已完成：便携路径解析、配置初始化、SQLite WAL schema、汇总查询、基础 Slint 面板与暂停状态 UI。Raw Input 采集模块接口已预留，详细采集与托盘将在后续阶段实现。
