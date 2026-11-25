//! The G502 drawing used by the button map: outline shapes, where each
//! button's marker sits, and where its label card goes. Free of GPUI.
//!
//! Coordinates are in a fixed view of [`VIEW_W`] by [`VIEW_H`] units; the
//! painter scales them to the element's bounds. The drawing is a stylised
//! three-quarter view shared by the Proteus Spectrum and the LIGHTSPEED, whose
//! button layouts match. Positions were placed by eye from photos.

/// Width of the drawing's coordinate space.
pub const VIEW_W: f32 = 820.0;
/// Height of the drawing's coordinate space.
pub const VIEW_H: f32 = 320.0;
/// Width of a label card, in view units.
pub const CARD_W: f32 = 150.0;
/// Height of a label card, in view units.
pub const CARD_H: f32 = 40.0;
/// Diameter of a button marker, in view units.
pub const MARKER: f32 = 24.0;

/// What a shape is filled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    None,
    Body,
    Shell,
    Recess,
    Key,
}

/// What a shape is outlined with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Outline,
    Seam,
    Accent,
}

/// One outline of the drawing, back to front.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// Path data using absolute `M`, `L`, `C` and `Z` only.
    pub d: &'static str,
    pub fill: Fill,
    pub stroke: Stroke,
    pub width: f32,
    /// The onboard-profile button index this shape is the key of.
    pub button: Option<usize>,
}

