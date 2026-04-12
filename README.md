# dfh

[![Test and Release](https://github.com/adamatan/dfh/actions/workflows/test_and_release.yml/badge.svg)](https://github.com/adamatan/dfh/actions/workflows/test_and_release.yml)

Human-readable disk usage with colorized bars, physical disk grouping, and system volume annotations. A `df -h` replacement for people who want to understand their storage at a glance.

## Install

```bash
cargo install dfh
```

## What it looks like

```
  Drives
  ──────────────────────────────────────────────────────────────────────────────────────────────
  nvme0n1  CT2000P5PSSD8 (1.82 TB)
  FILESYSTEM                 TYPE       SIZE       USED      AVAIL  USAGE              USE%  MOUNT
  /dev/nvme0n1p3             ext4    1.68 TB  890.00 GB  834.00 GB  [##########------]   52%  /
  /dev/nvme0n1p1             vfat  512.00 MB    8.00 MB  504.00 MB  [----------------]    2%  /boot/efi

  sda  ST8000VN004 (7.28 TB)
  /dev/mapper/vg0-data 🔒    xfs     7.20 TB    4.78 TB    2.41 TB  [#############---]   66%  /srv/data

  System Volumes
  ──────────────────────────────────────────────────────────────────────────────────────────────
  MOUNT     TYPE              USED  PURPOSE
  /dev      devtmpfs          0 B   Device files
  /dev/shm  tmpfs          3.39 GB  Shared memory
  /run      tmpfs          1.20 GB  Runtime state
  [swap]    swap(partition) 2.39 GB  Swap space
```

## Features

- **Physical disk grouping**: partitions on the same disk are shown under a shared header with the disk model and total size
- **APFS container dedup**: on macOS, APFS volumes sharing a container are collapsed to one row
- **System volume annotations**: every mount gets a PURPOSE column explaining what it is (swap, preboot, shared memory, etc.)
- **LUKS encryption indicator**: encrypted volumes show a lock icon (🔒)
- **Colorized usage bars**: green < 70%, yellow 70-89%, red >= 90%
- **Colored size units**: KB (blue), MB (cyan), GB (default), TB (yellow) for instant magnitude recognition
- **Swap detection**: Linux swap (partition, file, zram) shown with usage
- **Per-volume APFS sizes**: real per-volume used space via `diskutil`, not the misleading container-level numbers from `statfs`
- **Zero dependencies** beyond `libc`

## Usage

```
dfh              # normal view
dfh --no-color   # disable ANSI colors
dfh -i           # inode stats instead of bytes
```

## Supported platforms

- **macOS**: APFS, HFS+, exFAT, FAT32
- **Linux**: ext4, btrfs, xfs, exFAT, FAT32, LUKS/device-mapper, zram swap

## Development

```bash
cargo build
cargo run -- --demo    # show demo scenarios (debug builds only)
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

## License

[MIT](LICENSE)
