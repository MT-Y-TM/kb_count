# 关闭窗口默认行为

默认每次询问。关闭提醒中勾选“记住此选择，下次关闭窗口时直接执行”，再点击“仅关闭窗口”或“退出应用”后，保存对应操作。取消、Esc 或只勾选但未执行操作，不会修改偏好。未勾选时只执行当次操作，不覆盖设置中已选的默认操作。提示文字会说明记住后可在设置中更改。

在“设置与数据 → 偏好设置”中，选择“关闭窗口时”的默认行为，并独立决定是否启用“关闭窗口时直接执行默认行为，不再询问”。取消记住后恢复每次询问；切换默认行为不强制启用记住。页脚同步显示当前关闭行为。

配置仍位于 EXE 同目录的 `config.toml`，没有新数据目录、网络请求或注册表统计数据：

```toml
remember_close_action = false
close_action = "background" # background 或 exit
```

旧配置缺失字段时，`remember_close_action` 默认 false，操作默认 background。文件夹迁移和复制时随其他配置一起保留。选择退出仍执行采集屏障、最终 Flush、成功确认、WAL checkpoint 与托盘清理；失败时保留进程并恢复采集状态。后台关闭保持原暂停状态。托盘“退出”不受此偏好影响。

只有配置写入成功才更新内存中的有效偏好。写入失败时设置控件回退，提示检查目录权限；关闭提醒中的保存失败保留窗口供重试。正常关闭已记住的后台行为不会重复写配置。退出请求发出后立即禁用关闭提醒中的按钮和复选框，避免等待托盘事件处理期间再次更改选择。

## 验证

配置测试覆盖旧字段兼容、两个操作与记住开关的 TOML 往返。Slint 离线交互使用测试配置存储，不读取或写入个人配置和数据库，检查实际复选框点击、取消不保存、重启恢复、直接后台关闭、直接退出回调、恢复询问、更换默认操作、写入失败回退与重试、忙碌期间禁用控件。原有日历、清空确认、趋势、图表和不同缩放的关闭操作回归继续执行。

界面沿用 Slint 1.13.1 的原生 [CheckBox](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/checkbox/) 与 [ComboBox](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/combobox/)，API 以锁定版本的组件源码和编译结果为准。离线截图包括 `target/ui-review/close-remember-minimum.bmp`、`close-settings-{dark,light}-minimum.bmp`，以及原有深浅主题、100%–200% 缩放、最小窗口关闭提醒。

本轮不将离线渲染当作原生 Windows 多屏测试，也未重新测量常驻内存和 CPU。

## 关闭偏好阶段结果（2026-10-16）

`cargo fmt --all -- --check`、`cargo check --locked --offline`、`cargo clippy --all-targets --locked --offline -- -D warnings` 通过。最终代码完整测试 50 项通过、0 失败，复核耗时 65.56 秒；另一次离线绘图回归 1 项通过，111.03 秒。

Release 产物构建时间 2026-10-16 16:59:28（本地），17,088,512 字节；PE 检查为 x64、GUI 子系统、12 种内嵌图标和版本资源。SHA-256：`26697066DB75FC9D2143D8CC1991751406DB863A870F63D805641E7D78513F71`。该产物尚未包含后续全屏游戏采集修复。
