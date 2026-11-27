#[must_use]
pub fn circular_reveal_max_radius(width: f64, height: f64, cx: f64, cy: f64) -> f64 {
    let w = width.max(0.0);
    let h = height.max(0.0);
    let clamped_x = cx.clamp(0.0, w);
    let clamped_y = cy.clamp(0.0, h);
    let max_dx = clamped_x.max(w - clamped_x);
    let max_dy = clamped_y.max(h - clamped_y);
    max_dx.hypot(max_dy)
}

#[must_use]
pub fn map_reveal_origin_y(origin_y: f64, height: f64, geometry_flipped: bool) -> f64 {
    let h = height.max(0.0);
    let y = origin_y.clamp(0.0, h);
    if geometry_flipped { y } else { h - y }
}

#[must_use]
pub fn system_prefers_dark() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::system_prefers_dark()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn capture_theme_transition_snapshot() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::capture()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn reveal_theme_transition_snapshot(origin: [f32; 2], duration_ms: u64) {
    #[cfg(target_os = "macos")]
    {
        macos::reveal(origin, duration_ms);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (origin, duration_ms);
    }
}

pub fn cancel_theme_transition_snapshot() {
    #[cfg(target_os = "macos")]
    {
        macos::cancel();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::{Cell, RefCell};
    use std::ptr;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSApplication, NSWindow};
    use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
        CGImageAlphaInfo, CGImageByteOrderInfo, CGMutablePath, CGPath, kCGColorSpaceSRGB,
    };
    use objc2_foundation::{NSUserDefaults, ns_string};
    use objc2_quartz_core::{
        CAAnimation, CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer,
        CATransaction, kCAFillRuleEvenOdd, kCAGravityResize,
    };

    use super::{circular_reveal_max_radius, map_reveal_origin_y};

    const OVERLAY_Z_POSITION: CGFloat = 1_000_000.0;
    const INITIAL_HOLE_RADIUS: CGFloat = 0.5;
    const FINAL_RADIUS_PAD: CGFloat = 2.0;

    struct ActiveReveal {
        generation: u64,
        overlay: Retained<CALayer>,
    }

    thread_local! {
        static NEXT_GENERATION: Cell<u64> = const { Cell::new(1) };
        static ACTIVE_REVEAL: RefCell<Option<ActiveReveal>> = const { RefCell::new(None) };
    }

    pub(super) fn system_prefers_dark() -> Option<bool> {
        autoreleasepool(|_| {
            let defaults = NSUserDefaults::standardUserDefaults();
            let style = defaults.stringForKey(ns_string!("AppleInterfaceStyle"));
            Some(style.is_some_and(|val| val.isEqualToString(ns_string!("Dark"))))
        })
    }

    pub(super) fn capture() -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        autoreleasepool(|_| capture_on_main(mtm))
    }

    pub(super) fn reveal(origin: [f32; 2], duration_ms: u64) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        autoreleasepool(|_| reveal_on_main(origin, duration_ms));
    }

    pub(super) fn cancel() {
        if MainThreadMarker::new().is_none() {
            return;
        }
        autoreleasepool(|_| {
            let previous = ACTIVE_REVEAL.with(|slot| slot.borrow_mut().take());
            if let Some(active) = previous {
                quiet(|| active.overlay.removeFromSuperlayer());
            }
        });
    }

    fn visible_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        let app = NSApplication::sharedApplication(mtm);
        if let Some(window) = app.mainWindow()
            && window.isVisible()
        {
            return Some(window);
        }
        if let Some(window) = app.keyWindow()
            && window.isVisible()
        {
            return Some(window);
        }
        app.windows().into_iter().find(|window| window.isVisible())
    }

    fn capture_on_main(mtm: MainThreadMarker) -> bool {
        let Some(window) = visible_window(mtm) else {
            return false;
        };
        let Some(content_view) = window.contentView() else {
            return false;
        };
        let Some(container_layer) = content_view.layer() else {
            return false;
        };

        let bounds = container_layer.bounds();
        let width = bounds.size.width;
        let height = bounds.size.height;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return false;
        }
        let scale = window.backingScaleFactor().max(1.0);

        // Detach any in-flight overlay before rendering so `renderInContext:` captures the live
        // view tree instead of an animated `CAShapeLayer` presentation mask.
        let previous = ACTIVE_REVEAL.with(|slot| slot.borrow_mut().take());
        if let Some(prev) = previous {
            quiet(|| prev.overlay.removeFromSuperlayer());
        }

        let source_layer = if let Some(root) = content_view.subviews().firstObject()
            && let Some(layer) = root.layer()
        {
            layer
        } else {
            container_layer.clone()
        };

        display_tree(&source_layer);
        let Some(image) = render_layer_image(&source_layer, width, height, scale) else {
            return false;
        };

        let overlay = CALayer::new();
        let generation = NEXT_GENERATION.with(|next| {
            let id = next.get();
            next.set(id.wrapping_add(1).max(1));
            id
        });
        ACTIVE_REVEAL.with(|slot| {
            *slot.borrow_mut() = Some(ActiveReveal {
                generation,
                overlay: overlay.clone(),
            });
        });

        // `nsh-platform-macos` hides root children on `ScreenTransition` before the GPU returns
        // its first frame, producing a blank flash. Pinning a snapshot CALayer above the live view
        // tree keeps the previous theme visible without hiding children.
        quiet(|| {
            overlay.setFrame(bounds);
            overlay.setZPosition(OVERLAY_Z_POSITION);
            #[expect(
                unsafe_code,
                reason = "Core Animation immutable gravity constant and CGImage contents"
            )]
            unsafe {
                overlay.setContentsGravity(kCAGravityResize);
                overlay.setContents(Some(cgimage_object(&image)));
            }
            container_layer.addSublayer(&overlay);
        });
        CATransaction::flush();
        true
    }

    fn reveal_on_main(origin: [f32; 2], duration_ms: u64) {
        let active = ACTIVE_REVEAL.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|entry| (entry.generation, entry.overlay.clone()))
        });
        let Some((generation, overlay)) = active else {
            return;
        };

        let bounds = overlay.bounds();
        let width = bounds.size.width;
        let height = bounds.size.height;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            finish_reveal(generation);
            return;
        }

        let cx = f64::from(origin[0]).clamp(0.0, width);
        let cy = map_reveal_origin_y(f64::from(origin[1]), height, overlay.isGeometryFlipped());
        let max_radius = circular_reveal_max_radius(width, height, cx, cy) + FINAL_RADIUS_PAD;

        let from_path = even_odd_hole_path(bounds, cx, cy, INITIAL_HOLE_RADIUS);
        let to_path = even_odd_hole_path(bounds, cx, cy, max_radius);
        let from_cgpath: &CGPath = &from_path;
        let to_cgpath: &CGPath = &to_path;

        let mask = CAShapeLayer::new();
        let mask_layer: &CALayer = &mask;
        quiet(|| {
            mask.setFrame(bounds);
            #[expect(
                unsafe_code,
                reason = "Core Animation immutable fill rule constant and layer mask"
            )]
            unsafe {
                mask.setFillRule(kCAFillRuleEvenOdd);
                overlay.setMask(Some(mask_layer));
            }
            mask.setPath(Some(to_cgpath));
        });

        let animation = CABasicAnimation::animationWithKeyPath(Some(ns_string!("path")));
        animation.setDuration((duration_ms as f64) / 1000.0);
        let timing = CAMediaTimingFunction::functionWithControlPoints(0.4, 0.0, 0.2, 1.0);
        animation.setTimingFunction(Some(&timing));
        #[expect(
            unsafe_code,
            reason = "Toll-free bridged CGPath values for CABasicAnimation"
        )]
        unsafe {
            animation.setFromValue(Some(cgpath_object(from_cgpath)));
            animation.setToValue(Some(cgpath_object(to_cgpath)));
        }
        let ca_animation: &CAAnimation = &animation;

        let completion = RcBlock::new(move || {
            finish_reveal(generation);
        });
        CATransaction::begin();
        #[expect(unsafe_code, reason = "CATransaction completion block FFI")]
        unsafe {
            CATransaction::setCompletionBlock(Some(&completion));
        }
        mask.addAnimation_forKey(ca_animation, Some(ns_string!("cozyThemeReveal")));
        CATransaction::commit();
    }

    fn finish_reveal(generation: u64) {
        autoreleasepool(|_| {
            let removed = ACTIVE_REVEAL.with(|slot| {
                let mut guard = slot.borrow_mut();
                if guard
                    .as_ref()
                    .is_some_and(|entry| entry.generation == generation)
                {
                    guard.take()
                } else {
                    None
                }
            });
            if let Some(active) = removed {
                quiet(|| active.overlay.removeFromSuperlayer());
            }
        });
    }

    fn quiet(f: impl FnOnce()) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        f();
        CATransaction::commit();
    }

    fn display_tree(layer: &CALayer) {
        layer.displayIfNeeded();
        #[expect(unsafe_code, reason = "CALayer sublayers getter on main thread")]
        let sublayers = unsafe { layer.sublayers() };
        if let Some(sublayers) = sublayers {
            for sublayer in sublayers {
                display_tree(&sublayer);
            }
        }
    }

    fn render_layer_image(
        layer: &CALayer,
        width: CGFloat,
        height: CGFloat,
        scale: CGFloat,
    ) -> Option<CFRetained<CGImage>> {
        let pixel_width = (width * scale).round().max(1.0) as usize;
        let pixel_height = (height * scale).round().max(1.0) as usize;

        #[expect(
            unsafe_code,
            reason = "CoreGraphics immutable sRGB color space constant"
        )]
        let color_space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
        let bitmap_info =
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        #[expect(unsafe_code, reason = "CoreGraphics owned bitmap context allocation")]
        let context = unsafe {
            CGBitmapContextCreate(
                ptr::null_mut(),
                pixel_width,
                pixel_height,
                8,
                pixel_width.checked_mul(4)?,
                Some(&color_space),
                bitmap_info,
            )
        }?;

        CGContext::clear_rect(
            Some(&context),
            CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(pixel_width as CGFloat, pixel_height as CGFloat),
            ),
        );
        CGContext::save_g_state(Some(&context));
        if layer.isGeometryFlipped() {
            CGContext::translate_ctm(Some(&context), 0.0, pixel_height as CGFloat);
            CGContext::scale_ctm(Some(&context), scale, -scale);
        } else {
            CGContext::scale_ctm(Some(&context), scale, scale);
        }
        quiet(|| layer.renderInContext(&context));
        CGContext::restore_g_state(Some(&context));

        CGBitmapContextCreateImage(Some(&context))
    }

    fn even_odd_hole_path(
        bounds: CGRect,
        cx: CGFloat,
        cy: CGFloat,
        radius: CGFloat,
    ) -> CFRetained<CGMutablePath> {
        let path = CGMutablePath::new();
        let circle = CGRect::new(
            CGPoint::new(cx - radius, cy - radius),
            CGSize::new(radius * 2.0, radius * 2.0),
        );
        #[expect(
            unsafe_code,
            reason = "CoreGraphics path construction with null transform"
        )]
        unsafe {
            CGMutablePath::add_rect(Some(&path), ptr::null(), bounds);
            CGMutablePath::add_ellipse_in_rect(Some(&path), ptr::null(), circle);
        }
        path
    }

    fn cgimage_object(image: &CGImage) -> &AnyObject {
        // SAFETY: `CGImage` is a toll-free bridged CoreFoundation object kept alive by the caller.
        #[expect(unsafe_code, reason = "Toll-free bridge CGImage to CALayer contents")]
        unsafe {
            &*ptr::from_ref(image).cast::<AnyObject>()
        }
    }

    fn cgpath_object(path: &CGPath) -> &AnyObject {
        // SAFETY: `CGPath` is a toll-free bridged CoreFoundation object kept alive by the caller.
        #[expect(
            unsafe_code,
            reason = "Toll-free bridge CGPath to CABasicAnimation value"
        )]
        unsafe {
            &*ptr::from_ref(path).cast::<AnyObject>()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circular_reveal_max_radius_reaches_farthest_corner() {
        let r = circular_reveal_max_radius(920.0, 680.0, 840.0, 44.0);
        let expected = 840.0_f64.hypot(636.0);
        assert!((r - expected).abs() < 1e-6);

        let center = circular_reveal_max_radius(100.0, 100.0, 50.0, 50.0);
        assert!((center - 50.0_f64.hypot(50.0)).abs() < 1e-6);
    }

    #[test]
    fn map_reveal_origin_y_converts_between_top_left_and_bottom_left() {
        assert!((map_reveal_origin_y(44.0, 680.0, true) - 44.0).abs() < 1e-6);
        assert!((map_reveal_origin_y(44.0, 680.0, false) - 636.0).abs() < 1e-6);
        assert!((map_reveal_origin_y(-10.0, 680.0, false) - 680.0).abs() < 1e-6);
        assert!((map_reveal_origin_y(900.0, 680.0, false) - 0.0).abs() < 1e-6);
    }
}
