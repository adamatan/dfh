use std::collections::HashMap;
use std::io::IsTerminal;

#[derive(Debug, Clone)]
struct MountInfo {
    device: String,
    mount_point: String,
    fs_type: String,
    total: u64,
    used: u64,
    free: u64,
    inodes_total: u64,
    inodes_used: u64,
    inodes_free: u64,
}

/// All outputs are exactly 9 visible chars: "{:>6} {:<2}" e.g. "  3.14 GB" or "     0 B "
fn human_size(n: u64) -> String {
    const UNITS: &[&str] = &["B ", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{:>6} {}", n, UNITS[unit_idx])
    } else {
        format!("{:>6.2} {}", value, UNITS[unit_idx])
    }
}

fn human_size_colored(n: u64, color: bool) -> String {
    const UNITS: &[&str] = &["B ", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if !color {
        return human_size(n);
    }
    let color_code = match unit_idx {
        0 | 1 => "\x1b[34m", // blue: B/KB
        2 => "\x1b[36m",     // cyan: MB
        3 => "",             // default: GB
        _ => "\x1b[33m",     // yellow: TB+
    };
    let reset = if color_code.is_empty() { "" } else { "\x1b[0m" };
    if unit_idx == 0 {
        format!("{:>6} {}{}{}", n, color_code, UNITS[unit_idx], reset)
    } else {
        format!("{:>6.2} {}{}{}", value, color_code, UNITS[unit_idx], reset)
    }
}

fn render_bar(pct: f64, width: usize, color: bool) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    let bar_content = format!("{}{}", "#".repeat(filled), "-".repeat(empty));
    if color {
        let color_code = if pct >= 90.0 {
            "\x1b[31m" // red
        } else if pct >= 70.0 {
            "\x1b[33m" // yellow
        } else {
            "\x1b[32m" // green
        };
        format!("[{}{}{}]", color_code, bar_content, "\x1b[0m")
    } else {
        format!("[{}]", bar_content)
    }
}

/// disk3s1s1 -> disk3, disk3s7 -> disk3
#[cfg(target_os = "macos")]
fn container_disk(device: &str) -> &str {
    let name = device.strip_prefix("/dev/").unwrap_or(device);
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    &name[..i]
}

