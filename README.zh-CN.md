![Skyward Recovery](assets/hero.jpg)

# Skyward for merlinx

[English](README.md) | 简体中文

Skyward 是使用 Rust 从零编写的 recovery。本仓库是 [Skyward](https://github.com/kirklin/skyward)
的分支，专注于 **Redmi Note 9（merlinx）**。

> [!IMPORTANT]
> 本分支围绕 merlinx 的机型配置构建，面向 **vendor 分区为 vendor S（Android 12）**
> 的机器，且**只在 Infinity X 4.0 / Android 17 上适用并测试过**。
>
> 目前**只有启动界面可用**：屏幕、触摸、背光和 recovery 自己的菜单能正常显示。
> 其他功能——包括安装刷机包、WLAN、解密、振动和其余硬件——**尚未测试或尚未完成**。
> 见 [TODO](TODO.zh-CN.md) 和 [疑难问题](docs/tricky-issues.zh-CN.md)。

## 目前可用

- 在 merlinx 上启动，并在 MediaTek framebuffer 上完整绘制 recovery 界面
- 触摸输入、recovery 自己的菜单、主题和语言
- 通过 LED class 设备控制背光
- adb、`adb sideload`、fastbootd 和 MTP

其余功能请一律视为实验性。尤其是，**不要依赖这个版本解密或保护你的数据**。

## 支持的机型

| 机型 | 代号 | vendor | 测试的系统 |
| --- | --- | --- | --- |
| Redmi Note 9 (Global) | `merlinx` | vendor S（Android 12） | Infinity X 4.0（Android 17） |

上游的 `nezha` 和 `ishtar` 配置保留在 `devices/` 中，以便和上游保持接近，但本分支
并不为它们构建或测试。

## 安装

Skyward 需要已解锁的 bootloader。构建镜像（见下文）后，在手机处于 fastboot 模式时
把镜像刷入 recovery 分区：

```sh
fastboot flash recovery skyward-merlinx.img
fastboot reboot recovery
```

镜像是在 merlinx 原厂 recovery 分区的基础上构建的。请保留一份原厂 `recovery.img`，
以便随时恢复。

## 构建

Skyward 使用 `rust-toolchain.toml` 指定的稳定版 Rust 和它的 `aarch64-linux-android`
目标、Android NDK r29 和 [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) 构建。
recovery 服务针对手机构建，打包工具针对电脑构建：

```sh
export ANDROID_NDK_HOME=/path/to/android-ndk-r29
cargo ndk -t arm64-v8a --platform 30 build --release -p skyward
cargo build --release -p skyward-pack
```

然后在 merlinx 原厂 recovery 镜像的基础上打包：

```sh
target/release/skyward-pack recovery \
  --stock /path/to/stock/recovery.img \
  --binary target/aarch64-linux-android/release/skyward \
  --config devices/merlinx.toml \
  --font resources/fonts/MiSansVF.ttf \
  --mono resources/fonts/KirkMono-Regular.ttf \
  --mono-license resources/fonts/KirkMono-OFL.txt \
  --out skyward-merlinx.img
```

`devices/merlinx.toml` 中故意没有 `[stock]` 段：本分支使用你提供的 recovery 镜像
打包，而不是自动下载原厂更新包。

测试和代码检查在电脑上运行；只在手机上运行的代码按照手机目标检查：

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a --platform 30 clippy -p skyward -p skyward-install -- -D warnings
```

## 文档

- [疑难问题](docs/tricky-issues.zh-CN.md)：把本分支跑起来时遇到的问题，以及关于它们的结论
- [TODO](TODO.zh-CN.md)：尚未完成的工作

## 许可证

[GPL-3.0-or-later](LICENSE)
