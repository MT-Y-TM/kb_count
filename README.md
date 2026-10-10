# kb_count

Windows 便携式键盘鼠标输入统计工具。

当前仓库包含：

- Rust 2024 + Slint 原生界面；
- EXE 同目录的 `config.toml`、`stats.db` 便携路径；
- SQLite WAL schema：应用、日输入明细、日汇总、应用策略；
- 内存聚合器与 Detailed / Totals Only / Excluded 策略模型；
- Windows Raw Input 采集模块接口预留。

## 构建

```powershell
cargo fmt --all
cargo check --locked
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

发布产物为 `target/release/kb_count.exe`。将 EXE 复制到具有写入权限的目录后运行，配置和数据库会创建在 EXE 同级目录，不使用 AppData。

## 当前状态

MVP 基础工程和数据层已完成并提交。Windows Raw Input 消息窗口、系统托盘、应用策略管理 UI、CSV 导出、热力图及完整稳定性测试仍需后续阶段实现。依赖验证需在 Cargo registry 可访问的环境执行。
