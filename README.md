# kb_count

Windows 便携式键盘鼠标输入统计工具，Rust 2024 + Slint + windows-rs + SQLite。

## 已实现

- EXE 同目录的 `config.toml`、`stats.db` 便携路径；无写入权限时给出明确错误。
- SQLite WAL schema：应用、日输入明细、日汇总、应用策略，并在退出时执行 checkpoint。
- Windows Raw Input 隐藏窗口，后台接收键盘和鼠标事件。
- 键盘 KeyDown、扫描码/扩展标识、鼠标按钮、滚轮 Delta 归一化。
- 前台进程路径识别和 PID 缓存，无法识别时使用 Unknown。
- 有界输入队列、内存聚合、5 秒批量写入、暂停/恢复。
- Detailed / Totals Only / Excluded 隐私策略；浏览器默认 Totals Only，密码管理器默认 Excluded。
- Windows 单实例互斥。
- Slint 总览卡片、今日/昨日切换、应用排行、暂停控制和 CSV 导出。
- 本地中文 Git 提交，未推送远端。

## 构建

```powershell
cargo fmt --all
cargo check --locked
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

发布产物为 `target/release/kb_count.exe`。将 EXE 复制到具有写入权限的目录后运行，配置、数据库、CSV 导出文件均位于 EXE 同级目录，不使用 AppData。

## 当前限制

系统托盘右键菜单、完整应用策略编辑页面、键盘热力图、开机启动配置、文件选择式 CSV 导出，以及 Windows 10/11 长时间资源占用测试仍需补齐。当前构建和单元测试已通过，实机输入覆盖测试尚未完成。

