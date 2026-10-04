//! Puts one env value on a macOS pasteboard without passing it through the
//! webview, and takes a secret back off when its time is up.

use objc2_app_kit::{NSPasteboard, NSPasteboardContentsOptions, NSPasteboardTypeString};
use objc2_foundation::{NSData, NSString};

/// Marker types from the nspasteboard.org convention: clipboard managers that
/// honor it do not record a concealed item and do not keep a transient one.
const MARKER_TYPES: [&str; 2] = ["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"];

/// Seconds a concealed value stays on the clipboard before vne clears it.
pub const CONCEALED_SECONDS: u64 = 30;

/// The change count of the concealed value vne last wrote, so a delayed or
/// exit-time clear never removes something copied after it.
#[derive(Debug, Default)]
pub struct ConcealedClipboard(pub std::sync::Mutex<Option<isize>>);

/// Writes `text` and returns the change count that identifies this write.
/// A concealed write stays on this Mac (no Universal Clipboard) and carries
/// the marker types.
pub fn write(pasteboard: &NSPasteboard, text: &str, conceal: bool) -> isize {
    if conceal {
        pasteboard.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
    } else {
        pasteboard.clearContents();
    }
    // SAFETY: AppKit initializes this constant before any pasteboard exists.
    let string_type = unsafe { NSPasteboardTypeString };
    pasteboard.setString_forType(&NSString::from_str(text), string_type);
    if conceal {
        let empty = NSData::new();
        for marker in MARKER_TYPES {
            pasteboard.setData_forType(Some(&empty), &NSString::from_str(marker));
        }
    }
    pasteboard.changeCount()
}

/// Clears the concealed value written at `change_count`, unless anything was
/// written to the pasteboard since or a newer concealed write replaced it.
/// Returns whether it cleared.
pub fn clear_concealed(state: &ConcealedClipboard, pasteboard: &NSPasteboard, change_count: isize) -> bool {
    let mut last = state.0.lock().unwrap();
    if *last != Some(change_count) {
        return false;
    }
    *last = None;
    if pasteboard.changeCount() != change_count {
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

        write(&scratch.0, "sk_test_fake", true);

        assert_eq!(text(&scratch.0).as_deref(), Some("sk_test_fake"));
        let types = types(&scratch.0);
        for marker in MARKER_TYPES {
            assert!(types.iter().any(|kind| kind == marker), "{marker} missing from {types:?}");
        }
    }

    #[test]
    fn a_plain_write_carries_no_markers() {
        let scratch = Scratch::new();

        write(&scratch.0, "3000", false);

        assert_eq!(text(&scratch.0).as_deref(), Some("3000"));
        assert!(!types(&scratch.0).iter().any(|kind| kind.starts_with("org.nspasteboard.")));
    }

    #[test]
    fn clearing_removes_the_value_vne_wrote() {
        let scratch = Scratch::new();
        let state = ConcealedClipboard::default();
        let change = write(&scratch.0, "sk_test_fake", true);
        *state.0.lock().unwrap() = Some(change);

        assert!(clear_concealed(&state, &scratch.0, change));
        assert_eq!(text(&scratch.0), None);
        assert_eq!(*state.0.lock().unwrap(), None);
    }

    #[test]
    fn clearing_leaves_what_was_copied_afterwards() {
        let scratch = Scratch::new();
        let state = ConcealedClipboard::default();
        let change = write(&scratch.0, "sk_test_fake", true);
        *state.0.lock().unwrap() = Some(change);
        scratch.0.clearContents();
        // SAFETY: see `write`.
        scratch.0.setString_forType(&NSString::from_str("copied later"), unsafe { NSPasteboardTypeString });

        assert!(!clear_concealed(&state, &scratch.0, change));
        assert_eq!(text(&scratch.0).as_deref(), Some("copied later"));
    }

    #[test]
    fn an_older_timer_leaves_a_newer_concealed_value() {
        let scratch = Scratch::new();
        let state = ConcealedClipboard::default();
        let first = write(&scratch.0, "first", true);
        let second = write(&scratch.0, "second", true);
        *state.0.lock().unwrap() = Some(second);

        assert!(!clear_concealed(&state, &scratch.0, first));
        assert_eq!(text(&scratch.0).as_deref(), Some("second"));
        assert!(clear_concealed(&state, &scratch.0, second));
    }
}
