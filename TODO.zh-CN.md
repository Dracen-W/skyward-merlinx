# TODO

本分支尚未完成的工作。这是中文版；英文版见 [TODO.md](TODO.md)。

## 硬件

- [ ] **WLAN**：MediaTek 栈已经能启动（`wmt_loader`、`wmt_launcher`、
      `wlan_assistant`），内核日志出现 `WLAN-GEN4 driver init`，但没有下载固件，
      `wlan0` 始终不出现。缺失的一步是手机自己的 WLAN HAL：`libwifi-hal-mtk.so`
      会向 `/dev/wmtWifi` 写入 `"1"` 来启动驱动。现在 recovery 在 assistant 报告
      `vendor.mtk.nvram.ready=1` 之后自己写入，并且某个 vendor 助手失败不再阻止
      后续服务。还需要在真机上验证。
- [ ] **振动**：马达是 MediaTek 的 LED class 设备
      `/sys/class/leds/vibrator`，`[haptics]` 现在通过 `duration` 和 `activate`
      驱动它。需要在真机上验证（一次点击 15–30 ms）。
- [ ] **解密**：被 TEE 阻塞。Keymaster 在打开 `/dev/isee_tee0` 时卡在
      `soter_open`。需要可用的 `teei_daemon`/TEE 配置。在 data 分区存在之前无法
      端到端测试（userdata 已因 iOS 移植被清空）。
- [ ] 在真机上验证触摸、按键和其余传感器。

## 界面

- [x] **时钟位置**：已移到距左侧 50 dp，避开 merlinx 左上角的摄像头挖孔。
- [x] 在 `crates/skyward-ui/src/app.rs` 的 `TIME_ZONES` 中加入
      **Asia/Singapore**，并补充测试。

## 待验证功能

- [ ] 安装刷机包、系统更新包和分区镜像
- [ ] 备份和恢复
- [ ] 清除数据 / 恢复出厂设置
- [ ] 挂载、修复和格式化文件系统
- [ ] 文件管理和终端
- [ ] 本机上的 MTP、adb 和 fastbootd
- [ ] 重启目标和槽位处理（A-only）

## 构建 / 仓库

- [ ] 决定保留的 `nezha` 和 `ishtar` 配置是否继续留在这个分支。
- [ ] 为 merlinx 增加可复现的原厂 recovery 来源（目前使用手工提供的 recovery
      镜像打包）。