/// Find the dm-N block device name in /sys/block/ matching a device-mapper name
#[cfg(target_os = "linux")]
fn find_dm_block(mapper_name: &str) -> Option<String> {
    for entry in std::fs::read_dir("/sys/block/").ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("dm-") {
            continue;
        }
        let dm_name = std::fs::read_to_string(format!("/sys/block/{}/dm/name", name)).ok()?;
        if dm_name.trim() == mapper_name {
            return Some(name);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn resolve_dm_slave(mapper_name: &str) -> Option<String> {
    let dm_block = find_dm_block(mapper_name)?;
    let slaves_dir = format!("/sys/block/{}/slaves", dm_block);
    let mut slaves = std::fs::read_dir(&slaves_dir).ok()?;
    Some(
        slaves
            .next()?
            .ok()?
            .file_name()
            .to_string_lossy()
            .to_string(),
    )
}

/// Linux: strip partition suffix from a block device name
/// sda1 -> sda, mmcblk0p2 -> mmcblk0, nvme0n1p3 -> nvme0n1
#[cfg(target_os = "linux")]
fn strip_partition(dev: &str) -> &str {
    let bytes = dev.as_bytes();
    // mmcblk0p2, nvme0n1p3: digit then 'p' then digits
    if let Some(p_pos) = dev.rfind('p') {
        if p_pos > 0
            && p_pos + 1 < dev.len()
            && bytes[p_pos - 1].is_ascii_digit()
            && dev[p_pos + 1..].bytes().all(|b| b.is_ascii_digit())
        {
            return &dev[..p_pos];
        }
    }
    // sda1: strip trailing digits (but only if something remains)
    let mut i = dev.len();
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i > 0 && i < dev.len() {
        &dev[..i]
    } else {
        dev
    }
}

/// Linux: get the physical disk name for a device path
#[cfg(target_os = "linux")]
fn physical_disk(device: &str) -> String {
    let dev_name = device.strip_prefix("/dev/").unwrap_or(device);

    // Handle device-mapper: /dev/mapper/sda2_crypt -> resolve slave -> strip partition
    if let Some(mapper_name) = device.strip_prefix("/dev/mapper/") {
        if let Some(slave) = resolve_dm_slave(mapper_name) {
            return strip_partition(&slave).to_string();
        }
        return dev_name.to_string();
    }

    strip_partition(dev_name).to_string()
}

/// Read disk size from sysfs (returns bytes)
#[cfg(target_os = "linux")]
fn disk_size_bytes(disk: &str) -> Option<u64> {
    let sectors = std::fs::read_to_string(format!("/sys/block/{}/size", disk)).ok()?;
    let sectors: u64 = sectors.trim().parse().ok()?;
    Some(sectors * 512)
}

/// Read disk model from sysfs
#[cfg(target_os = "linux")]
fn disk_model(disk: &str) -> Option<String> {
    let model = std::fs::read_to_string(format!("/sys/block/{}/device/model", disk)).ok()?;
    let model = model.trim().to_string();
    if model.is_empty() {
        None
    } else {
        Some(model)
    }
}

#[cfg(target_os = "linux")]
fn is_encrypted(device: &str) -> bool {
    let mapper_name = match device.strip_prefix("/dev/mapper/") {
        Some(n) => n,
        None => return false,
    };
    let dm_block = match find_dm_block(mapper_name) {
        Some(b) => b,
        None => return false,
    };
    std::fs::read_to_string(format!("/sys/block/{}/dm/uuid", dm_block))
        .map(|uuid| uuid.trim().starts_with("CRYPT-"))
        .unwrap_or(false)
}

/// Friendly label for a disk based on its name
#[cfg(target_os = "linux")]
fn disk_label(disk: &str) -> String {
    let model = disk_model(disk);
    let size = disk_size_bytes(disk).map(human_size);

    let kind = if disk.starts_with("mmcblk") {
        "SD/eMMC"
    } else if disk.starts_with("nvme") {
        "NVMe"
    } else {
        "disk"
    };

    let desc = model.as_deref().unwrap_or(kind);
    match size {
        Some(s) => format!("{} ({})", desc, s),
        None => desc.to_string(),
    }
}

const VIRTUAL_FSTYPES: &[&str] = &[
    "sysfs",
    "proc",
    "devpts",
    "devtmpfs",
    "cgroup",
    "cgroup2",
    "pstore",
    "bpf",
    "debugfs",
    "tracefs",
    "configfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "securityfs",
    "efivarfs",
    "squashfs",
    "overlay",
    "autofs",
    "tmpfs",
    "ramfs",
    "nsfs",
    "rpc_pipefs",
    "nfsd",
    "fuse.portal",
    // macOS virtual filesystems
    "devfs",
    "autofs",
    "map",
    "nullfs",
    "fdesc",
];

fn is_real_drive(m: &MountInfo) -> bool {
    if m.total == 0 {
        return false;
    }
    if m.mount_point.starts_with("/System/Volumes/") {
        return false;
    }
    if VIRTUAL_FSTYPES.contains(&m.fs_type.as_str()) {
        return false;
    }
    true
}

#[cfg(target_os = "linux")]
fn get_mounts() -> Vec<MountInfo> {
    use std::ffi::CString;

    let content = match std::fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut mounts = Vec::new();
    let mut seen: std::collections::HashSet<(String, u64)> = std::collections::HashSet::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let device = parts[0].to_string();
        let mount_point = parts[1].to_string();
        let fs_type = parts[2].to_string();

        let path = match CString::new(mount_point.as_bytes()) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
        if ret != 0 {
            continue;
        }

        let frsize: u64 = stat.f_frsize.try_into().unwrap_or(0);
        let total = stat.f_blocks * frsize;
        let used = (stat.f_blocks.saturating_sub(stat.f_bfree)) * frsize;
        let free = stat.f_bavail * frsize;
        let inodes_total = stat.f_files;
        let inodes_free = stat.f_favail;
        let inodes_used = inodes_total.saturating_sub(stat.f_ffree);

        let key = (device.clone(), total);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);

        mounts.push(MountInfo {
            device,
            mount_point,
            fs_type,
            total,
            used,
            free,
            inodes_total,
            inodes_used,
            inodes_free,
        });
    }
    mounts
}

#[cfg(target_os = "linux")]
fn get_swap_entries() -> Vec<MountInfo> {
    let content = match std::fs::read_to_string("/proc/swaps") {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut entries = Vec::new();
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let device = parts[0].to_string();
        let swap_type = parts[1];
        let total_kb: u64 = parts[2].parse().unwrap_or(0);
        let used_kb: u64 = parts[3].parse().unwrap_or(0);

        let fs_type = if device.starts_with("/dev/zram") {
            "zram".to_string()
        } else {
            format!("swap({})", swap_type)
        };

        entries.push(MountInfo {
            device,
            mount_point: "[swap]".to_string(),
            fs_type,
            total: total_kb * 1024,
            used: used_kb * 1024,
            free: (total_kb - used_kb) * 1024,
            inodes_total: 0,
            inodes_used: 0,
            inodes_free: 0,
        });
    }
    entries
}

