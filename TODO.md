# TODO

Unfinished work on this fork. This is the English version; see
[TODO.zh-CN.md](TODO.zh-CN.md) for Chinese.

## Hardware

- [ ] **WLAN**: the MediaTek stack starts (`wmt_loader`, `wmt_launcher`,
      `wlan_assistant`), the kernel logs `WLAN-GEN4 driver init`, but no firmware
      is downloaded and `wlan0` never appears. Find the correct trigger for the
      firmware download (`ro.bootmode`? correct `WR-BUF:WIFI` format? vendor
      `wifi-service`?).
- [ ] **Vibration**: not investigated yet. Check `[haptics]` in the device
      config, the sysfs nodes and `Haptics::open`.
- [ ] **Decryption**: blocked on TEE. Keymaster blocks in `soter_open` while
      opening `/dev/isee_tee0`. Needs a working `teei_daemon`/TEE setup. Cannot
      be tested end-to-end until a data partition exists (the userdata partition
      was wiped for the iOS port).
- [ ] Verify touch, keys and the remaining sensors on the real device.

## UI

- [ ] **Clock position**: the clock is too close to the top-right camera cutout.
      Move it left.
- [ ] Add **Asia/Singapore** to `TIME_ZONES` in `crates/skyward-ui/src/app.rs`
      and cover it with a test.

## Features to verify

- [ ] Install zips, system update packages and partition images
- [ ] Backup and restore
- [ ] Wipe data / factory reset
- [ ] Mount, repair and format filesystems
- [ ] File manager and terminal
- [ ] MTP, adb and fastbootd on this device
- [ ] Reboot targets and slot handling (A-only)

## Build / repository

- [ ] Decide whether the retained `nezha` and `ishtar` configurations should stay
      in this fork.
- [ ] Add a reproducible stock recovery source for merlinx (currently packed from
      a recovery image supplied by hand).
