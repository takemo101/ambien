use std::fmt;

use gpui::{Bounds, DisplayId, Pixels, point, px, size};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSScreen, NSView, NSWindow, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, ns_string};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[derive(Clone, Copy)]
pub(crate) enum WindowRole {
    Scene,
    Settings,
}

impl WindowRole {
    fn content_size(self) -> NSSize {
        match self {
            Self::Scene => NSSize::new(960.0, 600.0),
            Self::Settings => NSSize::new(480.0, 360.0),
        }
    }

    fn style(self) -> NSWindowStyleMask {
        let common = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        match self {
            Self::Scene => common | NSWindowStyleMask::FullSizeContentView,
            Self::Settings => common,
        }
    }
}

pub(crate) struct PreparedWindow {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) display_id: DisplayId,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowPlacement {
    // Owning-screen-relative outer top-left, followed by logical content dimensions.
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) display_uuid: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowGeometryError {
    MainThreadRequired,
    DisplayUnavailable,
    InvalidGeometry,
    NativeHandleUnavailable,
}

impl fmt::Display for WindowGeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MainThreadRequired => "AppKit window geometry requires the main thread",
            Self::DisplayUnavailable => "the requested display has no available AppKit screen",
            Self::InvalidGeometry => {
                "window or screen geometry is not finite, positive, and usable"
            }
            Self::NativeHandleUnavailable => "the GPUI window has no available AppKit NSWindow",
        })
    }
}

impl std::error::Error for WindowGeometryError {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    top_left: NSPoint,
    content_size: NSSize,
}

#[derive(Clone, Copy)]
struct FrameMetrics {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

impl FrameMetrics {
    fn measured(frame: NSRect, content: NSRect) -> Self {
        Self {
            left: content.origin.x - frame.origin.x,
            bottom: content.origin.y - frame.origin.y,
            right: max_x(frame) - max_x(content),
            top: max_y(frame) - max_y(content),
        }
    }

    fn valid(self) -> bool {
        [self.left, self.bottom, self.right, self.top]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
            && (self.left + self.right).is_finite()
            && (self.bottom + self.top).is_finite()
    }

    fn frame_for_content(self, content: NSRect) -> NSRect {
        NSRect::new(
            NSPoint::new(content.origin.x - self.left, content.origin.y - self.bottom),
            NSSize::new(
                content.size.width + self.left + self.right,
                content.size.height + self.bottom + self.top,
            ),
        )
    }

    fn content_for_frame(self, frame: NSRect) -> NSRect {
        NSRect::new(
            NSPoint::new(frame.origin.x + self.left, frame.origin.y + self.bottom),
            NSSize::new(
                frame.size.width - self.left - self.right,
                frame.size.height - self.bottom - self.top,
            ),
        )
    }
}

fn max_x(rect: NSRect) -> f64 {
    rect.origin.x + rect.size.width
}

fn max_y(rect: NSRect) -> f64 {
    rect.origin.y + rect.size.height
}

fn valid_size(size: NSSize) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
}

fn valid_rect(rect: NSRect) -> bool {
    valid_size(rect.size)
        && rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && max_x(rect).is_finite()
        && max_y(rect).is_finite()
}

fn relative_top_left(screen: NSRect, outer: NSRect) -> Result<NSPoint, WindowGeometryError> {
    if !valid_rect(screen) || !valid_rect(outer) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let result = NSPoint::new(
        outer.origin.x - screen.origin.x,
        max_y(screen) - max_y(outer),
    );
    if !result.x.is_finite() || !result.y.is_finite() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(result)
}

fn outer_from_top_left(
    screen: NSRect,
    top_left: NSPoint,
    outer_size: NSSize,
) -> Result<NSRect, WindowGeometryError> {
    if !valid_rect(screen) || !top_left.x.is_finite() || !top_left.y.is_finite() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let outer = NSRect::new(
        NSPoint::new(
            screen.origin.x + top_left.x,
            max_y(screen) - top_left.y - outer_size.height,
        ),
        outer_size,
    );
    if !valid_rect(outer) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(outer)
}

