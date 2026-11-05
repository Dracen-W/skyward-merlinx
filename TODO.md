# TODO

Unfinished work on this fork. This is the English version; see
[TODO.zh-CN.md](TODO.zh-CN.md) for Chinese.

## Hardware

- [ ] **WLAN**: the MediaTek stack starts (`wmt_loader`, `wmt_launcher`,
      `wlan_assistant`), the kernel logs `WLAN-GEN4 driver init`, but no firmware
      is downloaded and `wlan0` never appears. The launcher now reports
      `vendor.connsys.formeta.ready=yes` and the assistant waits for it before
      writing NVRAM and `WIFI`; this still needs testing on the phone.
- [ ] **Vibration**: the motor is MediaTek's LED class device at
      `/sys/class/leds/vibrator`, and `[haptics]` now drives it with `duration`
      and `activate`. Needs testing on the phone (a click is 15–30 ms).
- [ ] **Decryption**: blocked on TEE. Keymaster blocks in `soter_open` while
      opening `/dev/isee_tee0`. Needs a working `teei_daemon`/TEE setup. Cannot
      be tested end-to-end until a data partition exists (the userdata partition
      was wiped for the iOS port).
- [ ] Verify touch, keys and the remaining sensors on the real device.

## UI

- [x] **Clock position**: moved to 40 dp from the left, clear of merlinx's
      top-left camera hole.
- [x] Add **Asia/Singapore** to `TIME_ZONES` in `crates/skyward-ui/src/app.rs`
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
