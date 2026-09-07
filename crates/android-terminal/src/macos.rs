//! macOS window chrome: native traffic-light layout.

use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameVibrantDark, NSView, NSWindow,
    NSWindowButton, NSWindowStyleMask,
};
use objc2_foundation::NSPoint;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Vertically centers the close/miniaturize/zoom buttons in the title bar.
///
/// AppKit relayouts these buttons after resize; this writes origin only when it has drifted.
/// Disabled buttons use the system grey (inactive) look; hover enables the usual colors.
pub fn sync_traffic_lights(frame: &eframe::Frame, title_bar_height: f32, colored: bool) {
    let Some(window) = ns_window(frame) else {
        return;
    };
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return;
    }

    let Some(close) = window.standardWindowButton(NSWindowButton::NSWindowCloseButton) else {
        return;
    };
    let Some(miniaturize) = window.standardWindowButton(NSWindowButton::NSWindowMiniaturizeButton)
    else {
        return;
    };
    let Some(zoom) = window.standardWindowButton(NSWindowButton::NSWindowZoomButton) else {
        return;
    };

    // SAFETY: AppKit exports this appearance name as an immutable string constant.
    let Some(appearance) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameVibrantDark })
    else {
        return;
    };

    // SAFETY: `standardWindowButton` views live in the window's titlebar hierarchy.
    unsafe {
        let Some(button_container) = close.superview() else {
            return;
        };
        let Some(titlebar_container) = button_container.superview() else {
            return;
        };

        let height = f64::from(title_bar_height);
        let window_height = window.frame().size.height;
        let mut container_frame = titlebar_container.frame();
        if (container_frame.size.height - height).abs() > 0.5 {
            container_frame.size.height = height;
            container_frame.origin.y = window_height - height;
            titlebar_container.setFrame(container_frame);
        }

        let button_height = close.frame().size.height;
        let origin_y = (height - button_height) / 2.0;
        for button in [&close, &miniaturize, &zoom] {
            button.setAppearance(Some(&appearance));
            if button.isEnabled() != colored {
                button.setEnabled(colored);
            }
            let origin = button.frame().origin;
            if (origin.y - origin_y).abs() > 0.5 {
                button.setFrameOrigin(NSPoint::new(origin.x, origin_y));
            }
        }
    }
}

fn ns_window(frame: &eframe::Frame) -> Option<Retained<NSWindow>> {
    let handle = frame.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: eframe's AppKit handle is the live content NSView for this frame.
    let ns_view = unsafe { Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }?;
    ns_view.window()
}
