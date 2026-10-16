# 全屏游戏输入排查

本轮用户复现目标为：普通权限的 kb_count，管理员权限启动的《原神》，无边框模式。不得把桌面或模拟包测试当作游戏兼容已验证，也不通过提权、注入或关闭游戏保护来绕过限制。

## 代码中确认的问题

此前虽然注册了键盘 Raw Input，但只有低级键盘 Hook **安装失败**时才处理 Raw Input 键盘包。Hook 安装成功不等于在所有前台程序中都能收到输入；原实现也无法察觉 Hook 被系统静默移除。微软在 [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc) 文档中建议此类监控优先使用 Raw Input。

当前选择键盘 Raw Input 为主路径，Hook 只作后备，二者互斥计数。Raw Input 的长按过滤不再查询异步按键状态，改用每个设备的 make/break 状态；设备变化、睡眠恢复清理状态，Pause 无 break 的情况单独处理。普通修饰键和多键盘状态独立。键盘、鼠标均保留 INPUTSINK 注册；注册查询包含目标 HWND 和 INPUTSINK 标志，低频检查可在同进程 UI 库覆盖注册时修复。其他进程注册不会覆盖本进程，参见 [RegisterRawInputDevices](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices)。

此前鼠标只使用 `GetCursorPos → WindowFromPoint`。新归属优先采用 [GetGUIThreadInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo) 返回的实际捕获窗口；前台窗口覆盖整个显示器且光标隐藏时，光标仍处于其范围内则优先前台游戏。第二显示器上的点击、普通窗口、可见光标的全屏程序仍按位置判断。桌面和 Shell 窗口不作为游戏候选。不保存坐标、窗口标题或输入内容，无法获取应用路径时仍使用 Unknown。

这些是已确认的实现缺陷和对应修改；“管理员权限原神的缺失是否全部由此造成”需要实际游戏验证，不能仅从源码下结论。[Raw Input 官方说明](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input) 支持使用 INPUTSINK 在后台读取设备输入，但不等于对受限桌面、所有游戏环境的采集承诺。

## 可选汇总诊断

先正常退出旧 kb_count，再使用普通权限 PowerShell 启动新版：

```powershell
& 'H:\rust_project\kb_count\target\release\kb_count.exe' --capture-diagnostics
```

绝对启动路径只是本机示例，存储仍通过 EXE 实际目录解析。此模式在 EXE 同目录的 `logs/input-diagnostics.toml` 保存汇总。普通启动不创建诊断线程或文件；诊断模式只创建一个容量为 1 的队列和一个后台写入线程，每 10 秒取汇总，未变化不写入，覆盖同一个小文件，不追加历史。采集回调不执行文件 I/O。

字段包括本进程权限状态（standard/elevated/unavailable）、两类注册归属、键盘当前路径、收到的 Raw 键盘/鼠标包数量、Hook 消息数量、入队数量、Unknown 归属数量、进程查询失败数量、捕获/全屏归属次数、队列丢弃与注册修复次数。权限状态仅查询本进程，不请求提升权限。没有应用路径、标题、按键码、逐次时间戳、坐标或原始输入序列。诊断计数位于采集层，可能包含最终被策略排除的输入包；统计库仍由隐私策略过滤。

## 原神验收步骤

保持游戏以管理员权限、无边框模式运行，kb_count 使用普通权限。分别在普通桌面程序和游戏中进行几次实体键盘按下、长按、左/右键点击、滚轮操作；不要使用合成输入作为物理设备验证。返回统计面板前等待至少 10 秒，让默认 5 秒数据库批量提交和诊断快照完成。

- `process_elevation = "standard"`：证明 kb_count 未提权，避免把同为管理员时的测试误认为普通权限兼容。
- `raw_keyboard_primary = true` 且两类 registration 为 `collector`：采集窗口注册有效。
- Raw 包增加但入队不增加：检查归一化、暂停状态或队列丢弃；Raw 键盘包含 key-up 与长按重复，数量不应与统计次数直接相等。
- 入队增加且 Unknown/进程查询失败增加：优先排查应用归属，而不是把输入丢弃。
- 游戏应用的日键盘、鼠标、滚轮汇总应新增，长按只增加一次，松开后再按正常增加；桌面程序仍能统计且没有双计。
- 若在游戏中 Raw 包本身不增加，保留汇总诊断继续定位，不把单元测试通过当作此环境已解决。

## 自动回归范围

测试覆盖连续 1,000 次长按重复、松开后重按、多设备/扩展键独立状态、Pause、鼠标捕获优先、隐藏光标全屏窗口的归属、第二显示器、无效目标、负坐标显示器边界、诊断内容仅含汇总。另在测试进程的真实隐藏 HWND 上注册键盘与鼠标，使用 Win32 查询验证后台 INPUTSINK 标志；移除 INPUTSINK 后验证其不被错误识别为有效后台注册。该测试不改变前台窗口、不注入输入、不读取用户数据库。保留原有 SQLite、隐私策略和界面回归。真实游戏、权限环境和持续资源占用另外验收。

## 候选版检查

最终代码的 `cargo fmt --all -- --check`、`cargo check --locked --offline`、`cargo clippy --all-targets --locked --offline -- -D warnings` 均通过。完整 `cargo test --locked --offline`：56 项通过、0 失败，58.39 秒，其中包含真实 Win32 注册查询与本进程权限查询。没有将这些测试等同为《原神》实测；管理员原神＋普通权限 kb_count 的最终游戏验证仍待完成。

候选代码提交 `1c0d8c7`。`cargo build --release --locked --offline` 成功，耗时 4 分 59 秒。产物 `target/release/kb_count.exe`：17,120,768 字节，2026-10-16 23:55:07（+08:00）生成；SHA-256 为 `CEB1A6F38D0180857B920D0CF86CB2D5FE195EACA7A164200748B3A68FF65DA4`。PE 验证确认 x64、GUI 子系统、12 种图标及版本资源。运行中的旧 EXE 已仅在原目录改名保留为 `kb_count.before-game-input-20261016-235008.exe`，未强制终止用户采集进程，也未移动配置或统计库。
