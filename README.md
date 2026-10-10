# kb_count

Windows 便携式键盘鼠标输入统计工具，Rust 2024 + Slint + windows-rs + SQLite。

## 已实现

- EXE 同目录的 `config.toml`、`stats.db` 便携路径；无写入权限时给出明确错误。
- SQLite WAL schema：应用、日输入明细、日汇总、应用策略，并在退出时执行 checkpoint。
- Windows Raw Input 隐藏窗口接收鼠标事件，并使用低级键盘钩子接收物理键盘 KeyDown；两者均在采集线程消息循环中处理，键盘自动重复会被过滤。
- 键盘 KeyDown、扫描码/扩展标识、鼠标按钮、滚轮 Delta 归一化。
- 前台进程路径识别和 PID 缓存，无法识别时使用 Unknown。
- 有界输入队列、内存聚合、5 秒批量写入、暂停/恢复。
- Detailed / Totals Only / Excluded 隐私策略；浏览器默认 Totals Only，密码管理器默认 Excluded。
- Windows 单实例互斥。
- 鼠标点击按光标所在顶层窗口归属应用，滚轮 Delta 支持高精度累积归一化。
- 启动默认隐藏统计窗口，托盘菜单支持打开、暂停/恢复、设置和退出。
- Slint 总览卡片、今日/昨日及指定日期查询、应用排行排序、应用详情、策略设置、清空数据二次确认和 CSV 导出。
- 开机启动注册表项可在设置界面启用或关闭，启动项路径使用当前 EXE 的绝对路径。
- 键盘热力图按物理扫描码显示常用键位的分级颜色和次数，支持全局或应用详情筛选。
- 热力图覆盖数字行、字母区、常用标点、修饰键和空格；应用排行条目可点击打开对应详情。
- 应用可执行路径按 Windows 大小写不敏感规则归一化，避免同一程序产生重复身份。
- 数据库批量写入失败时保留聚合批次并在后续 Flush 重试，队列和待写批次均有上限。
- 清空统计和退出操作通过统计线程串行执行，避免清空时把尚未写入的聚合批次重新写回。
- Windows 原生 CSV 保存对话框、浅色/深色主题、睡眠恢复状态重置和进程缓存定期复核。
- 统计窗口隐藏时不刷新排行榜和热力图；窗口打开后只在统计数据版本变化时刷新，定时器仅作为低频唤醒检查。
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

Windows 10/11 长时间资源占用测试仍需补齐。当前构建、启动、单实例、数据库策略和单元测试已通过，实机输入覆盖测试仍需在实体键鼠上完成。



## 输入采集诊断

启动后控制台应出现类似日志：

```text
INFO kb_count::collector::raw_input: Raw Input mouse and low-level keyboard collector started
INFO kb_count: portable data directory initialized data_dir=...
```

最新版本默认过滤该 fontdb 字体扫描警告。请确认运行的是最新 `target/release/kb_count.exe`，并检查日志中的 `data_dir` 与 `stats.db` 是否在同一目录。统计默认每 5 秒批量写入数据库。

输入采集会忽略带有 `LLKHF_INJECTED` 标记的模拟键盘事件，因此 PowerShell `SendInput`、自动化工具等不会被当作真实用户输入；请用实体键盘和鼠标验证。托盘退出会先请求统计线程完成最后一次 Flush，再关闭数据库。

## 本机 Release 资源实测

测试环境为 Windows 11 专业工作站版 10.0.26200，后台隐藏运行 10 秒、无输入，使用 PowerShell `Get-Process` 每秒采样：私有内存约 32.21–32.34 MiB，工作集约 82.12–82.25 MiB，平均 CPU 约 0.04%；打开统计窗口后短时采样约 32.01–32.14 MiB、81.87–81.99 MiB，平均 CPU 约 0.07%。当前 Release EXE 约 11.32 MiB。该结果只代表本机短时测试，不是所有 Windows 10/11 设备的性能承诺。