/// The drawing, in paint order.
pub const SHAPES: &[Shape] = &[
    Shape { d: "M 175 217.5 C 180 185 220 145 265 115 C 310 80 375 45 460 37.5 C 540 32.5 610 65 640 120 C 647.5 137.5 652.5 152.5 647.5 157.5 L 625 170 C 617.5 200 610 225 602.5 235 C 560 252.5 510 257.5 470 255 L 460 265 L 400 260 L 380 247.5 L 230 225 L 190 226 Z", fill: Fill::Body, stroke: Stroke::Outline, width: 1.5, button: None },
    Shape { d: "M 410 65 C 460 47.5 550 50 600 95 C 620 115 635 140 625 170 L 575 145 L 510 151 L 505 151 L 460 127.5 L 492.5 82.5 Z", fill: Fill::Shell, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 460 190 L 575 145 L 625 170 C 617.5 200 610 225 602.5 235 C 560 252.5 510 257.5 480 252.5 L 477.5 230 Z", fill: Fill::Recess, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 490 200 L 498 230", fill: Fill::None, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 509 193.5 L 517 225", fill: Fill::None, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 528 187 L 536 220", fill: Fill::None, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 547 180.5 L 555 215", fill: Fill::None, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 566 174 L 574 210", fill: Fill::None, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 585 167.5 L 593 205", fill: Fill::None, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 225 225 L 285 211 L 395 212.5 L 410 235 L 400 260 L 380 247.5 Z", fill: Fill::Recess, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 180 214 C 190 180 225 145 267.5 117.5 L 280 129 C 245 157.5 217.5 187.5 210 219 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(1) },
    Shape { d: "M 215 219 C 245 180 285 151 325 132.5 L 332.5 100 L 410 65 L 492.5 82.5 L 445 112.5 L 372.5 149 L 285 210 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(0) },
    Shape { d: "M 262.5 120 C 280 100 310 85 332.5 100 L 325 132.5 C 305 140 285 155 272.5 165 Z", fill: Fill::Recess, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 312.6 135.7 L 308.4 141.4 L 303.4 146.3 L 298 150.1 L 292.4 152.5 L 286.8 153.5 L 281.6 153.1 L 277.1 151.2 L 273.3 147.8 L 270.6 143.4 L 269.1 137.9 L 268.9 131.8 L 269.9 125.2 L 272.1 118.7 L 275.4 112.3 L 279.6 106.6 L 284.6 101.7 L 290 98 L 295.6 95.5 L 301.2 94.5 L 306.4 95 L 310.9 96.8 L 314.7 100.2 L 317.4 104.6 L 318.9 110 L 319.1 116.2 L 318.1 122.8 L 315.9 129.3 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.2, button: Some(2) },
    Shape { d: "M 280 140 L 293 148", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 283.5 133.5 L 296.5 141.5", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 287 127 L 300 135", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 290.5 120.5 L 303.5 128.5", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 294 114 L 307 122", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 297.5 107.5 L 310.5 115.5", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 301 101 L 314 109", fill: Fill::None, stroke: Stroke::Outline, width: 1.0, button: None },
    Shape { d: "M 347.5 84 L 375 71 L 387.5 79 L 360 93 Z", fill: Fill::Recess, stroke: Stroke::Seam, width: 1.0, button: None },
    Shape { d: "M 387.5 66 L 415 54 L 430 61 L 402.5 74 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(8) },
    Shape { d: "M 289 213 L 370 154 L 395 165 L 380 186 L 310 217 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(7) },
    Shape { d: "M 374 151 L 442.5 115 L 461 128 L 441 153 L 398 164 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(6) },
    Shape { d: "M 474 138 L 491 132", fill: Fill::None, stroke: Stroke::Accent, width: 2.5, button: None },
    Shape { d: "M 481 130 L 498 124", fill: Fill::None, stroke: Stroke::Accent, width: 2.5, button: None },
    Shape { d: "M 488 122 L 505 116", fill: Fill::None, stroke: Stroke::Accent, width: 2.5, button: None },
    Shape { d: "M 438 171 L 505 152 L 511 164 L 451 186 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(4) },
    Shape { d: "M 514 143 L 561 123 L 569 132 L 524 155 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(3) },
    Shape { d: "M 396 213 L 460 198 L 478 230 L 455 248 L 410 235 Z", fill: Fill::Key, stroke: Stroke::Outline, width: 1.0, button: Some(5) },
    Shape { d: "M 540 55 C 520 51 505 60 507.5 71 C 510 81 530 83 541 76 L 541 68 L 527.5 68", fill: Fill::None, stroke: Stroke::Accent, width: 3.0, button: None },
];

/// Which side of the drawing a button's label card sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Where one button is marked and labelled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Centre of the marker on the drawing.
    pub marker: (f32, f32),
    pub side: Side,
    /// Vertical centre of the label card.
    pub card_y: f32,
}

/// Anchors in onboard-profile button order: G1 to G9, wheel tilt right,
/// wheel tilt left.
pub const ANCHORS: [Anchor; 11] = [
    Anchor { marker: (375.0, 105.0), side: Side::Right, card_y: 98.0 },
    Anchor { marker: (208.0, 188.0), side: Side::Left, card_y: 228.0 },
    Anchor { marker: (294.0, 122.0), side: Side::Left, card_y: 124.0 },
    Anchor { marker: (540.0, 138.0), side: Side::Right, card_y: 150.0 },
    Anchor { marker: (472.0, 168.0), side: Side::Right, card_y: 202.0 },
    Anchor { marker: (435.0, 222.0), side: Side::Right, card_y: 254.0 },
    Anchor { marker: (415.0, 142.0), side: Side::Left, card_y: 72.0 },
    Anchor { marker: (342.0, 188.0), side: Side::Left, card_y: 280.0 },
    Anchor { marker: (408.0, 64.0), side: Side::Right, card_y: 46.0 },
    Anchor { marker: (320.0, 95.0), side: Side::Left, card_y: 20.0 },
    Anchor { marker: (268.0, 148.0), side: Side::Left, card_y: 176.0 },
];

impl Anchor {
    /// Left edge of the label card.
    pub fn card_x(&self) -> f32 {
        match self.side {
            Side::Left => 16.0,
            Side::Right => VIEW_W - 16.0 - CARD_W,
        }
    }

    /// Where the leader line leaves the card.
    pub fn line_start(&self) -> (f32, f32) {
        match self.side {
            Side::Left => (self.card_x() + CARD_W, self.card_y),
            Side::Right => (self.card_x(), self.card_y),
        }
    }
}

/// One step of a parsed path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Move(f32, f32),
    Line(f32, f32),
    /// Two control points, then the end point.
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Parse path data made of absolute `M`, `L`, `C` and `Z` commands. Returns
/// `None` for anything else, or for a command with the wrong number of
/// coordinates.
pub fn parse_path(d: &str) -> Option<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut tokens = d.split_whitespace().peekable();
    while let Some(command) = tokens.next() {
        let mut numbers = Vec::new();
        while let Some(n) = tokens.peek().and_then(|t| t.parse::<f32>().ok()) {
            numbers.push(n);
            tokens.next();
        }
        match (command, numbers.as_slice()) {
            ("M", [x, y]) => segments.push(Segment::Move(*x, *y)),
            ("L", [x, y]) => segments.push(Segment::Line(*x, *y)),
            ("C", [a, b, c, d, x, y]) => segments.push(Segment::Cubic(*a, *b, *c, *d, *x, *y)),
            ("Z", []) => segments.push(Segment::Close),
            _ => return None,
        }
    }
    Some(segments)
}
