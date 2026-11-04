# 疑难问题

把本分支在 merlinx 上跑起来时遇到的问题，以及关于它们的结论。这是中文版；
英文版见 [tricky-issues.md](tricky-issues.md)。

## 1. recovery 内核里的 loop 设备不可用

运行中的内核是 `4.19.325-cip133-st17-Skyron+`。所有从 loop 设备读取的操作都返回
`EIO`，即使是 tmpfs 或 ext4 上一个普通的 512 字节文件也一样，内核线程本身存在。
因此，任何通过 loop 设备挂载“文件里的文件系统”的操作都会失败。

后果：运行时 APEX（`com.android.runtime`）无法挂载。它是一个 ext4 镜像，位于偏移
4096，大小 12730368；`mount_ro` 先尝试 erofs，然后返回 `EIO`。

规避：不要依赖 loop 支持的 APEX。改用已安装系统自带的 bootstrap loader：
`/mnt/system/system/bin/bootstrap/linker64`。

## 2. Android 17 的 `hwservicemanager` 读取设备 VINTF manifest

Android 17 的 `hwservicemanager`（`/system_ext/bin/hwservicemanager`）根据**设备**
manifest 而不是 recovery 发布的框架 manifest，来决定是否运行 HIDL 服务管理器。
它查找的是 `android.hidl.manager@1.0::IServiceManager/default`，transport 为
`hwbinder`。

如果只有框架 manifest 声明了它，服务管理器会报告 `ready=true` 但
`disabled=false`，之后 HIDL 客户端会以 `BR_DEAD_REPLY`（“the other side is
gone”）失败。

规避：把一份设备 manifest 暂存在 recovery 自己的存储上，再 bind mount 覆盖
`/vendor/etc/vintf/manifest.xml`。`/vendor` 是只读且已满的，所以文件暂存在
`/skyward/vintf/` 下，再挂载覆盖原文件。

## 3. 已安装系统的程序要用已安装系统的 loader

recovery 自己的 `/system` 是 Android 12 左右的版本，而已安装的系统及其 vendor
程序更新。loader 和最底层的库是一起构建的，对程序自身存储的布局有共同约定，
所以用 recovery 的 loader 运行已安装系统的程序可能失败或崩溃。

已安装系统真正的 bootstrap loader 在
`/mnt/system/system/bin/bootstrap/linker64`（2,402,552 字节）。它的
`/system/bin/linker64` 是指向运行时 APEX 的符号链接；loop 设备不可用时该链接是
悬空的。

规避：用
`/mnt/system/system/bin/bootstrap/linker64 <程序>` 运行已安装系统的程序，并把
已安装系统的库放在前面（`/vendor/lib64`、`/system_ext/lib64`、`/product/lib64`、
`/odm/lib64`、`/mnt/system/system/lib64/bootstrap`、
`/mnt/system/system/lib64`）。实现在 `crates/skyward/src/runtime.rs`。

## 4. Keymaster 是 HIDL 4.1，存储密钥按 RAW 导出

ROM 里没有 AIDL KeyMint。解密针对 HIDL Keymaster 4.1：
`android.hardware.keymaster@4.1::IKeymasterDevice/default`（4.0 作为回退），以及
`kmsetkey.beanpod`。

存储密钥通过 `exportKey(KeyFormat::RAW = 3)` 转换。merlinx 的 metadata 加密密钥
没有经过硬件封装（`ro.crypto.volume.options=::v2`）。

## 5. TEE 在 `soter_open` 处卡住 Keymaster

Keymaster 服务能启动，但在打开 `/dev/isee_tee0`（`teedev_open`）时卡在
`soter_open`，一直不注册。密钥服务和凭据服务都通过 `teei_daemon` 访问安全世界；
`teei_daemon` 必须带上其 init 脚本给出的可信应用列表启动。TEE 一侧没起来之前，
解密无法完成。

## 6. 是 MediaTek framebuffer，不是 DRM/KMS

merlinx 没有 `/dev/dri`。显示设备是 MediaTek `mtkfb`，位于
`/dev/graphics/fb0`：1080x2340，32bpp，RGBA，pitch 4352，`yres_virtual=7104`。

因此 recovery 使用 framebuffer 后端，双缓冲并调用 `FBIOPAN_DISPLAY`。最多只保留
两个缓冲区，因为内核只交出这么多。

## 7. 背光不在 `/sys/class/backlight`

merlinx 的 `/sys/class/backlight` 是空的。背光是
`/sys/class/leds/lcd-backlight`，`max_brightness=2047`。

recovery 只在 Qualcomm 专用的背光节点存在时读取它，否则回退到 LED class 设备。
背光是可选的：节点缺失不能让界面卡住。

## 8. WLAN 需要 MediaTek 整套栈，而且目前仍未起来

WLAN 驱动编译进内核；vendor 分区里没有 WLAN 的 `.ko`。MediaTek 的栈需要：

- `wmt_loader`：创建 `/dev/wmtWifi`、`/dev/stpwmt`、`/dev/wmtdetect`、
  `/dev/connfem`，并设置 `vendor.connsys.driver.ready=yes`
- `wmt_launcher -p /vendor/firmware/ -o 1`
- `wlan_assistant`：把 NVRAM 从 `/mnt/vendor/nvdata/APCFG/APRDEB` 写入
  `/dev/wmtWifi`，并设置 `vendor.mtk.nvram.ready=1`

内核日志出现 `WLAN-GEN4 driver init`，但没有下载固件，`wlan0` 始终不出现。
手动写 `WIFI` / `WR-BUF:WIFI` 返回 `EIO` / `524`。这个问题仍未解决，见 TODO。

## 9. A-only 机型

merlinx 没有 A/B 槽位，分区名也没有槽位后缀。Skyward 现在通过 `Slot::Only` 支持
这种情况；`devices/merlinx.toml` 中设置 `ab = false`。

## 10. 旧式 boot image

merlinx 使用旧式 boot image（v0/v1/v2），不是较新的 boot image 格式。打包工具
读写 v0/v1/v2，并支持 gzip ramdisk。

## 11. `/metadata` 可能无法挂载

`/metadata` 挂载失败不是致命错误：super 仍会挂载，recovery 继续启动。从未格式化
过的 metadata 分区不能阻塞启动。

## 12. 挂载点和 recovery 自己的不同

已安装系统的 fstab 把 `system` 挂在 `/mnt/system`，`system_ext` 挂在
`/system_ext`，`product` 挂在 `/product`，`vendor` 挂在 `/vendor`，`odm` 挂在
`/odm`。其他分区挂到 `/mnt/<name>`，其中 `nvdata` 在 `/mnt/vendor/nvdata`，
`persist` 在 `/mnt/vendor/persist`。

不要假设 recovery 自己的挂载点。如果某个分区没有挂在服务期望的位置，手动挂载它。
