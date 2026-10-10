# kb_count

Windows 便携式键盘鼠标输入统计工具，Rust 2024 + Slint + windows-rs + SQLite。

## 已实现

- EXE 同目录的 `config.toml`、`stats.db` 便携路径；无写入权限时给出明确错误。
- SQLite WAL schema：应用、日输入明细、日汇总、应用策略，并在退出时执行 checkpoint。
- Windows Raw Input 隐藏窗口接收鼠标事件，并使用低级键盘钩子接收物理键盘 KeyDown；两者均在采集线程消息循环中处理。
- 键盘 KeyDown、扫描码/扩展标识、鼠标按钮、滚轮 Delta 归一化。
- 前台进程路径识别和 PID 缓存，无法识别时使用 Unknown。
- 有界输入队列、内存聚合、5 秒批量写入、暂停/恢复。
- Detailed / Totals Only / Excluded 隐私策略；浏览器默认 Totals Only，密码管理器默认 Excluded。
- Windows 单实例互斥。
- 启动默认隐藏统计窗口，托盘菜单支持打开、暂停/恢复、设置和退出。
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

完整应用策略编辑页面、键盘热力图、开机启动配置、文件选择式 CSV 导出，以及 Windows 10/11 长时间资源占用测试仍需补齐。当前构建和单元测试已通过，实机输入覆盖测试尚未完成。



## 输入采集诊断

启动后控制台应出现类似日志：

```text
INFO kb_count::collector::raw_input: Raw Input mouse and low-level keyboard collector started
INFO kb_count: portable data directory initialized data_dir=...
```

最新版本默认过滤该 fontdb 字体扫描警告。请确认运行的是最新 `target/release/kb_count.exe`，并检查日志中的 `data_dir` 与 `stats.db` 是否在同一目录。统计默认每 5 秒批量写入数据库。

输入采集会忽略带有 `LLKHF_INJECTED` 标记的模拟键盘事件，因此 PowerShell `SendInput`、自动化工具等不会被当作真实用户输入；请用实体键盘和鼠标验证。托盘退出会先请求统计线程完成最后一次 Flush，再关闭数据库。



