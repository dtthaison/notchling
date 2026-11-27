use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSBezierPath, NSColor, NSCompositingOperation, NSGraphicsContext, NSImage};
use objc2_foundation::{NSAffineTransform, NSPoint, NSRect, NSSize};

use crate::tray_glyph::{Frame, Mark, Tint, HEIGHT, PILLS, STROKE, TURN_CENTER, WIDTH};

const HOLE_GAP: f64 = 0.45;

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn tint_color(tint: Tint, alpha: f64) -> Retained<NSColor> {
    let base = match tint {
        Tint::Green => NSColor::systemGreenColor(),
        Tint::Orange => NSColor::systemOrangeColor(),
        Tint::Red => NSColor::systemRedColor(),
    };
    base.colorWithAlphaComponent(alpha)
}

fn pill_path(index: usize) -> Retained<NSBezierPath> {
    let pill = PILLS[index];
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
        rect(pill.x, pill.y, pill.width, pill.height),
        pill.radius,
        pill.radius,
    );
    if pill.turn_degrees != 0.0 {
        let turn = NSAffineTransform::transform();
        turn.translateXBy_yBy(TURN_CENTER.0, TURN_CENTER.1);
        turn.rotateByDegrees(pill.turn_degrees);
        turn.translateXBy_yBy(-TURN_CENTER.0, -TURN_CENTER.1);
        path.transformUsingAffineTransform(&turn);
    }
    path
}

fn oval(x: f64, y: f64, radius: f64) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithOvalInRect(rect(x - radius, y - radius, radius * 2.0, radius * 2.0))
}

fn draw_frame(frame: &Frame) {
    let label = NSColor::labelColor();
    label.set();
    for index in 0..PILLS.len() {
        let path = pill_path(index);
        path.setLineWidth(STROKE);
        if frame.filled {
            path.fill();
        }
        path.stroke();
    }
    if frame.filled {
        if let Some(context) = NSGraphicsContext::currentContext() {
            context.setCompositingOperation(NSCompositingOperation::Clear);
            for mark in &frame.marks {
                let (x, y) = mark.center();
                let radius = mark.radius() + HOLE_GAP;
                oval(x, y, radius).fill();
            }
            context.setCompositingOperation(NSCompositingOperation::SourceOver);
        }
    }
    let color = tint_color(frame.tint, frame.alpha);
    color.set();
    for mark in &frame.marks {
        match *mark {
            Mark::Bead { x, y, radius } => oval(x, y, radius).fill(),
            Mark::Ring { x, y, radius, line } => {
                let ring = oval(x, y, radius);
                ring.setLineWidth(line);
                ring.stroke();
            }
        }
    }
}

pub fn drawn_image(frame: Frame, description: &str) -> Retained<NSImage> {
    let handler = RcBlock::new(move |_: NSRect| -> Bool {
        draw_frame(&frame);
        Bool::YES
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(WIDTH, HEIGHT), true, &handler);
    image.setTemplate(false);
    image.setAccessibilityDescription(Some(&objc2_foundation::NSString::from_str(description)));
    image
}