fn fit_geometry(
    screen: NSRect,
    visible: Option<NSRect>,
    baseline: NSSize,
    metrics: FrameMetrics,
    saved: Option<&WindowPlacement>,
) -> Result<Placement, WindowGeometryError> {
    let visible = visible.ok_or(WindowGeometryError::DisplayUnavailable)?;
    if !valid_rect(screen) || !valid_rect(visible) || !valid_size(baseline) || !metrics.valid() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let available = metrics.content_for_frame(visible);
    if !valid_rect(available) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let content_size = NSSize::new(
        baseline.width.min(available.size.width),
        baseline.height.min(available.size.height),
    );
    let outer_size = metrics
        .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), content_size))
        .size;
    let top_left = if let Some(saved) = saved {
        if !saved.x.is_finite()
            || !saved.y.is_finite()
            || !valid_size(NSSize::new(saved.width, saved.height))
        {
            return Err(WindowGeometryError::InvalidGeometry);
        }
        let minimum = relative_top_left(screen, visible)?;
        let maximum = NSPoint::new(
            minimum.x + visible.size.width - outer_size.width,
            minimum.y + visible.size.height - outer_size.height,
        );
        if !maximum.x.is_finite()
            || !maximum.y.is_finite()
            || minimum.x > maximum.x
            || minimum.y > maximum.y
        {
            return Err(WindowGeometryError::InvalidGeometry);
        }
        NSPoint::new(
            saved.x.clamp(minimum.x, maximum.x),
            saved.y.clamp(minimum.y, maximum.y),
        )
    } else {
        relative_top_left(screen, centered_outer(visible, outer_size)?)?
    };
    // Exercise the same inverse used by the native adapter, rejecting overflow before opening.
    outer_from_top_left(screen, top_left, outer_size)?;
    Ok(Placement {
        top_left,
        content_size,
    })
}

fn centered_outer(visible: NSRect, outer_size: NSSize) -> Result<NSRect, WindowGeometryError> {
    if !valid_rect(visible)
        || !valid_size(outer_size)
        || outer_size.width > visible.size.width
        || outer_size.height > visible.size.height
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let outer = NSRect::new(
        NSPoint::new(
            visible.origin.x + (visible.size.width - outer_size.width) / 2.0,
            visible.origin.y + (visible.size.height - outer_size.height) / 2.0,
        ),
        outer_size,
    );
    if !valid_rect(outer) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(outer)
}

