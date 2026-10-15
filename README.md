# kb_count

Windows 便携式键盘鼠标输入统计工具，Rust 2024 + Slint + windows-rs + SQLite。

## 已实现

- EXE 同目录的 `config.toml`、`stats.db` 便携路径；无写入权限时给出明确错误。
- SQLite WAL schema：应用、日输入明细、日汇总、应用策略，并在退出时执行 checkpoint。
- Windows Raw Input 隐藏窗口接收鼠标事件，并使用低级键盘钩子接收物理键盘 KeyDown；两者均在采集线程消息循环中处理，键盘自动重复会被过滤。
- 键盘 KeyDown、扫描码/扩展标识、鼠标按钮、滚轮 Delta 归一化；滚轮总量累计绝对单位，方向在明细代码中区分。
- `daily_input_counts.input_type` 统一使用 `keyboard` / `mouse`；滚轮通过 `wheel_up`、`wheel_down`、`hwheel_up`、`hwheel_down` 代码区分，旧版 `wheel` 行会在打开数据库时自动迁移。
- 前台进程路径识别和 PID 缓存，无法识别时使用 Unknown。
- 有界输入队列、内存聚合、5 秒批量写入、暂停/恢复。
- Detailed / Totals Only / Excluded 隐私策略；浏览器默认 Totals Only，密码管理器默认 Excluded。
- Windows 单实例互斥。
- Slint 窗口先初始化，随后由采集线程注册鼠标 Raw Input；诊断会核对最终注册目标是否仍为采集窗口。
- 鼠标点击按光标所在顶层窗口归属应用，滚轮 Delta 支持高精度累积归一化。
- 启动默认隐藏统计窗口，托盘菜单支持打开、暂停/恢复、设置和退出。
- Slint 总览卡片、今日/昨日及指定日期查询、应用排行排序、应用详情、策略设置、清空数据二次确认和 CSV 导出。
- 开机启动注册表项可在设置界面启用或关闭，启动项路径使用当前 EXE 的绝对路径。
- 键盘热力图按物理扫描码显示常用键位的分级颜色和次数，支持全局或应用详情筛选。
- 热力图覆盖数字行、字母区、常用标点、修饰键和空格；应用排行条目可点击打开对应详情。
- 应用可执行路径按 Windows 大小写不敏感规则归一化，避免同一程序产生重复身份。
- 数据库批量写入失败时保留聚合批次并在后续 Flush 重试，队列和待写批次均有上限。
- 清空统计和退出操作通过统计线程串行执行，避免清空时把尚未写入的聚合批次重新写回。
- 暂停统计时仍保持消息循环和按键状态释放，但跳过进程识别、滚轮残余和事件入队。
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

发布产物为 `target/release/kb_count.exe`。将 EXE 复制到具有写入权限的目录后运行，配置和数据库位于 EXE 同级目录，不使用 AppData；CSV 仅在用户主动导出时保存到所选位置。

## 运行

1. 将 `kb_count.exe` 复制到具有读写权限的目录。
2. 双击启动。程序默认隐藏主窗口并驻留系统托盘；首次启动会在同目录创建 `config.toml` 和 `stats.db`。
3. 从托盘图标打开统计面板。关闭面板只会隐藏窗口，托盘菜单中的“退出”才会停止采集并完成最后一次 Flush。
4. 使用实体键盘、鼠标进行验证，等待默认 5 秒批量写入后再查询面板数据。自动化工具产生的注入键盘事件会被忽略。

如果双击后看不到窗口，请先检查系统托盘的隐藏图标区域；这是默认后台运行行为。若目录无写入权限，程序会提示将 EXE 移动到具有读写权限的目录，不会改写到 AppData。如果托盘菜单显示“恢复统计”，说明当前处于全局暂停状态；点击后才会继续采集。该状态保存在同目录的 `config.toml`，其中 `capture_enabled = true` 表示运行中。

## 统计面板

- **输入总览**：四项核心指标、可排序的完整应用排行、总输入与占比，点击应用行进入详情。支持按名称或路径即时搜索，并显示匹配数量；搜索保留当天排名和占比分母，不改变顶部日汇总。
- **键位分布**：常用主键区及 Esc/F1–F12 热力图、完整按键列表、鼠标与滚轮分类；可切换全部应用和当前应用。
- **应用详情**：独立显示应用路径、采集方式、三项总量和分类列表；仅总量模式明确提示未记录详细数据。
- **设置与数据**：主题、开机启动、应用策略、CSV 范围导出和本地存储；清空确认可取消，离开设置页时会取消确认。

总输入按键盘敲击、鼠标点击和滚轮绝对单位相加。排序切换后，占比跟随所选指标；名称和路径搜索不区分大小写，路径接受正反斜杠。

应用排行与详情显示本地 EXE 的图标，缺失、不可访问或网络路径使用首字母标识。图标使用单个后台工作线程按需读取，64px 透明图像缓存于内存（最多 128 项，请求队列最多 32 项），不会随每次输入重复读取，也不会保存到程序目录之外。为限制大量历史应用的图标内存与读取开销，优先加载当前详情及筛选结果前 127 个应用；其他应用可通过搜索或进入详情加载。图标变化会在下次运行时重新读取。