#[cfg(target_os = "macos")]
fn apfs_volume_used(mount: &str) -> Option<u64> {
    use std::process::Command;

    let output = Command::new("diskutil")
        .args(["info", mount])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Volume Used Space:") {
            // Format: "13.9 MB (13897728 Bytes) (exactly 27144 512-Byte-Units)"
            if let Some(bytes_start) = rest.find('(') {
                let after = &rest[bytes_start + 1..];
                let num: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(bytes) = num.parse::<u64>() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn get_mounts() -> Vec<MountInfo> {
    use std::ffi::CStr;

    let mut ptr: *mut libc::statfs = std::ptr::null_mut();
    let count = unsafe { libc::getmntinfo(&mut ptr, libc::MNT_NOWAIT) };
    if count <= 0 || ptr.is_null() {
        return vec![];
    }

    let entries = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
    let mut mounts = Vec::new();

    for entry in entries {
        let device = unsafe {
            CStr::from_ptr(entry.f_mntfromname.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        let mount_point = unsafe {
            CStr::from_ptr(entry.f_mntonname.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        let fs_type = unsafe {
            CStr::from_ptr(entry.f_fstypename.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        let bsize = entry.f_bsize as u64;
        let total = entry.f_blocks * bsize;
        let used = entry.f_blocks.saturating_sub(entry.f_bfree) * bsize;
        let free = entry.f_bavail * bsize;
        let inodes_total = entry.f_files;
        let inodes_used = entry.f_files.saturating_sub(entry.f_ffree);
        let inodes_free = entry.f_ffree;

        mounts.push(MountInfo {
            device,
            mount_point,
            fs_type,
            total,
            used,
            free,
            inodes_total,
            inodes_used,
            inodes_free,
        });
    }
    mounts
}

struct DriveRow {
    device: String,
    fs_type: String,
    total: u64,
    used: u64,
    free: u64,
    pct: f64,
    mount: String,
    encrypted: bool,
    inodes_total: u64,
    inodes_used: u64,
    inodes_free: u64,
}

struct SysRow {
    mount: String,
    fs_type: String,
    used: u64,
    purpose: Option<&'static str>,
}

fn purpose_for(mount: &str, fs_type: &str) -> Option<&'static str> {
    let exact = match mount {
        // macOS
        "/System/Volumes/Data" => Some("User data (Apple split system/data layout)"),
        "/System/Volumes/Data/home" => Some("Auto-mounted user home directories"),
        "/System/Volumes/Preboot" => Some("APFS boot files"),
        "/System/Volumes/Update" => Some("macOS update staging"),
        "/System/Volumes/VM" => Some("Swap files and sleep image"),
        "/System/Volumes/Hardware" => Some("Hardware-specific configuration"),
        "/System/Volumes/iSCPreboot" => Some("Apple Silicon internal storage preboot"),
        "/System/Volumes/xarts" => Some("Secure Enclave token storage"),
        "/private/var/vm" => Some("Swap files and sleep image"),
        // Linux
        "/proc" => Some("Kernel process info"),
        "/sys" => Some("Kernel and device info"),
        "/dev" => Some("Device files"),
        "/dev/pts" => Some("Pseudo-terminals"),
        "/dev/shm" => Some("Shared memory"),
        "/run" => Some("Runtime state"),
        "/tmp" => Some("Temporary files"),
        "/sys/fs/cgroup" => Some("Control group hierarchy"),
        "/sys/fs/bpf" => Some("BPF filesystem"),
        "/sys/kernel/debug" => Some("Kernel debug interface"),
        "/sys/kernel/tracing" => Some("Kernel tracing"),
        "/sys/kernel/security" => Some("Kernel security modules"),
        "/sys/firmware/efi/efivars" => Some("EFI variables"),
        "/proc/sys/fs/binfmt_misc" => Some("Binary format handlers"),
        "/boot/efi" => Some("EFI system partition"),
        "/boot" => Some("Boot partition"),
        "/boot/firmware" => Some("Boot firmware partition"),
        _ => None,
    };
    if exact.is_some() {
        return exact;
    }

    if mount.starts_with("/run/credentials/") {
        return Some("Service credentials");
    }
    if mount.starts_with("/snap/") || mount.starts_with("/var/lib/snapd/snap/") {
        return Some("Snap package mount");
    }

    // fstype match before broad prefix matches
    let by_type = match fs_type {
        "tmpfs" => Some("Temporary in-memory filesystem"),
        "devfs" => Some("Device files"),
        "autofs" => Some("Auto-mount point"),
        "squashfs" => Some("Compressed read-only filesystem"),
        "overlay" => Some("Overlay filesystem"),
        "cgroup" | "cgroup2" => Some("Control groups"),
        "nullfs" => Some("Null mount"),
        "fuse.portal" => Some("Flatpak portal"),
        "fuse.gvfsd-fuse" => Some("GNOME virtual filesystem"),
        "zram" => Some("Compressed RAM swap"),
        "swap(partition)" | "swap(file)" => Some("Swap space"),
        "mqueue" => Some("POSIX message queues"),
        "fusectl" => Some("FUSE control"),
        "pstore" => Some("Persistent storage for kernel panics"),
        "configfs" => Some("Kernel configuration"),
        "rpc_pipefs" => Some("NFS RPC pipe"),
        "binfmt_misc" => Some("Binary format handlers"),
        _ => None,
    };
    if by_type.is_some() {
        return by_type;
    }

    if mount.starts_with("/run/user/") {
        return Some("Per-user runtime directory");
    }

    None
}

fn make_sys_row(m: &MountInfo, used: u64) -> SysRow {
    SysRow {
        mount: m.mount_point.clone(),
        fs_type: m.fs_type.clone(),
        used,
        purpose: purpose_for(&m.mount_point, &m.fs_type),
    }
}

struct DiskGroup {
    disk: String,
    label: String,
    rows: Vec<DriveRow>,
}

fn print_section_header(title: &str, width: usize) {
    let w = width.max(40);
    println!("  {}", title);
    println!("  {}", "\u{2500}".repeat(w));
}

const LOCK: &str = " \u{1f512}";
const LOCK_DISPLAY_WIDTH: usize = 3; // space + emoji (2 cells)

fn device_display_len(r: &DriveRow) -> usize {
    if r.encrypted {
        r.device.len() + LOCK_DISPLAY_WIDTH
    } else {
        r.device.len()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn print_device_col(r: &DriveRow, fs_w: usize) {
    if r.encrypted {
        let max_name = fs_w.saturating_sub(LOCK_DISPLAY_WIDTH);
        let name = truncate(&r.device, max_name);
        let display_len = name.len() + LOCK_DISPLAY_WIDTH;
        let pad = fs_w.saturating_sub(display_len);
        print!("  {}{}{}", name, LOCK, " ".repeat(pad));
    } else {
        let name = truncate(&r.device, fs_w);
        print!("  {:<fs_w$}", name, fs_w = fs_w);
    }
}

fn print_group_header(group: &DiskGroup, color: bool) {
    if color {
        println!("  \x1b[1m{}\x1b[0m  {}", group.disk, group.label);
    } else {
        println!("  {}  {}", group.disk, group.label);
    }
}

fn print_drives(groups: &[DiskGroup], color: bool, inodes: bool, min_width: usize) -> usize {
    if groups.is_empty() {
        print_section_header("Drives", min_width);
        return min_width;
    }

    let all_rows: Vec<&DriveRow> = groups.iter().flat_map(|g| g.rows.iter()).collect();
    if all_rows.is_empty() {
        return 0;
    }

    let fs_max = 30;
    let fs_w = all_rows
        .iter()
        .map(|r| device_display_len(r))
        .max()
        .unwrap_or(10)
        .max(10)
        .min(fs_max);
    let type_w = all_rows
        .iter()
        .map(|r| r.fs_type.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let multi_disk = groups.len() > 1 || groups.first().is_some_and(|g| g.rows.len() > 1);

    if inodes {
        let itotal_w = all_rows
            .iter()
            .map(|r| human_size(r.inodes_total).len())
            .max()
            .unwrap_or(6)
            .max(6);
        let iused_w = all_rows
            .iter()
            .map(|r| human_size(r.inodes_used).len())
            .max()
            .unwrap_or(5)
            .max(5);
        let ifree_w = all_rows
            .iter()
            .map(|r| human_size(r.inodes_free).len())
            .max()
            .unwrap_or(5)
            .max(5);
        let content_w =
            fs_w + 2 + type_w + 2 + itotal_w + 2 + iused_w + 2 + ifree_w + 2 + 22 + 2 + 5 + 2 + 5;

        print_section_header("Drives", content_w.max(min_width));

        for (gi, group) in groups.iter().enumerate() {
            if multi_disk {
                if gi > 0 {
                    println!();
                }
                print_group_header(group, color);
            }
            if gi == 0 || multi_disk {
                println!(
                    "  {:<fs_w$}  {:<type_w$}  {:>itotal_w$}  {:>iused_w$}  {:>ifree_w$}  {:<22}  {:<5}  MOUNT",
                    "FILESYSTEM", "TYPE", "INODES", "IUSED", "IFREE", "USAGE", "USE%",
                    fs_w = fs_w, type_w = type_w, itotal_w = itotal_w,
                    iused_w = iused_w, ifree_w = ifree_w
                );
            }
            for r in &group.rows {
                let bar = render_bar(r.pct, 20, color);
                print_device_col(r, fs_w);
                println!(
                    "  {:<type_w$}  {:>itotal_w$}  {:>iused_w$}  {:>ifree_w$}  {}  {:>4.0}%  {}",
                    r.fs_type,
                    human_size_colored(r.inodes_total, color),
                    human_size_colored(r.inodes_used, color),
                    human_size_colored(r.inodes_free, color),
                    bar,
                    r.pct,
                    r.mount,
                    type_w = type_w,
                    itotal_w = itotal_w,
                    iused_w = iused_w,
                    ifree_w = ifree_w
                );
            }
        }
        content_w
    } else {
        let size_w = all_rows
            .iter()
            .map(|r| human_size(r.total).len())
            .max()
            .unwrap_or(4)
            .max(4);
        let used_w = all_rows
            .iter()
            .map(|r| human_size(r.used).len())
            .max()
            .unwrap_or(4)
            .max(4);
        let avail_w = all_rows
            .iter()
            .map(|r| human_size(r.free).len())
            .max()
            .unwrap_or(5)
            .max(5);
        let content_w =
            fs_w + 2 + type_w + 2 + size_w + 2 + used_w + 2 + avail_w + 2 + 22 + 2 + 5 + 2 + 5;

        print_section_header("Drives", content_w.max(min_width));

        for (gi, group) in groups.iter().enumerate() {
            if multi_disk {
                if gi > 0 {
                    println!();
                }
                print_group_header(group, color);
            }
            if gi == 0 {
                println!(
                    "  {:<fs_w$}  {:<type_w$}  {:>size_w$}  {:>used_w$}  {:>avail_w$}  {:<22}  {:<5}  MOUNT",
                    "FILESYSTEM", "TYPE", "SIZE", "USED", "AVAIL", "USAGE", "USE%",
                    fs_w = fs_w, type_w = type_w, size_w = size_w,
                    used_w = used_w, avail_w = avail_w
                );
            }
            for r in &group.rows {
                let bar = render_bar(r.pct, 20, color);
                print_device_col(r, fs_w);
                println!(
                    "  {:<type_w$}  {:>size_w$}  {:>used_w$}  {:>avail_w$}  {}  {:>4.0}%  {}",
                    r.fs_type,
                    human_size_colored(r.total, color),
                    human_size_colored(r.used, color),
                    human_size_colored(r.free, color),
                    bar,
                    r.pct,
                    r.mount,
                    type_w = type_w,
                    size_w = size_w,
                    used_w = used_w,
                    avail_w = avail_w
                );
            }
        }
        content_w
    }
}

fn sys_content_width(rows: &[SysRow]) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let mount_w = rows.iter().map(|r| r.mount.len()).max().unwrap_or(5).max(5);
    let type_w = rows
        .iter()
        .map(|r| r.fs_type.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let used_w = rows
        .iter()
        .map(|r| human_size(r.used).len())
        .max()
        .unwrap_or(4)
        .max(4);
    let purpose_w = rows
        .iter()
        .map(|r| r.purpose.map_or("(user mount)".len(), |p| p.len()))
        .max()
        .unwrap_or(7);
    mount_w + 2 + type_w + 2 + used_w + 2 + purpose_w
}

fn is_swap_row(r: &SysRow) -> bool {
    r.mount == "[swap]" || r.fs_type.starts_with("swap(") || r.fs_type == "zram"
}

fn print_sys_volumes(rows: &[SysRow], color: bool) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let mount_w = rows.iter().map(|r| r.mount.len()).max().unwrap_or(5).max(5);
    let type_w = rows
        .iter()
        .map(|r| r.fs_type.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let used_w = rows
        .iter()
        .map(|r| human_size(r.used).len())
        .max()
        .unwrap_or(4)
        .max(4);
    let content_w = mount_w + 2 + type_w + 2 + used_w + 2 + 7;

    println!(
        "  {:<mount_w$}  {:<type_w$}  {:>used_w$}  PURPOSE",
        "MOUNT",
        "TYPE",
        "USED",
        mount_w = mount_w,
        type_w = type_w,
        used_w = used_w
    );
    for r in rows {
        let (purpose, is_user) = match r.purpose {
            Some(p) => (p.to_string(), false),
            None => ("(user mount)".to_string(), true),
        };
        let is_swap = is_swap_row(r);
        let line = format!(
            "  {:<mount_w$}  {:<type_w$}  {:>used_w$}  {}",
            r.mount,
            r.fs_type,
            human_size_colored(r.used, color),
            purpose,
            mount_w = mount_w,
            type_w = type_w,
            used_w = used_w
        );
        if color {
            if is_swap {
                println!("\x1b[1;33m{}\x1b[0m", line);
            } else if is_user {
                println!("\x1b[36m{}\x1b[0m", line);
            } else {
                println!("\x1b[2m{}\x1b[0m", line);
            }
        } else {
            println!("{}", line);
        }
    }
    content_w
}

fn render(drive_rows: &[DiskGroup], sys_rows: &[SysRow], color: bool, inodes: bool) {
    println!();
    let sys_w = sys_content_width(sys_rows);
    let drives_w = print_drives(drive_rows, color, inodes, sys_w);
    println!();

    if !sys_rows.is_empty() {
        print_section_header("System Volumes", drives_w);
        print_sys_volumes(sys_rows, color);
        println!();
    }
}

#[cfg(debug_assertions)]
mod demo {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    const TB: u64 = 1024 * GB;
    const KB: u64 = 1024;

    fn dr(
        device: &str,
        fs_type: &str,
        total: u64,
        used: u64,
        mount: &str,
        encrypted: bool,
    ) -> DriveRow {
        let pct = if total > 0 {
            used as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        DriveRow {
            device: device.to_string(),
            fs_type: fs_type.to_string(),
            total,
            used,
            free: total.saturating_sub(used),
            pct,
            mount: mount.to_string(),
            encrypted,
            inodes_total: 0,
            inodes_used: 0,
            inodes_free: 0,
        }
    }

    fn sr(mount: &str, fs_type: &str, used: u64) -> SysRow {
        SysRow {
            mount: mount.to_string(),
            fs_type: fs_type.to_string(),
            used,
            purpose: purpose_for(mount, fs_type),
        }
    }

    fn scenario_macbook() -> (Vec<DiskGroup>, Vec<SysRow>) {
        let drives = vec![DiskGroup {
            disk: "disk2".into(),
            label: format!("({})", human_size(TB)),
            rows: vec![dr("/dev/disk2", "apfs", 926 * GB, 614 * GB, "/", false)],
        }];
        let sys = vec![
            sr("/System/Volumes/Data", "apfs", 580 * GB),
            sr("/System/Volumes/Data/home", "autofs", 0),
            sr("/System/Volumes/Hardware", "apfs", 5 * MB),
            sr("/System/Volumes/Preboot", "apfs", 6 * GB + 200 * MB),
            sr("/System/Volumes/Update", "apfs", 22 * MB),
            sr("/System/Volumes/VM", "apfs", 8 * GB),
            sr("/System/Volumes/iSCPreboot", "apfs", 4 * MB),
            sr("/System/Volumes/xarts", "apfs", 7 * MB),
            sr("/dev", "devfs", 245 * KB),
        ];
        (drives, sys)
    }

    fn scenario_homelab() -> (Vec<DiskGroup>, Vec<SysRow>) {
        let drives = vec![
            DiskGroup {
                disk: "mmcblk0".into(),
                label: "SD/eMMC (62.54 GB)".into(),
                rows: vec![
                    dr(
                        "/dev/mmcblk0p2",
                        "ext4",
                        58 * GB,
                        12 * GB + 300 * MB,
                        "/",
                        false,
                    ),
                    dr(
                        "/dev/mmcblk0p1",
                        "vfat",
                        256 * MB,
                        52 * MB,
                        "/boot/firmware",
                        false,
                    ),
                ],
            },
            DiskGroup {
                disk: "sda".into(),
                label: "Elements 25A3 (3.64 TB)".into(),
                rows: vec![dr(
                    "/dev/mapper/sda1_crypt",
                    "btrfs",
                    3 * TB + 600 * GB,
                    TB + 200 * GB,
                    "/mnt/storage",
                    true,
                )],
            },
        ];
        let sys = vec![
            sr("/dev", "devtmpfs", 0),
            sr("/dev/shm", "tmpfs", 180 * KB),
            sr("/proc", "proc", 0),
            sr("/run", "tmpfs", 9 * MB),
            sr("/run/lock", "tmpfs", 16 * KB),
            sr("/sys", "sysfs", 0),
            sr("/sys/fs/cgroup", "cgroup2", 0),
            sr("/tmp", "tmpfs", 2 * MB + 100 * KB),
            SysRow {
                mount: "[swap]".into(),
                fs_type: "zram".into(),
                used: 310 * MB,
                purpose: Some("Compressed RAM swap"),
            },
        ];
        (drives, sys)
    }

    fn scenario_linux_server() -> (Vec<DiskGroup>, Vec<SysRow>) {
        let drives = vec![
            DiskGroup {
                disk: "nvme0n1".into(),
                label: "CT2000P5PSSD8 (1.82 TB)".into(),
                rows: vec![
                    dr(
                        "/dev/nvme0n1p3",
                        "ext4",
                        TB + 700 * GB,
                        890 * GB,
                        "/",
                        false,
                    ),
                    dr(
                        "/dev/nvme0n1p1",
                        "vfat",
                        512 * MB,
                        8 * MB,
                        "/boot/efi",
                        false,
                    ),
                ],
            },
            DiskGroup {
                disk: "sda".into(),
                label: "ST8000VN004 (7.28 TB)".into(),
                rows: vec![dr(
                    "/dev/mapper/vg0-data",
                    "xfs",
                    7 * TB + 200 * GB,
                    4 * TB + 800 * GB,
                    "/srv/data",
                    true,
                )],
            },
            DiskGroup {
                disk: "sdb".into(),
                label: "ST8000VN004 (7.28 TB)".into(),
                rows: vec![dr(
                    "/dev/mapper/vg1-backup",
                    "xfs",
                    7 * TB + 200 * GB,
                    2 * TB + 100 * GB,
                    "/srv/backup",
                    true,
                )],
            },
        ];
        let sys = vec![
            sr("/dev", "devtmpfs", 0),
            sr("/dev/shm", "tmpfs", 3 * GB + 400 * MB),
            sr("/proc", "proc", 0),
            sr("/run", "tmpfs", GB + 200 * MB),
            sr("/sys", "sysfs", 0),
            sr("/tmp", "tmpfs", 245 * MB),
            SysRow {
                mount: "[swap]".into(),
                fs_type: "swap(partition)".into(),
                used: 2 * GB + 400 * MB,
                purpose: Some("Swap space"),
            },
        ];
        (drives, sys)
    }

    fn scenario_minimal_vm() -> (Vec<DiskGroup>, Vec<SysRow>) {
        let drives = vec![DiskGroup {
            disk: "vda".into(),
            label: "disk (30.00 GB)".into(),
            rows: vec![dr(
                "/dev/vda1",
                "ext4",
                29 * GB,
                27 * GB + 300 * MB,
                "/",
                false,
            )],
        }];
        let sys = vec![
            sr("/dev", "devtmpfs", 0),
            sr("/proc", "proc", 0),
            sr("/run", "tmpfs", 38 * MB),
            sr("/sys", "sysfs", 0),
            SysRow {
                mount: "[swap]".into(),
                fs_type: "swap(file)".into(),
                used: 780 * MB,
                purpose: Some("Swap space"),
            },
        ];
        (drives, sys)
    }

    pub fn run() {
        let color = std::io::stdout().is_terminal();
        type Scenario = (&'static str, fn() -> (Vec<DiskGroup>, Vec<SysRow>));
        let scenarios: Vec<Scenario> = vec![
            ("MacBook (Apple Silicon, 1TB)", scenario_macbook),
            ("SBC with external USB drive", scenario_homelab),
            ("Linux server (NVMe + 2x HDD)", scenario_linux_server),
            ("Cloud VM (almost full)", scenario_minimal_vm),
        ];

        for (name, builder) in &scenarios {
            if color {
                println!("\x1b[1;35m  === {} ===\x1b[0m", name);
            } else {
                println!("  === {} ===", name);
            }
            let (drives, sys) = builder();
            render(&drives, &sys, color, false);
            println!();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--demo") {
        demo::run();
        return;
    }

    let no_color = args.iter().any(|a| a == "--no-color");
    let inodes = args.iter().any(|a| a == "-i" || a == "--inodes");

    let color = !no_color && std::io::stdout().is_terminal();

    let all_mounts = get_mounts();

    #[cfg(target_os = "macos")]
    let used_for = |m: &MountInfo| -> u64 {
        if m.fs_type == "apfs" {
            if let Some(u) = apfs_volume_used(&m.mount_point) {
                return u;
            }
        }
        m.used
    };
    #[cfg(not(target_os = "macos"))]
    let used_for = |m: &MountInfo| -> u64 { m.used };

    let (real, system_raw): (Vec<_>, Vec<_>) = all_mounts.into_iter().partition(is_real_drive);

    let mut sys_rows: Vec<SysRow> = system_raw
        .iter()
        .map(|m| make_sys_row(m, used_for(m)))
        .collect();

    #[cfg(target_os = "macos")]
    let drive_rows = {
        let mut containers: HashMap<String, Vec<MountInfo>> = HashMap::new();
        for m in real {
            let c = container_disk(&m.device).to_string();
            containers.entry(c).or_default().push(m);
        }

        let mut groups: Vec<DiskGroup> = Vec::new();

        for (container, mut vols) in containers {
            let has_user_mount = vols
                .iter()
                .any(|v| !v.mount_point.starts_with("/System/Volumes/"));
            if !has_user_mount {
                for v in vols {
                    sys_rows.push(make_sys_row(&v, used_for(&v)));
                }
                continue;
            }

            vols.sort_by(|a, b| a.mount_point.len().cmp(&b.mount_point.len()));
            let rep_idx = vols
                .iter()
                .position(|v| v.mount_point == "/")
                .or_else(|| {
                    vols.iter()
                        .position(|v| !v.mount_point.starts_with("/System/Volumes/"))
                })
                .unwrap_or(0);

            let rep = vols.remove(rep_idx);

            for v in vols {
                sys_rows.push(make_sys_row(&v, used_for(&v)));
            }

            let total = rep.total;
            let used = rep.used;
            let pct = if total > 0 {
                used as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let label = format!("({})", human_size(total));

            groups.push(DiskGroup {
                disk: container.clone(),
                label,
                rows: vec![DriveRow {
                    device: format!("/dev/{}", container),
                    fs_type: rep.fs_type,
                    total,
                    used,
                    free: rep.free,
                    pct,
                    mount: rep.mount_point,
                    encrypted: false,
                    inodes_total: rep.inodes_total,
                    inodes_used: rep.inodes_used,
                    inodes_free: rep.inodes_free,
                }],
            });
        }

        groups.sort_by(|a, b| {
            a.rows
                .first()
                .map(|r| r.mount.as_str())
                .cmp(&b.rows.first().map(|r| r.mount.as_str()))
        });
        groups
    };

    #[cfg(target_os = "linux")]
    let drive_rows = {
        let rows: Vec<(String, DriveRow)> = real
            .into_iter()
            .map(|m| {
                let disk = physical_disk(&m.device);
                let total = m.total;
                let used = m.used;
                let pct = if total > 0 {
                    used as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                let encrypted = is_encrypted(&m.device);
                (
                    disk,
                    DriveRow {
                        device: m.device,
                        fs_type: m.fs_type,
                        total,
                        used,
                        free: m.free,
                        pct,
                        mount: m.mount_point,
                        encrypted,
                        inodes_total: m.inodes_total,
                        inodes_used: m.inodes_used,
                        inodes_free: m.inodes_free,
                    },
                )
            })
            .collect();

        let mut disk_order: Vec<String> = Vec::new();
        let mut groups_map: HashMap<String, Vec<DriveRow>> = HashMap::new();
        for (disk, row) in rows {
            if !groups_map.contains_key(&disk) {
                disk_order.push(disk.clone());
            }
            groups_map.entry(disk).or_default().push(row);
        }

        let mut groups: Vec<DiskGroup> = disk_order
            .into_iter()
            .map(|disk| {
                let mut rows = groups_map.remove(&disk).unwrap();
                rows.sort_by(|a, b| a.mount.cmp(&b.mount));
                let label = disk_label(&disk);
                DiskGroup { disk, label, rows }
            })
            .collect();
        groups.sort_by(|a, b| {
            a.rows
                .first()
                .map(|r| r.mount.as_str())
                .cmp(&b.rows.first().map(|r| r.mount.as_str()))
        });
        groups
    };

    #[cfg(target_os = "linux")]
    {
        for m in get_swap_entries() {
            sys_rows.push(SysRow {
                mount: m.mount_point.clone(),
                fs_type: m.fs_type.clone(),
                used: m.used,
                purpose: purpose_for(&m.mount_point, &m.fs_type),
            });
        }
    }

    sys_rows.sort_by(|a, b| a.mount.cmp(&b.mount));

    render(&drive_rows, &sys_rows, color, inodes);
}