fn content_pixels(value: f64) -> Result<Pixels, WindowGeometryError> {
    let mut represented = value as f32;
    // GPUI stores logical dimensions as f32. Do not round fitted content up beyond the workarea.
    if f64::from(represented) > value {
        represented = represented.next_down();
    }
    if !represented.is_finite() || represented <= 0.0 {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(px(represented))
}

fn coordinate_pixels(
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<Pixels, WindowGeometryError> {
    if !value.is_finite() || !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let mut represented = value.clamp(minimum, maximum) as f32;
    if f64::from(represented) < minimum {
        represented = represented.next_up();
    }
    if f64::from(represented) > maximum {
        represented = represented.next_down();
    }
    if !represented.is_finite()
        || f64::from(represented) < minimum
        || f64::from(represented) > maximum
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(px(represented))
}

fn placement_from_geometry(
    screen: NSRect,
    outer: NSRect,
    content: NSRect,
    display_uuid: Option<String>,
) -> Result<WindowPlacement, WindowGeometryError> {
    if !valid_rect(content) || !valid_rect(outer) || !FrameMetrics::measured(outer, content).valid()
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let top_left = relative_top_left(screen, outer)?;
    Ok(WindowPlacement {
        x: top_left.x,
        y: top_left.y,
        width: content.size.width,
        height: content.size.height,
        display_uuid,
    })
}

pub(crate) fn prepare_window(
    role: WindowRole,
    display_id: DisplayId,
    saved: Option<&WindowPlacement>,
) -> Result<PreparedWindow, WindowGeometryError> {
    let mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let screens = NSScreen::screens(mtm);
    let mut geometry = None;
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let description = screen.deviceDescription();
        let Some(number) = description.objectForKey(ns_string!("NSScreenNumber")) else {
            continue;
        };
        let Some(number) = number.downcast_ref::<NSNumber>() else {
            continue;
        };
        if DisplayId::new(u64::from(number.unsignedIntValue())) == display_id {
            geometry = Some((screen.frame(), screen.visibleFrame()));
            break;
        }
    }
    let (screen, visible) = geometry.ok_or(WindowGeometryError::DisplayUnavailable)?;
    if !valid_rect(screen) || !valid_rect(visible) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let style = role.style();
    // These are AppKit's real conversions for the exact style GPUI will create.
    let available_content = NSWindow::contentRectForFrameRect_styleMask(visible, style, mtm);
    if !valid_rect(available_content) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let metrics = FrameMetrics::measured(visible, available_content);
    let placement = fit_geometry(screen, Some(visible), role.content_size(), metrics, saved)?;
    let content_size = size(
        content_pixels(placement.content_size.width)?,
        content_pixels(placement.content_size.height)?,
    );
    let native_content_size = NSSize::new(
        f64::from(content_size.width),
        f64::from(content_size.height),
    );
    let actual_frame = NSWindow::frameRectForContentRect_styleMask(
        NSRect::new(NSPoint::new(0.0, 0.0), native_content_size),
        style,
        mtm,
    );
    if !valid_rect(actual_frame) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let top_left = placement.top_left;
    let origin = point(
        coordinate_pixels(
            top_left.x,
            visible.origin.x - screen.origin.x,
            max_x(visible) - screen.origin.x - actual_frame.size.width,
        )?,
        coordinate_pixels(
            top_left.y,
            max_y(screen) - max_y(visible),
            max_y(screen) - visible.origin.y - actual_frame.size.height,
        )?,
    );
    outer_from_top_left(
        screen,
        NSPoint::new(f64::from(origin.x), f64::from(origin.y)),
        actual_frame.size,
    )?;
    Ok(PreparedWindow {
        bounds: Bounds::new(origin, content_size),
        display_id,
    })
}

pub(crate) fn capture_placement(
    window: &gpui::Window,
) -> Result<(WindowPlacement, DisplayId), WindowGeometryError> {
    let _mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| WindowGeometryError::NativeHandleUnavailable)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(WindowGeometryError::NativeHandleUnavailable);
    };
    // SAFETY: WindowHandle guarantees this NSView for the borrowed GPUI Window.
    // AppKit access is on the main thread and neither view nor getter-owned native
    // objects escape this synchronous scope. Capture performs no native mutations.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native = view
        .window()
        .ok_or(WindowGeometryError::NativeHandleUnavailable)?;
    let screen = native
        .screen()
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let description = screen.deviceDescription();
    let number = description
        .objectForKey(ns_string!("NSScreenNumber"))
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let number = number
        .downcast_ref::<NSNumber>()
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let display_id = DisplayId::new(u64::from(number.unsignedIntValue()));
    let outer = native.frame();
    let placement = placement_from_geometry(
        screen.frame(),
        outer,
        native.contentRectForFrameRect(outer),
        None,
    )?;
    Ok((placement, display_id))
}

