//! Puts one env value on a macOS pasteboard without passing it through the
//! webview, and takes a secret back off when its time is up.

use std::hash::BuildHasher;
use std::sync::OnceLock;

use objc2_app_kit::{NSPasteboard, NSPasteboardContentsOptions, NSPasteboardTypeString};
use objc2_foundation::{NSData, NSString};

/// Marker types from the nspasteboard.org convention: clipboard managers that
/// honor it do not record a concealed item and do not keep a transient one.
const MARKER_TYPES: [&str; 2] = ["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"];

/// Seconds a concealed value stays on the clipboard before vne clears it.
pub const CLEAR_AFTER_SECONDS: u64 = 30;

/// Blocks for CLEAR_AFTER_SECONDS on CLOCK_MONOTONIC, which on macOS keeps
/// counting while the Mac sleeps and is not moved by clock changes, so
/// neither can stretch the time a secret stays on the clipboard.
pub fn wait_out_clear_delay() {
    fn now() -> std::time::Duration {
        let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: clock_gettime fills the timespec it is given.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
        std::time::Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
    }

    let deadline = now() + std::time::Duration::from_secs(CLEAR_AFTER_SECONDS);
    while let Some(left) = deadline.checked_sub(now()).filter(|left| !left.is_zero()) {
        std::thread::sleep(left.min(std::time::Duration::from_secs(1)));
    }
}

