//! WebKitGTK 2.54 stopped compositing transparent windows on some GPU stacks
//! (seen on NVIDIA + KWin), leaving the whole window blank. Running the window
//! opaque there paints it; the only cost is the rounded corners are filled in.

#[cfg(target_os = "linux")]
extern "C" {
    fn webkit_get_major_version() -> u32;
    fn webkit_get_minor_version() -> u32;
}

/// The version that regressed transparent surfaces; earlier ones stay
/// transparent so their rounded corners keep showing the desktop.
fn opaque_from_version(major: u32, minor: u32) -> bool {
    (major, minor) >= (2, 54)
}

/// Whether the linked WebKitGTK needs an opaque window. Memoized: the library
/// version cannot change under a running process.
pub fn needs_opaque_window() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::sync::OnceLock;
        static NEEDS: OnceLock<bool> = OnceLock::new();
        *NEEDS.get_or_init(|| {
            let (major, minor) =
                unsafe { (webkit_get_major_version(), webkit_get_minor_version()) };
            opaque_from_version(major, minor)
        })
    }
    #[cfg(not(target_os = "linux"))]
    false
}

#[cfg(test)]
mod tests {
    use super::opaque_from_version;

    #[test]
    fn transparent_stays_on_pre_2_54() {
        assert!(!opaque_from_version(2, 52));
        assert!(!opaque_from_version(2, 48));
        assert!(!opaque_from_version(1, 99));
    }

    #[test]
    fn opaque_from_2_54_onward() {
        assert!(opaque_from_version(2, 54));
        assert!(opaque_from_version(2, 55));
        assert!(opaque_from_version(3, 0));
    }
}