pub(crate) fn constrain_window(
    window: &gpui::Window,
    prepared: &PreparedWindow,
) -> Result<(), WindowGeometryError> {
    let _mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let content_size = NSSize::new(
        f64::from(prepared.bounds.size.width),
        f64::from(prepared.bounds.size.height),
    );
    if !valid_size(content_size)
        || !f64::from(prepared.bounds.origin.x).is_finite()
        || !f64::from(prepared.bounds.origin.y).is_finite()
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| WindowGeometryError::NativeHandleUnavailable)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(WindowGeometryError::NativeHandleUnavailable);
    };
    // SAFETY: WindowHandle guarantees a valid NSView for the borrowed GPUI Window.
    // MainThreadMarker was checked above. This reference never escapes this synchronous
    // scope, and the raw pointer is neither retained nor released.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native = view
        .window()
        .ok_or(WindowGeometryError::NativeHandleUnavailable)?;
    let frame = native.frame();
    if !valid_rect(frame) || !valid_rect(native.contentRectForFrameRect(frame)) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    // All fallible work precedes mutations: an error leaves existing finite placement
    // and constraints untouched. NSView::window's getter-owned Retained ends here.
    let mut behavior = native.collectionBehavior();
    behavior.remove(
        NSWindowCollectionBehavior::FullScreenPrimary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::FullScreenAllowsTiling,
    );
    behavior.insert(
        NSWindowCollectionBehavior::FullScreenNone
            | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
    );
    native.setCollectionBehavior(behavior);
    if let Some(zoom) = native.standardWindowButton(NSWindowButton::ZoomButton) {
        zoom.setEnabled(false);
    }
    native.setContentMinSize(content_size);
    native.setContentMaxSize(content_size);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
    }

    fn metrics() -> FrameMetrics {
        // Measured frame/content deltas are input data, not a titlebar constant.
        FrameMetrics {
            left: 3.0,
            bottom: 2.0,
            right: 3.0,
            top: 29.0,
        }
    }

    fn saved(x: f64, y: f64, width: f64, height: f64) -> WindowPlacement {
        WindowPlacement {
            x,
            y,
            width,
            height,
            display_uuid: None,
        }
    }

    #[test]
    fn preparing_off_main_thread_fails_before_native_access() {
        let rejected = std::thread::spawn(|| {
            matches!(
                prepare_window(WindowRole::Scene, DisplayId::new(0), None),
                Err(WindowGeometryError::MainThreadRequired),
            )
        })
        .join()
        .unwrap();
        assert!(rejected);
    }

    #[test]
    fn capture_preserves_actual_offscreen_position_then_restore_uses_current_viewport() {
        let old_screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let content = rect(-2000.0, 400.0, 414.0, 259.0);
        let outer = metrics().frame_for_content(content);
        let captured = placement_from_geometry(old_screen, outer, content, None).unwrap();
        assert_eq!((captured.x, captured.y), (-83.0, -308.0));
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        let visible = rect(10.0, 20.0, 1420.0, 850.0);
        let restored = fit_geometry(
            screen,
            Some(visible),
            WindowRole::Scene.content_size(),
            metrics(),
            Some(&captured),
        )
        .unwrap();
        assert_eq!(restored.top_left, NSPoint::new(10.0, 30.0));
        assert_eq!(restored.content_size, WindowRole::Scene.content_size());
        assert_ne!(
            restored.content_size,
            NSSize::new(captured.width, captured.height),
        );
    }

    #[test]
    fn saved_outer_top_left_restores_on_a_negative_screen() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, -680.0, 1900.0, 1030.0);
        let saved = saved(220.0, 140.0, 720.0, 480.0);
        let placement = fit_geometry(
            screen,
            Some(visible),
            WindowRole::Settings.content_size(),
            metrics(),
            Some(&saved),
        )
        .unwrap();
        assert_eq!(placement.top_left, NSPoint::new(saved.x, saved.y));
        assert_eq!(placement.content_size, WindowRole::Settings.content_size());
        let frame = metrics()
            .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), placement.content_size));
        let outer = outer_from_top_left(screen, placement.top_left, frame.size).unwrap();
        assert_eq!(outer.origin, NSPoint::new(-1700.0, -151.0));
    }

    #[test]
    fn offscreen_saved_positions_clamp_to_current_workarea_edges() {
        let screen = rect(-1920.0, 200.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, 220.0, 1900.0, 1030.0);
        for (x, y, expected) in [
            (-f64::MAX, -f64::MAX, NSPoint::new(10.0, 30.0)),
            (f64::MAX, f64::MAX, NSPoint::new(1424.0, 669.0)),
        ] {
            let saved = saved(x, y, f64::MAX, f64::MAX);
            let placement = fit_geometry(
                screen,
                Some(visible),
                WindowRole::Settings.content_size(),
                metrics(),
                Some(&saved),
            )
            .unwrap();
            assert_eq!(placement.top_left, expected);
            let frame = metrics()
                .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), placement.content_size));
            let outer = outer_from_top_left(screen, placement.top_left, frame.size).unwrap();
            assert!(outer.origin.x >= visible.origin.x);
            assert!(outer.origin.y >= visible.origin.y);
            assert!(max_x(outer) <= max_x(visible));
            assert!(max_y(outer) <= max_y(visible));
        }
    }

    #[test]
    fn restored_size_uses_current_fixed_role_and_available_content() {
        let screen = rect(-1300.0, -400.0, 800.0, 600.0);
        let visible = rect(-1290.0, -380.0, 420.0, 290.0);
        for saved in [
            saved(100.0, 100.0, 1.0, 1.0),
            saved(100.0, 100.0, 8000.0, 6000.0),
        ] {
            for role in [WindowRole::Scene, WindowRole::Settings] {
                let placement = fit_geometry(
                    screen,
                    Some(visible),
                    role.content_size(),
                    metrics(),
                    Some(&saved),
                )
                .unwrap();
                assert_eq!(placement.content_size, NSSize::new(414.0, 259.0));
                assert_eq!(placement.top_left, NSPoint::new(10.0, 290.0));
            }
        }
    }

    #[test]
    fn captured_geometry_keeps_outer_coordinates_and_logical_content_size() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let content = rect(-1700.0, -300.0, 480.25, 360.5);
        let outer = metrics().frame_for_content(content);
        let uuid = Some("owning-display".to_owned());
        let placement = placement_from_geometry(screen, outer, content, uuid.clone()).unwrap();
        let expected = relative_top_left(screen, outer).unwrap();
        assert_eq!(placement.x, expected.x);
        assert_eq!(placement.y, expected.y);
        assert_eq!(placement.width, content.size.width);
        assert_eq!(placement.height, content.size.height);
        assert_ne!(placement.height, outer.size.height);
        assert_eq!(placement.display_uuid, uuid);
        assert_eq!(
            outer_from_top_left(screen, NSPoint::new(placement.x, placement.y), outer.size,)
                .unwrap(),
            outer,
        );
        assert!(
            placement_from_geometry(screen, outer, content, None)
                .unwrap()
                .display_uuid
                .is_none()
        );
    }

    #[test]
    fn nonfinite_saved_fields_and_nonpositive_dimensions_fail_explicitly() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        for saved in [
            saved(f64::NAN, 20.0, 480.0, 360.0),
            saved(20.0, f64::INFINITY, 480.0, 360.0),
            saved(20.0, 20.0, 0.0, 360.0),
            saved(20.0, 20.0, 480.0, -1.0),
            saved(20.0, 20.0, f64::INFINITY, 360.0),
            saved(20.0, 20.0, 480.0, f64::NAN),
        ] {
            assert_eq!(
                fit_geometry(
                    screen,
                    Some(screen),
                    WindowRole::Settings.content_size(),
                    metrics(),
                    Some(&saved),
                ),
                Err(WindowGeometryError::InvalidGeometry),
            );
        }
    }

    #[test]
    fn capture_rejects_invalid_native_content_and_coordinate_overflow() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        let outer = rect(20.0, 20.0, 486.0, 391.0);
        for content in [
            rect(23.0, 22.0, 480.0, 0.0),
            rect(23.0, 22.0, f64::NAN, 360.0),
            rect(23.0, 22.0, 480.0, f64::INFINITY),
            rect(19.0, 22.0, 480.0, 360.0),
        ] {
            assert_eq!(
                placement_from_geometry(screen, outer, content, None),
                Err(WindowGeometryError::InvalidGeometry),
            );
        }
        assert_eq!(
            placement_from_geometry(
                rect(-1e308, 0.0, 1e300, 900.0),
                rect(1e308, 0.0, 480.0, 360.0),
                rect(1e308, 0.0, 480.0, 360.0),
                None,
            ),
            Err(WindowGeometryError::InvalidGeometry),
        );
    }

    #[test]
    fn narrow_negative_screen_fits_the_entire_outer_frame() {
        let screen = rect(-1300.0, -400.0, 800.0, 600.0);
        let visible = rect(-1290.0, -380.0, 420.0, 290.0);
        let placement = fit_geometry(
            screen,
            Some(visible),
            NSSize::new(960.0, 600.0),
            metrics(),
            None,
        )
        .unwrap();
        assert_eq!(placement.content_size, NSSize::new(414.0, 259.0));
        let outer = metrics().frame_for_content(rect(
            0.0,
            0.0,
            placement.content_size.width,
            placement.content_size.height,
        ));
        let outer = outer_from_top_left(screen, placement.top_left, outer.size).unwrap();
        assert_eq!(outer, visible);
        assert_eq!(placement.top_left, NSPoint::new(10.0, 290.0));
    }

    #[test]
    fn roomy_workarea_keeps_baseline_content_and_centers_outer_frame() {
        let screen = rect(-1920.0, 200.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, 220.0, 1900.0, 1030.0);
        let placement = fit_geometry(
            screen,
            Some(visible),
            NSSize::new(480.0, 360.0),
            metrics(),
            None,
        )
        .unwrap();
        assert_eq!(placement.content_size, NSSize::new(480.0, 360.0));
        let frame = metrics().frame_for_content(rect(0.0, 0.0, 480.0, 360.0));
        let outer = outer_from_top_left(screen, placement.top_left, frame.size).unwrap();
        assert_eq!(outer.origin, NSPoint::new(-1203.0, 539.5));
        assert_eq!(placement.top_left, NSPoint::new(717.0, 349.5));
    }

    #[test]
    fn frame_content_and_relative_top_left_round_trip() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let content = rect(-1700.0, -300.0, 480.0, 360.0);
        let frame = metrics().frame_for_content(content);
        assert_eq!(metrics().content_for_frame(frame), content);
        let top_left = relative_top_left(screen, frame).unwrap();
        assert_eq!(
            outer_from_top_left(screen, top_left, frame.size).unwrap(),
            frame
        );
    }

    #[test]
    fn fractional_frame_content_round_trip_preserves_top_left_within_tolerance() {
        let screen = rect(-1600.3, -900.7, 1600.3, 900.7);
        let content = rect(-1400.2, -700.1, 480.2, 360.3);
        let measured = FrameMetrics {
            left: 0.3,
            bottom: 0.4,
            right: 0.7,
            top: 28.6,
        };
        let frame = measured.frame_for_content(content);
        let restored = measured.frame_for_content(measured.content_for_frame(frame));
        let expected = relative_top_left(screen, frame).unwrap();
        let actual = relative_top_left(screen, restored).unwrap();
        assert!((actual.x - expected.x).abs() < 1e-9);
        assert!((actual.y - expected.y).abs() < 1e-9);
    }

    #[test]
    fn invalid_and_unavailable_workareas_return_typed_failure() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        for visible in [
            rect(0.0, 0.0, 0.0, 875.0),
            rect(0.0, 0.0, 1440.0, -1.0),
            rect(f64::NAN, 0.0, 100.0, 100.0),
            rect(0.0, 0.0, f64::INFINITY, 100.0),
            rect(0.0, 0.0, 4.0, 20.0),
        ] {
            let update = fit_geometry(
                screen,
                Some(visible),
                NSSize::new(480.0, 360.0),
                metrics(),
                None,
            );
            assert_eq!(update, Err(WindowGeometryError::InvalidGeometry));
        }
        let unavailable = fit_geometry(screen, None, NSSize::new(480.0, 360.0), metrics(), None);
        assert_eq!(unavailable, Err(WindowGeometryError::DisplayUnavailable));
    }

    #[test]
    fn invalid_screen_content_and_metrics_are_rejected() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        assert!(
            fit_geometry(
                rect(0.0, 0.0, 0.0, 900.0),
                Some(screen),
                NSSize::new(480.0, 360.0),
                metrics(),
                None,
            )
            .is_err()
        );
        assert!(
            fit_geometry(
                rect(f64::MAX, 0.0, f64::MAX, 900.0),
                Some(screen),
                NSSize::new(480.0, 360.0),
                metrics(),
                None,
            )
            .is_err()
        );
        for size in [
            NSSize::new(0.0, 360.0),
            NSSize::new(-1.0, 360.0),
            NSSize::new(480.0, f64::NAN),
            NSSize::new(f64::INFINITY, 360.0),
        ] {
            assert!(fit_geometry(screen, Some(screen), size, metrics(), None).is_err());
        }
        let invalid = FrameMetrics {
            top: f64::NAN,
            ..metrics()
        };
        assert!(
            fit_geometry(
                screen,
                Some(screen),
                NSSize::new(480.0, 360.0),
                invalid,
                None
            )
            .is_err()
        );
        let negative = FrameMetrics {
            left: -1.0,
            ..metrics()
        };
        assert!(
            fit_geometry(
                screen,
                Some(screen),
                NSSize::new(480.0, 360.0),
                negative,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn fitted_content_rounds_down_at_the_gpui_boundary_and_rejects_underflow() {
        let lower = 480.0_f32;
        let upper = lower.next_up();
        let available = f64::from(lower) + (f64::from(upper) - f64::from(lower)) * 0.75;
        assert_eq!(content_pixels(available), Ok(px(lower)));
        assert_eq!(
            content_pixels(f64::from(f32::from_bits(1)) * 0.25),
            Err(WindowGeometryError::InvalidGeometry)
        );
    }

    #[test]
    fn coordinates_stay_inside_representable_closed_intervals() {
        let lower = 24.0_f32;
        let upper = lower.next_up();
        let left = f64::from(lower);
        let right = f64::from(upper);
        let gap = right - left;
        assert_eq!(
            coordinate_pixels(left + gap * 0.3, left + gap * 0.25, right),
            Ok(px(upper))
        );
        assert_eq!(
            coordinate_pixels(left + gap * 0.7, left, left + gap * 0.75),
            Ok(px(lower))
        );
        assert_eq!(
            coordinate_pixels(left + gap * 0.5, left + gap * 0.25, left + gap * 0.75),
            Err(WindowGeometryError::InvalidGeometry)
        );
    }
}
