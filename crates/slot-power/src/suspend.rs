//! Hardware Suspend-to-RAM (S3) with optional RTC wakealarm for auto power-off.
//!
//! Used on chips with working S3 (`a133p` / TrimUI Brick & Smart Pro):
//! - Clears and optionally programs `/sys/class/rtc/rtc0/wakealarm` so the RTC wakes the device
//!   after the configured auto power-off timeout.
//! - Creates `/tmp/system_suspend` around the `mem` transition so `trimui_inputd` pauses its
//!   backlight workaround (matching TrimUI stock `keymon` and NextUI).
//! - Syncs filesystems and writes `"mem"` to `/sys/power/state`.
//! - On resume, classifies whether the wakeup came from the RTC alarm, the power button
//!   (`axp20x-pek`), or another IRQ (such as USB plug/unplug).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const SUSPEND_FLAG: &str = "/tmp/system_suspend";
const PROC_INTERRUPTS: &str = "/proc/interrupts";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WakeReason {
    /// The RTC wakealarm fired (or elapsed RTC time reached the programmed deadline).
    Alarm,
    /// The power button (AXP PEK) woke the device.
    PowerKey,
    /// Another hardware IRQ (such as USB plug/unplug) woke the kernel before the deadline.
    Other,
    /// S3 suspend is unsupported or failed; caller should fall back to light doze.
    Unsupported,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct IrqSnapshot {
    pub rtc_irq: Option<u32>,
    pub rtc_count: u64,
    pub pek_count: u64,
}

/// Parses `/proc/interrupts` for the RTC alarm IRQ and AXP power-key (PEK) IRQ counts.
pub fn parse_interrupts(text: &str) -> IrqSnapshot {
    let mut snap = IrqSnapshot::default();
    for line in text.lines() {
        let Some((head, rest)) = line.split_once(':') else {
            continue;
        };
        let Ok(irq) = head.trim().parse::<u32>() else {
            continue;
        };
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let count: u64 = tokens
            .iter()
            .take_while(|t| t.chars().all(|c| c.is_ascii_digit()))
            .filter_map(|t| t.parse::<u64>().ok())
            .sum();
        let label = tokens.last().copied().unwrap_or("");
        if label.eq_ignore_ascii_case("rtc") || label.to_ascii_lowercase().starts_with("sunxi-rtc")
        {
            snap.rtc_irq = Some(irq);
            snap.rtc_count = snap.rtc_count.saturating_add(count);
        } else if label.contains("pek") || line.contains("axp20x-pek") {
            snap.pek_count = snap.pek_count.saturating_add(count);
        }
    }
    snap
}

/// Classifies why the kernel returned from `/sys/power/state`.
pub fn classify_wake(
    alarm: Option<u64>,
    rtc_after: Option<u64>,
    wakeup_irq: Option<u32>,
    before: IrqSnapshot,
    after: IrqSnapshot,
) -> WakeReason {
    if let Some(target) = alarm {
        let rtc_hit_deadline = rtc_after.is_some_and(|now| now >= target);
        let rtc_irq_incremented = after.rtc_count > before.rtc_count;
        let wakeup_is_rtc = wakeup_irq.is_some() && wakeup_irq == before.rtc_irq.or(after.rtc_irq);
        if rtc_hit_deadline || rtc_irq_incremented || wakeup_is_rtc {
            return WakeReason::Alarm;
        }
    }
    if after.pek_count > before.pek_count {
        return WakeReason::PowerKey;
    }
    // If the kernel doesn't expose AXP PEK or RTC IRQ lines in /proc/interrupts, assume user wake.
    if before.rtc_irq.is_none() && after.rtc_irq.is_none() && before.pek_count == 0 {
        return WakeReason::PowerKey;
    }
    WakeReason::Other
}

/// Enters S3 Suspend-to-RAM using standard `/sys`, `/proc/interrupts`, and `/tmp/system_suspend`.
/// Returns `(reason, elapsed_rtc_secs)`.
pub fn suspend_to_ram(sysfs: &Path, timeout: Option<Duration>) -> (WakeReason, u64) {
    suspend_to_ram_at(
        sysfs,
        Path::new(PROC_INTERRUPTS),
        Path::new(SUSPEND_FLAG),
        timeout,
        true,
    )
}

/// Configurable entry point for S3 Suspend-to-RAM (used directly by unit tests).
pub fn suspend_to_ram_at(
    sysfs: &Path,
    interrupts_path: &Path,
    suspend_flag: &Path,
    timeout: Option<Duration>,
    sync_fs: bool,
) -> (WakeReason, u64) {
    let state_path = sysfs.join("power/state");
    let Ok(states) = fs::read_to_string(&state_path) else {
        return (WakeReason::Unsupported, 0);
    };
    if !states.split_whitespace().any(|s| s == "mem") {
        return (WakeReason::Unsupported, 0);
    }

    let rtc_dir = find_rtc(sysfs);
    let wakealarm_path = rtc_dir.as_ref().map(|d| d.join("wakealarm"));
    let epoch_path = rtc_dir.as_ref().map(|d| d.join("since_epoch"));

    let rtc_before = epoch_path.as_deref().and_then(read_u64);
    let alarm = match (timeout, rtc_before, wakealarm_path.as_deref()) {
        (Some(dur), Some(now), Some(alarm_file)) => {
            let secs = dur.as_secs().max(1);
            let target = now.saturating_add(secs);
            if fs::write(alarm_file, "0\n").is_err()
                || fs::write(alarm_file, format!("{target}\n")).is_err()
            {
                eprintln!(
                    "slot: s3: failed to arm {}, falling back to light doze",
                    alarm_file.display()
                );
                return (WakeReason::Unsupported, 0);
            }
            Some(target)
        }
        (Some(_), _, _) => {
            eprintln!("slot: s3: no RTC wakealarm available for timed power-off");
            return (WakeReason::Unsupported, 0);
        }
        (None, _, Some(alarm_file)) => {
            let _ = fs::write(alarm_file, "0\n");
            None
        }
        (None, _, None) => None,
    };

    let irqs_before = fs::read_to_string(interrupts_path)
        .map(|s| parse_interrupts(&s))
        .unwrap_or_default();

    let _ = fs::write(suspend_flag, b"");
    if sync_fs {
        let _ = Command::new("sync").status();
    }

    let mut wlan_quiesced = false;
    let mut write_res = fs::write(&state_path, "mem\n");
    if is_ebusy(&write_res) {
        // xradio_wlan's wiphy_suspend can transiently return -EBUSY while a Wi-Fi transaction is
        // in flight. Retry briefly, and if still busy quiesce wlan0 across S3 so suspend succeeds.
        for attempt in 0..3 {
            std::thread::sleep(Duration::from_millis(400));
            if attempt == 1 && !wlan_quiesced && wlan_is_up(sysfs) {
                let _ = Command::new("ifconfig").args(["wlan0", "down"]).status();
                wlan_quiesced = true;
                std::thread::sleep(Duration::from_millis(200));
            }
            write_res = fs::write(&state_path, "mem\n");
            if !is_ebusy(&write_res) {
                break;
            }
        }
    }

    if wlan_quiesced {
        let _ = Command::new("ifconfig").args(["wlan0", "up"]).status();
    }
    let _ = fs::remove_file(suspend_flag);
    if let Some(alarm_file) = wakealarm_path.as_deref() {
        let _ = fs::write(alarm_file, "0\n");
    }

    if let Err(e) = write_res {
        eprintln!("slot: s3: {} write failed: {e}", state_path.display());
        return (WakeReason::Unsupported, 0);
    }

    let rtc_after = epoch_path.as_deref().and_then(read_u64);
    let elapsed = match (rtc_before, rtc_after) {
        (Some(b), Some(a)) => a.saturating_sub(b),
        _ => 0,
    };
    let wakeup_irq = read_u32(&sysfs.join("power/pm_wakeup_irq"));
    let irqs_after = fs::read_to_string(interrupts_path)
        .map(|s| parse_interrupts(&s))
        .unwrap_or_default();

    let reason = classify_wake(alarm, rtc_after, wakeup_irq, irqs_before, irqs_after);
    (reason, elapsed)
}

fn find_rtc(sysfs: &Path) -> Option<PathBuf> {
    let dir = sysfs.join("class/rtc");
    let mut names: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    names.sort();
    names.into_iter().next()
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_u32(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn is_ebusy(res: &std::io::Result<()>) -> bool {
    matches!(res, Err(e) if e.raw_os_error() == Some(16))
}

fn wlan_is_up(sysfs: &Path) -> bool {
    let flags = fs::read_to_string(sysfs.join("class/net/wlan0/flags")).ok();
    flags
        .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .is_some_and(|f| f & 1 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRICK_INTERRUPTS_BEFORE: &str = "\
344:          0          0          0          0  wakeupgen 108 Level     rtc
380:         13          0          0          0  sunxi-8i-nmi   0 Level     axp2202_irq_chip
388:          0          0          0          1  axp2202_irq_chip   7 Edge      vbus_insert
394:          0          0          3          0  axp2202_irq_chip  13 Edge      axp20x-pek-dbf
395:          0          0          3          0  axp2202_irq_chip  14 Edge      axp20x-pek-dbr
";

    #[test]
    fn parses_brick_interrupts() {
        let s = parse_interrupts(BRICK_INTERRUPTS_BEFORE);
        assert_eq!(s.rtc_irq, Some(344));
        assert_eq!(s.rtc_count, 0);
        assert_eq!(s.pek_count, 6);
    }

    #[test]
    fn classifies_rtc_alarm_by_deadline_irq_or_wakeup_irq() {
        let before = parse_interrupts(BRICK_INTERRUPTS_BEFORE);
        let mut after_rtc = before;
        after_rtc.rtc_count = 1;

        assert_eq!(
            classify_wake(Some(1000), Some(1002), Some(344), before, after_rtc),
            WakeReason::Alarm
        );
        // Even if rtc_after lagged by 1s, rtc IRQ firing still classifies as Alarm.
        assert_eq!(
            classify_wake(Some(1000), Some(999), Some(344), before, after_rtc),
            WakeReason::Alarm
        );
        // Even if pm_wakeup_irq is empty, reaching the deadline classifies as Alarm.
        assert_eq!(
            classify_wake(Some(1000), Some(1000), None, before, before),
            WakeReason::Alarm
        );
    }

    #[test]
    fn classifies_power_key_and_spurious_usb_wake() {
        let before = parse_interrupts(BRICK_INTERRUPTS_BEFORE);
        let mut after_pek = before;
        after_pek.pek_count += 1;

        assert_eq!(
            classify_wake(Some(1000), Some(950), Some(380), before, after_pek),
            WakeReason::PowerKey
        );
        // USB plug wakes AXP NMI (380) without incrementing PEK or hitting RTC deadline.
        assert_eq!(
            classify_wake(Some(1000), Some(950), Some(380), before, before),
            WakeReason::Other
        );
        // Untimed sleep still wakes cleanly on PowerKey.
        assert_eq!(
            classify_wake(None, Some(950), Some(380), before, after_pek),
            WakeReason::PowerKey
        );
    }

    #[test]
    fn suspend_arms_and_clears_wakealarm_and_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let sysfs = tmp.path().join("sys");
        fs::create_dir_all(sysfs.join("power")).unwrap();
        fs::create_dir_all(sysfs.join("class/rtc/rtc0")).unwrap();
        fs::write(sysfs.join("power/state"), "freeze mem\n").unwrap();
        fs::write(sysfs.join("power/pm_wakeup_irq"), "344\n").unwrap();
        fs::write(sysfs.join("class/rtc/rtc0/since_epoch"), "1000\n").unwrap();
        fs::write(sysfs.join("class/rtc/rtc0/wakealarm"), "").unwrap();
        let interrupts = tmp.path().join("interrupts");
        fs::write(&interrupts, BRICK_INTERRUPTS_BEFORE).unwrap();
        let flag = tmp.path().join("system_suspend");

        let (reason, elapsed) = suspend_to_ram_at(
            &sysfs,
            &interrupts,
            &flag,
            Some(Duration::from_secs(180)),
            false,
        );
        assert_eq!(reason, WakeReason::Alarm);
        assert_eq!(elapsed, 0);
        assert_eq!(
            fs::read_to_string(sysfs.join("power/state")).unwrap(),
            "mem\n"
        );
        assert_eq!(
            fs::read_to_string(sysfs.join("class/rtc/rtc0/wakealarm")).unwrap(),
            "0\n"
        );
        assert!(!flag.exists(), "suspend flag must be cleaned up on resume");
    }

    #[test]
    fn returns_unsupported_when_mem_not_available() {
        let tmp = tempfile::tempdir().unwrap();
        let sysfs = tmp.path().join("sys");
        fs::create_dir_all(sysfs.join("power")).unwrap();
        fs::write(sysfs.join("power/state"), "freeze\n").unwrap();
        let (reason, _) = suspend_to_ram_at(
            &sysfs,
            &tmp.path().join("interrupts"),
            &tmp.path().join("flag"),
            Some(Duration::from_secs(60)),
            false,
        );
        assert_eq!(reason, WakeReason::Unsupported);
    }
}
