//! Permission to use AiM USB devices (a MyChron 6 by USB, the MyChron 4
//! data key) on Linux. Linux lets an app talk to a USB device only when a
//! rule allows it. The .deb and .rpm install AiM's rule; with the AppImage or
//! the tarball, the Device page offers to install it (the computer asks for
//! an administrator's password once). Macs need nothing.

/// AiM's rule, as the packages install it.
pub const RULE: &str = include_str!("../../../packaging/linux/70-mychronix-aim.rules");

/// True when an AiM device is plugged in that Mychronix isn't allowed to open.
#[cfg(target_os = "linux")]
pub fn blocked() -> bool {
    let Ok(devices) = std::fs::read_dir("/sys/bus/usb/devices") else { return false };
    devices.flatten().any(|d| {
        let read = |f: &str| std::fs::read_to_string(d.path().join(f)).map(|s| s.trim().to_string()).ok();
        if read("idVendor").as_deref() != Some("11cc") {
            return false;
        }
        let (Some(bus), Some(dev)) = (read("busnum").and_then(|b| b.parse::<u32>().ok()), read("devnum").and_then(|d| d.parse::<u32>().ok())) else {
            return false;
        };
        // Opening the device file is how any USB app starts; it changes nothing on the device.
        let node = format!("/dev/bus/usb/{bus:03}/{dev:03}");
        std::fs::OpenOptions::new().read(true).write(true).open(&node).is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

#[cfg(not(target_os = "linux"))]
pub fn blocked() -> bool {
    false
}

/// Installs the rule and applies it to devices already plugged in, asking
/// for an administrator's password through the desktop (pkexec). Blocks
/// until done; run it in the background.
#[cfg(target_os = "linux")]
pub fn allow() -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("mychronix-aim-{}.rules", std::process::id()));
    std::fs::write(&tmp, RULE).map_err(|e| format!("Couldn't prepare the rule: {e}"))?;
    let script = "install -m 644 \"$1\" /etc/udev/rules.d/70-mychronix-aim.rules \
                  && udevadm control --reload-rules \
                  && udevadm trigger --action=change --subsystem-match=usb --attr-match=idVendor=11cc";
    let status = std::process::Command::new("pkexec").args(["/bin/sh", "-c", script, "sh"]).arg(&tmp).status();
    let _ = std::fs::remove_file(&tmp);
    match status {
        Ok(s) if s.success() => {
            log::info!("installed AiM's USB rule in /etc/udev/rules.d");
            Ok(())
        }
        // pkexec: 126 = the password prompt was dismissed, 127 = not authorized.
        Ok(s) if matches!(s.code(), Some(126) | Some(127)) => Err("Not allowed: the password prompt was closed or refused.".into()),
        Ok(s) => Err(format!("Installing the rule didn't work (exit {}).", s.code().unwrap_or(-1))),
        Err(e) => Err(format!("Couldn't ask for permission ({e}). {}", manual())),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn allow() -> Result<(), String> {
    Ok(())
}

/// The same, by hand, for computers without a password prompt (pkexec).
pub fn manual() -> &'static str {
    "In a terminal: sudo cp 70-mychronix-aim.rules /etc/udev/rules.d/ (from the Mychronix download), then unplug the device and plug it back in."
}
