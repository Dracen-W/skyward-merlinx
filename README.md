![Skyward Recovery](assets/hero.jpg)

# Skyward for merlinx

English | [简体中文](README.zh-CN.md)

Skyward is a recovery written in Rust, from scratch. This repository is a fork of
[Skyward](https://github.com/kirklin/skyward) focused on the **Redmi Note 9
(merlinx)**.

> [!IMPORTANT]
> This fork is built around the merlinx device configuration. It targets phones
> whose **vendor partition is vendor S (Android 12)**, and it is applicable to and
> has **only been tested on Infinity X 4.0 / Android 17**.
>
> At this stage **only the boot UI works**. The screen, touch, backlight and the
> recovery's own menus come up. Other features — including installing packages,
> WLAN, decryption, vibration and the remaining hardware — are **untested or not
> finished**. See [TODO](TODO.md) and [Tricky issues](docs/tricky-issues.md).

## What works now

- Boots on merlinx and draws the full recovery UI on the MediaTek framebuffer
- Touch input, the recovery's own menus, themes and languages
- Backlight control through the LED class device
- adb, `adb sideload`, fastbootd and MTP

Everything else should be treated as experimental. In particular, **do not rely
on this build to decrypt or preserve your data**.

## Supported device

| Device | Codename | Vendor | Tested system |
| --- | --- | --- | --- |
| Redmi Note 9 (Global) | `merlinx` | vendor S (Android 12) | Infinity X 4.0 (Android 17) |

The upstream `nezha` and `ishtar` configurations are kept in `devices/` so the
fork stays close to upstream, but this fork is not built or tested for them.

## Installing

Skyward needs an unlocked bootloader. Build the image (see below), then flash it
to the recovery partition with the phone in fastboot mode:

```sh
fastboot flash recovery skyward-merlinx.img
fastboot reboot recovery
```

The image is built on top of the stock merlinx recovery partition. Keep a copy of
your stock `recovery.img` so you can go back.

## Building

Skyward builds with the stable Rust that `rust-toolchain.toml` names, with its
`aarch64-linux-android` target, the Android NDK r29 and
[cargo-ndk](https://github.com/bbqsrc/cargo-ndk). The recovery service is built
for the phone, and the packer for the computer:

```sh
export ANDROID_NDK_HOME=/path/to/android-ndk-r29
cargo ndk -t arm64-v8a --platform 30 build --release -p skyward
cargo build --release -p skyward-pack
```

Then pack the image on top of a stock merlinx recovery image:

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

The `[stock]` section is intentionally absent from `devices/merlinx.toml`: this
fork is packed from a recovery image you supply, not from a stock package
downloaded automatically.

Tests and lints run on the computer; what runs only on the phone is linted for it:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a --platform 30 clippy -p skyward -p skyward-install -- -D warnings
```

## Documentation

- [Tricky issues](docs/tricky-issues.md) — problems found while bringing this
  fork up, and what was learned about them
- [TODO](TODO.md) — what is unfinished

## License

[GPL-3.0-or-later](LICENSE)