/// The concealed value vne last wrote, kept only as a write's change count
/// and a fingerprint of its text, so a delayed or exit-time clear removes
/// that value and nothing copied after it.
#[derive(Debug, Default)]
pub struct LastConcealedWrite(pub std::sync::Mutex<Option<ConcealedWrite>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcealedWrite {
    pub change_count: isize,
    pub fingerprint: u64,
}

/// A keyed hash, with keys drawn once per process, so the copied value can be
/// recognized later without being kept.
pub fn fingerprint(text: &str) -> u64 {
    static KEYS: OnceLock<std::hash::RandomState> = OnceLock::new();
    KEYS.get_or_init(std::hash::RandomState::new).hash_one(text)
}

/// Writes `text` and returns the change count that identifies this write, or
/// None when the pasteboard refused the text. A concealed write stays on this
/// Mac (no Universal Clipboard) and carries the marker types.
pub fn write(pasteboard: &NSPasteboard, text: &str, conceal: bool) -> Option<isize> {
    if conceal {
        pasteboard.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
    } else {
        pasteboard.clearContents();
    }
    // SAFETY: AppKit initializes this constant before any pasteboard exists.
    let string_type = unsafe { NSPasteboardTypeString };
    if !pasteboard.setString_forType(&NSString::from_str(text), string_type) {
        return None;
    }
    if conceal {
        let empty = NSData::new();
        for marker in MARKER_TYPES {
            pasteboard.setData_forType(Some(&empty), &NSString::from_str(marker));
        }
    }
    Some(pasteboard.changeCount())
}

/// Clears `written` while the pasteboard still holds that value, whether as
/// vne's own write or as a copy another process re-posted without the
/// markers. Anything else on the pasteboard, or a newer concealed write,
/// is left alone. Returns whether it cleared.
pub fn clear_concealed(state: &LastConcealedWrite, pasteboard: &NSPasteboard, written: ConcealedWrite) -> bool {
    let mut last = state.0.lock().unwrap();
    if *last != Some(written) {
        return false;
    }
    *last = None;
    // SAFETY: see `write`.
    let string_type = unsafe { NSPasteboardTypeString };
    let still_holds_it = pasteboard.changeCount() == written.change_count
        || pasteboard
            .stringForType(string_type)
            .is_some_and(|text| fingerprint(&text.to_string()) == written.fingerprint);
    if !still_holds_it {
        return false;
    }
    pasteboard.clearContents();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::rc::Retained;

    /// A private pasteboard, so the tests never touch the user's clipboard.
    struct Scratch(Retained<NSPasteboard>);

    impl Scratch {
        fn new() -> Self {
            Self(NSPasteboard::pasteboardWithUniqueName())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // SAFETY: releaseGlobally takes no arguments and returns nothing.
            let () = unsafe { objc2::msg_send![&*self.0, releaseGlobally] };
        }
    }

    fn types(pasteboard: &NSPasteboard) -> Vec<String> {
        pasteboard
            .types()
            .map(|types| types.iter().map(|kind| kind.to_string()).collect())
            .unwrap_or_default()
    }

    fn text(pasteboard: &NSPasteboard) -> Option<String> {
        // SAFETY: see `write`.
        let string_type = unsafe { NSPasteboardTypeString };
        pasteboard.stringForType(string_type).map(|text| text.to_string())
    }

    #[test]
    fn a_concealed_write_carries_the_markers() {
        let scratch = Scratch::new();

        write(&scratch.0, "sk_test_fake", true).unwrap();

        assert_eq!(text(&scratch.0).as_deref(), Some("sk_test_fake"));
        let types = types(&scratch.0);
        for marker in MARKER_TYPES {
            assert!(types.iter().any(|kind| kind == marker), "{marker} missing from {types:?}");
        }
    }

    #[test]
    fn a_plain_write_carries_no_markers() {
        let scratch = Scratch::new();

        write(&scratch.0, "3000", false).unwrap();

        assert_eq!(text(&scratch.0).as_deref(), Some("3000"));
        assert!(!types(&scratch.0).iter().any(|kind| kind.starts_with("org.nspasteboard.")));
    }

    fn concealed(scratch: &Scratch, state: &LastConcealedWrite, value: &str) -> ConcealedWrite {
        let change_count = write(&scratch.0, value, true).expect("a scratch pasteboard accepts text");
        let written = ConcealedWrite { change_count, fingerprint: fingerprint(value) };
        *state.0.lock().unwrap() = Some(written);
        written
    }

    fn post_plain(scratch: &Scratch, value: &str) {
        scratch.0.clearContents();
        // SAFETY: see `write`.
        scratch.0.setString_forType(&NSString::from_str(value), unsafe { NSPasteboardTypeString });
    }

    #[test]
    fn clearing_removes_the_value_vne_wrote() {
        let scratch = Scratch::new();
        let state = LastConcealedWrite::default();
        let written = concealed(&scratch, &state, "sk_test_fake");

        assert!(clear_concealed(&state, &scratch.0, written));
        assert_eq!(text(&scratch.0), None);
        assert_eq!(*state.0.lock().unwrap(), None);
    }

    #[test]
    fn clearing_removes_the_value_when_another_process_reposted_it() {
        let scratch = Scratch::new();
        let state = LastConcealedWrite::default();
        let written = concealed(&scratch, &state, "sk_test_fake");
        post_plain(&scratch, "sk_test_fake");

        assert!(clear_concealed(&state, &scratch.0, written));
        assert_eq!(text(&scratch.0), None);
    }

    #[test]
    fn clearing_leaves_what_was_copied_afterwards() {
        let scratch = Scratch::new();
        let state = LastConcealedWrite::default();
        let written = concealed(&scratch, &state, "sk_test_fake");
        post_plain(&scratch, "copied later");

        assert!(!clear_concealed(&state, &scratch.0, written));
        assert_eq!(text(&scratch.0).as_deref(), Some("copied later"));
    }

    #[test]
    fn an_older_timer_leaves_a_newer_concealed_value() {
        let scratch = Scratch::new();
        let state = LastConcealedWrite::default();
        let first = concealed(&scratch, &state, "first");
        let second = concealed(&scratch, &state, "second");

        assert!(!clear_concealed(&state, &scratch.0, first));
        assert_eq!(text(&scratch.0).as_deref(), Some("second"));
        assert!(clear_concealed(&state, &scratch.0, second));
    }
}