文字采用 Microsoft YaHei UI：应用名称及数值 13px，路径、占比和表头 12px，页面标题 22–25px，主要计数 26–30px（均为逻辑像素）。深浅主题加强说明文字对比度，并增加排行行距，避免只放大文字后产生重叠。设计及验证记录见 [界面可读性说明](docs/ui-readability.md)。

窗口支持浅色/深色主题，最小内容尺寸为 820 × 440 **逻辑像素**。窗口较小或 Windows 缩放较高时自动减少导航与卡片留白，保留全部排行数值列；热力图可横向和纵向滚动，详情与设置可纵向滚动。从托盘打开时，面板会按照当前显示器工作区收缩并调整位置，为任务栏和实际窗口边框留出空间。统计刷新保留正在编辑的日期、导出范围、应用搜索和应用查询路径。重复启动通过现有进程的 Slint 事件循环打开面板，避免直接显示原生隐藏窗口导致白屏。Release 默认不附带控制台窗口。

界面回归测试使用内存 SQLite 和 Slint 软件渲染器，不启动输入采集，也不读取个人统计数据。可在 PowerShell 中生成离线预览（样例聚合数据）：

```powershell
$env:KB_COUNT_RENDER_UI = '1'
cargo test --locked ui_tests -- --nocapture
Remove-Item Env:KB_COUNT_RENDER_UI
```

预览位于忽略的 `target/ui-review/`，覆盖四页的深浅主题、最小窗口、搜索匹配/未匹配、空数据和仅总量模式；还在 1280 × 720 物理像素客户区模拟 100%、125%、150% 缩放，并检查各尺寸下的导航、暂停、日期查询和应用行点击。测试也检查日期草稿、搜索大小写与路径匹配、筛选分母、应用行导航与隐私显示规则。离线渲染不能代替真实 Windows 多屏切换、系统缩放与工作区调整的实机验证，这些项目仍待补测。

## 当前限制

Windows 10/11 长时间资源占用测试仍需补齐。当前构建、启动、单实例、数据库策略和单元测试已通过；Windows 11 实体鼠标的左键、右键及垂直滚轮已验证到 SQLite，其他按钮和水平滚轮仍需对应设备实测。



## 输入采集诊断

Debug 构建的控制台会出现类似日志（Release 默认不显示控制台）：

```text
INFO kb_count::collector::raw_input: Raw Input mouse and keyboard collector started raw_input_registered=true mouse_raw_input_registered=true keyboard_raw_input_registered=true low_level_keyboard_hook=true
INFO kb_count: portable data directory initialized data_dir=... capture_enabled=true
INFO kb_count::collector::raw_input: first input event received input_type="mouse"
INFO kb_count::collector::raw_input: mouse input diagnostic summary raw_input_messages=... raw_mouse_packets=... mouse_button_packets=... mouse_wheel_packets=... mouse_events_enqueued=... mouse_unknown_attribution=...
```

键盘和鼠标分别只记录一次首事件诊断日志；日志不包含按键代码、输入内容、坐标或窗口标题。
鼠标诊断汇总每 10 秒输出一次，仅包含阶段计数；`raw_mouse_packets=0` 表示尚未收到鼠标 Raw Input，`mouse_events_enqueued` 则表示已进入统计队列。
`mouse_registration_target_matches=Some(true)` 表示当前进程的鼠标 Raw Input 仍注册到采集窗口。旧版在 Slint 初始化后可能显示 `Some(false)`，使鼠标输入转入别的窗口而无法进入统计链路。

实体鼠标验收可在普通桌面应用中依次执行左键、右键、中键、XButton1、XButton2、垂直滚轮和水平滚轮（设备支持时）。等待一次 10 秒诊断输出并再等待默认 5 秒批量写入：按钮动作应使 `mouse_button_packets` 和 `mouse_events_enqueued` 增加，垂直/水平滚轮应使 `mouse_wheel_packets` 增加并在明细中出现 `wheel_up/down` 或 `hwheel_up/down`。鼠标移动只会增加 `raw_mouse_packets`，不会产生数据库统计行。

最新版本默认过滤该 fontdb 字体扫描警告。请确认运行的是最新 `target/release/kb_count.exe`，并检查日志中的 `data_dir` 与 `stats.db` 是否在同一目录。统计默认每 5 秒批量写入数据库。

输入采集会忽略带有 `LLKHF_INJECTED` 标记的模拟键盘事件，因此 PowerShell `SendInput`、自动化工具等不会被当作真实用户输入；请用实体键盘和鼠标验证。托盘退出会先请求统计线程完成最后一次 Flush，再关闭数据库。

## 本机 Release 资源实测

以下是界面改版前的短时参考，改版后的资源占用仍需重新实机测量。测试环境为 Windows 11 专业工作站版 10.0.26200、16 个逻辑处理器，在无输入状态下预热 3 秒后用 PowerShell 每秒采样 10 秒。后台隐藏运行时，私有内存平均 69.95 MiB（69.89–70.53 MiB），工作集平均 65.91 MiB（65.90–65.91 MiB），全机平均 CPU 约 0.0386%。打开统计窗口后采样 10 秒，私有内存平均 69.70 MiB，工作集平均 65.93 MiB，全机平均 CPU 约 0.0097%。该结果不是所有 Windows 10/11 设备的性能承诺。



