//! Collect painted axis-aligned edges. Unpainted/clipping paths are discarded.
use super::fonts::number;
use super::malformed;
use crate::{Context, Error};
use lopdf::content::Operation;

#[derive(Clone, Copy, Debug)]
pub(super) struct Rule {
    // Vertical edges use the same interval representation: [left, right] is
    // their y extent, and y is their x coordinate.
    pub left: f64,
    pub right: f64,
    pub y: f64,
    pub vertical: bool,
    // Broad fills can be backgrounds inside an already ruled cell. Keep them
    // separate so their inset edges do not subdivide an explicit grid.
    pub background: bool,
}
#[derive(Default)]
pub(super) struct Path {
    current: Option<(f64, f64)>,
    start: Option<(f64, f64)>,
    strokes: Vec<Rule>,
    fills: Vec<Rule>,
    polygon: Vec<(f64, f64)>,
    bounds: Option<[f64; 4]>,
    curved: bool,
}
impl Path {
    fn filled_rectangle(&mut self) {
        let p = &self.polygon;
        let n = if p.len() == 5 && p[0] == p[4] {
            4
        } else {
            p.len()
        };
        if n != 4
            || (0..4).any(|i| {
                let a = p[i];
                let b = p[(i + 1) % 4];
                (a.0 - b.0).abs() > 0.25 && (a.1 - b.1).abs() > 0.25
            })
        {
            return;
        }
        let left = p.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let right = p.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let bottom = p.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let top = p.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        // Every vertex must be a different rectangle corner, excluding folded
        // or self-intersecting paths that happen to have rectangular bounds.
        if (0..4).any(|i| (0..i).any(|j| p[i] == p[j])) {
            return;
        }
        let background = right - left > 2. && top - bottom > 2.;
        if right - left > 0.5 {
            for y in if top - bottom <= 2. {
                vec![(top + bottom) / 2.]
            } else {
                vec![bottom, top]
            } {
                self.fills.push(Rule {
                    left,
                    right,
                    y,
                    vertical: false,
                    background,
                });
            }
        }
        if top - bottom > 0.5 {
            for x in if right - left <= 2. {
                vec![(right + left) / 2.]
            } else {
                vec![left, right]
            } {
                self.fills.push(Rule {
                    left: bottom,
                    right: top,
                    y: x,
                    vertical: true,
                    background,
                });
            }
        }
    }
    fn line(&mut self, end: (f64, f64)) {
        if !self.polygon.is_empty() && self.polygon.len() <= 5 {
            self.polygon.push(end);
        }
        if let Some((x, y)) = self.current {
            if (y - end.1).abs() < 0.25 && (x - end.0).abs() > 8. {
                self.strokes.push(Rule {
                    left: x.min(end.0),
                    right: x.max(end.0),
                    y: (y + end.1) / 2.,
                    vertical: false,
                    background: false,
                });
            } else if (x - end.0).abs() < 0.25 && (y - end.1).abs() > 8. {
                self.strokes.push(Rule {
                    left: y.min(end.1),
                    right: y.max(end.1),
                    y: (x + end.0) / 2.,
                    vertical: true,
                    background: false,
                });
            }
        }
        self.current = Some(end);
    }
    pub fn operation(
        &mut self,
        op: &Operation,
        transform: impl Fn(f64, f64) -> (f64, f64),
        rules: &mut Vec<Rule>,
        outlines: &mut Vec<[f64; 4]>,
        ctx: &mut Context<'_>,
    ) -> Result<(), Error> {
        let point = |i: usize| -> Result<(f64, f64), Error> {
            let x = number(
                op.operands
                    .get(i)
                    .ok_or_else(|| malformed("missing path operand"))?,
            )?;
            let y = number(
                op.operands
                    .get(i + 1)
                    .ok_or_else(|| malformed("missing path operand"))?,
            )?;
            let p = transform(x, y);
            if !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 1e12 || p.1.abs() > 1e12 {
                return Err(malformed("excessive PDF path coordinate"));
            }
            Ok(p)
        };
        let points = match op.operator.as_str() {
            "m" | "l" => 1,
            "c" => 3,
            "v" | "y" => 2,
            _ => 0,
        };
        for i in 0..points {
            let (x, y) = point(i * 2)?;
            let b = self.bounds.get_or_insert([x, y, x, y]);
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x);
            b[3] = b[3].max(y);
        }
        self.curved |= matches!(op.operator.as_str(), "c" | "v" | "y");
        match op.operator.as_str() {
            "m" => {
                self.filled_rectangle();
                let p = point(0)?;
                self.current = Some(p);
                self.start = Some(p);
                self.polygon = vec![p];
            }
            "l" => self.line(point(0)?),
            "c" => {
                self.current = Some(point(4)?);
                self.polygon.clear();
            }
            "v" | "y" => {
                self.current = Some(point(2)?);
                self.polygon.clear();
            }
            "h" => {
                if let Some(p) = self.start {
                    self.line(p);
                }
            }
            "re" => {
                self.filled_rectangle();
                self.polygon.clear();
                let mut v = [0.; 4];
                for (i, n) in v.iter_mut().enumerate() {
                    *n = number(
                        op.operands
                            .get(i)
                            .ok_or_else(|| malformed("missing rectangle operand"))?,
                    )?;
                }
                let [x, y, w, h] = v;
                let corners = [
                    transform(x, y),
                    transform(x + w, y),
                    transform(x + w, y + h),
                    transform(x, y + h),
                ];
                if corners.iter().any(|p| {
                    !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 1e12 || p.1.abs() > 1e12
                }) {
                    return Err(malformed("excessive PDF rectangle coordinate"));
                }
                self.current = Some(corners[0]);
                self.start = Some(corners[0]);
                for p in [corners[1], corners[2], corners[3], corners[0]] {
                    self.line(p);
                }
                if (corners[0].1 - corners[1].1).abs() < 0.25
                    && (corners[0].0 - corners[3].0).abs() < 0.25
                {
                    let (left, right) = (
                        corners[0].0.min(corners[1].0),
                        corners[0].0.max(corners[1].0),
                    );
                    let (bottom, top) = (
                        corners[0].1.min(corners[3].1),
                        corners[0].1.max(corners[3].1),
                    );
                    if right - left > 0.5 {
                        for y in if top - bottom <= 2. {
                            vec![(top + bottom) / 2.]
                        } else {
                            vec![bottom, top]
                        } {
                            self.fills.push(Rule {
                                left,
                                right,
                                y,
                                vertical: false,
                                background: top - bottom > 2.,
                            });
                        }
                    }
                    if top - bottom > 0.5 {
                        for x in if right - left <= 2. {
                            vec![(right + left) / 2.]
                        } else {
                            vec![left, right]
                        } {
                            self.fills.push(Rule {
                                left: bottom,
                                right: top,
                                y: x,
                                vertical: true,
                                background: right - left > 2.,
                            });
                        }
                    }
                }
            }
            "S" | "s" | "B" | "B*" | "b" | "b*" | "f" | "F" | "f*" | "n" => {
                let stroke = matches!(op.operator.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*");
                let fill = matches!(
                    op.operator.as_str(),
                    "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                );
                if matches!(op.operator.as_str(), "s" | "b" | "b*") {
                    if let Some(p) = self.start {
                        self.line(p);
                    }
                }
                if stroke {
                    for rule in self.strokes.drain(..) {
                        ctx.node()?;
                        rules.push(rule);
                    }
                }
                if fill {
                    if self.curved {
                        if let Some(b) = self.bounds {
                            outlines.push(b);
                        }
                    }
                    self.filled_rectangle();
                    // CSS/PDF borders are often trapezoids with mitred ends,
                    // or a rectangular ring rather than a `re` operation.
                    // Recover only paired, closely spaced parallel edges;
                    // arbitrary filled shapes must not become table rules.
                    self.strokes
                        .sort_by(|a, b| a.vertical.cmp(&b.vertical).then(a.y.total_cmp(&b.y)));
                    for (i, a) in self.strokes.iter().enumerate() {
                        for b in &self.strokes[i + 1..] {
                            if a.vertical != b.vertical || b.y - a.y > 2. {
                                break;
                            }
                            ctx.node()?;
                            if a.vertical == b.vertical
                                && (a.y - b.y).abs() > 0.05
                                && (a.y - b.y).abs() <= 2.
                                && a.right.min(b.right) - a.left.max(b.left)
                                    >= (a.right - a.left).max(b.right - b.left) * 0.8
                            {
                                self.fills.push(Rule {
                                    left: a.left.min(b.left),
                                    right: a.right.max(b.right),
                                    y: (a.y + b.y) / 2.,
                                    vertical: a.vertical,
                                    background: false,
                                });
                            }
                        }
                    }
                    for rule in self.fills.drain(..) {
                        ctx.node()?;
                        rules.push(rule);
                    }
                }
                *self = Self::default();
            }
            _ => {}
        }
        Ok(())
    }
}
