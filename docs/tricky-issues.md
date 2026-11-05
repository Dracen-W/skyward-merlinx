# Tricky issues

Problems found while bringing this fork up on merlinx, and what was learned about
them. This is the English version; see
[tricky-issues.zh-CN.md](tricky-issues.zh-CN.md) for Chinese.

## 1. Loop devices do not work in the recovery kernel

The running kernel is `4.19.325-cip133-st17-Skyron+`. Every read from a loop
device fails with `EIO`, even for a plain 512-byte file on tmpfs or ext4, and the
kernel threads are present. Because of that, anything that mounts a filesystem in
a file through a loop device fails.

Consequence: the runtime APEX (`com.android.runtime`) cannot be mounted. It is an
ext4 image at offset 4096 with size 12730368, and `mount_ro` tries erofs first and
returns `EIO`.

Workaround: do not depend on the loop-backed APEX. Use the installed system's own
bootstrap loader at `/mnt/system/system/bin/bootstrap/linker64` instead.

## 2. Android 17 `hwservicemanager` needs the device VINTF manifest

Android 17's `hwservicemanager` (`/system_ext/bin/hwservicemanager`) decides
whether to run the HIDL service manager from the **device** manifest, not from
the framework manifest a recovery publishes. It looks for
`android.hidl.manager@1.0::IServiceManager/default` with `hwbinder` transport.

If only the framework manifest declares it, the service manager reports
`ready=true` but `disabled=false`, and HIDL clients later fail with
`BR_DEAD_REPLY` ("the other side is gone").

Workaround: stage a device manifest on the recovery's own storage and bind-mount
it over `/vendor/etc/vintf/manifest.xml`. `/vendor` is read-only and full, so the
file is staged under `/skyward/vintf/` and mounted over the original.

## 3. Installed-system binaries need the installed-system loader

The recovery's own `/system` is Android 12-ish. The installed system and its
vendor binaries are newer. A loader and the lowest library are built together and
agree on the program's storage layout, so running an installed binary with the
recovery's loader can fail or crash.

The installed system's real bootstrap loader is at
`/mnt/system/system/bin/bootstrap/linker64` (2,402,552 bytes). Its
`/system/bin/linker64` is a symlink into the runtime APEX, which is dangling
without a working loop device.

Workaround: run installed-system programs as
`/mnt/system/system/bin/bootstrap/linker64 <program>`, with the installed system's
libraries first (`/vendor/lib64`, `/system_ext/lib64`, `/product/lib64`,
`/odm/lib64`, `/mnt/system/system/lib64/bootstrap`, `/mnt/system/system/lib64`).
This is implemented in `crates/skyward/src/runtime.rs`.

## 4. Keymaster is HIDL 4.1, and storage keys are exported as RAW

The ROM has no AIDL KeyMint. Decryption targets HIDL Keymaster 4.1:
`android.hardware.keymaster@4.1::IKeymasterDevice/default` (with 4.0 as a
fallback) and `kmsetkey.beanpod`.

The storage key is converted with `exportKey(KeyFormat::RAW = 3)`. The merlinx
metadata encryption keys are not hardware-wrapped
(`ro.crypto.volume.options=::v2`).

## 5. TEE blocks Keymaster at `soter_open`

The Keymaster service starts but blocks in `soter_open` while opening
`/dev/isee_tee0` (`teedev_open`), and never registers. The key service and the
credential service both reach the secure world through `teei_daemon`, which must
be started with the list of trusted apps its init script gives it. Until the TEE
side is up, decryption cannot complete.

## 6. MediaTek framebuffer, not DRM/KMS

merlinx has no `/dev/dri`. The display is MediaTek `mtkfb` at
`/dev/graphics/fb0`: 1080x2340, 32bpp, RGBA, pitch 4352, `yres_virtual=7104`.

The recovery therefore uses a framebuffer backend with two buffers and
`FBIOPAN_DISPLAY`. It keeps at most two buffers, because the kernel only hands
over that many.

## 7. Backlight is not in `/sys/class/backlight`

`/sys/class/backlight` is empty on merlinx. The backlight is
`/sys/class/leds/lcd-backlight` with `max_brightness=2047`.

The recovery reads the Qualcomm-only backlight node only where it exists, and
falls back to the LED class device. Backlight is optional: a missing node must not
hang the UI.

## 8. WLAN needs the MediaTek stack, and still does not come up

The WLAN driver is built into the kernel; the vendor partition carries no WLAN
`.ko`. The MediaTek stack needs:

- `wmt_loader`, which creates `/dev/wmtWifi`, `/dev/stpwmt`, `/dev/wmtdetect`
  and `/dev/connfem`, and sets `vendor.connsys.driver.ready=yes`
- `wmt_launcher -p /vendor/firmware/ -o 1`
- `wlan_assistant`, which writes NVRAM from
  `/mnt/vendor/nvdata/APCFG/APRDEB` to `/dev/wmtWifi` and sets
  `vendor.mtk.nvram.ready=1`

The kernel logs `WLAN-GEN4 driver init`, but no firmware download happens and
`wlan0` never appears. Manual `WIFI` / `WR-BUF:WIFI` writes return `EIO` / `524`.

`wmt_launcher` downloads the chip's own patch and then sets
`vendor.connsys.formeta.ready=yes`. `wlan_assistant` must wait for that before
writing NVRAM and the `WIFI` command, or the driver never asks for the WLAN
firmware. The service now declares that property as its ready signal. This is
still being verified; see the TODO.

## 9. A-only devices

merlinx has no A/B slots and no `slot` suffix on partitions. Skyward now supports
this with `Slot::Only`; `ab = false` in `devices/merlinx.toml`.

## 10. Legacy boot images

merlinx uses a legacy boot image (v0/v1/v2), not the newer boot image format. The
packer reads and writes v0/v1/v2, including gzip ramdisks.

## 11. `/metadata` may not be mountable

A failed `/metadata` mount is not fatal: super still mounts, and the recovery
continues. A metadata partition that was never formatted must not block startup.

## 12. Mount points differ from the recovery's own

The installed system's fstab mounts `system` at `/mnt/system`, `system_ext` at
`/system_ext`, `product` at `/product`, `vendor` at `/vendor` and `odm` at
`/odm`. Other partitions go to `/mnt/<name>`, with `nvdata` at
`/mnt/vendor/nvdata` and `persist` at `/mnt/vendor/persist`.

Do not assume the recovery's own mount points. If a partition is not mounted
where a service expects it, mount it manually.

## 13. Vibration is an LED class device, not force feedback

MediaTek's `mtk_vibrator` driver registers the motor as the LED class device
`/sys/class/leds/vibrator`, with `duration` (milliseconds) and `activate`
attributes, not as an input force-feedback device. The recovery's original
haptics only spoke force feedback, so merlinx had no working click.

`[haptics] device = "/sys/class/leds/vibrator"` now selects a timed backend: a
click writes `duration` and then `activate=1`, and the driver stops the motor
itself when the time is up. A missing motor is not fatal: the UI logs it and
carries on without haptics.
