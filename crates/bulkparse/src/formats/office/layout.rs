//! Conservative ordering of positioned objects. Unknown geometry is an order barrier.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub height: f64,
}
impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Option<Self> {
        [x, y, width, height]
            .iter()
            .all(|v| v.is_finite() && v.abs() < 1e18)
            .then_some(())?;
        (width > 0. && height > 0.).then_some(Self { x, y, height })
    }
}
pub(super) fn order(bounds: &[Option<Rect>]) -> Vec<usize> {
    let mut result: Vec<_> = (0..bounds.len()).collect();
    let mut start = 0;
    while start < result.len() {
        if bounds[start].is_none() {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < bounds.len() && bounds[end].is_some() {
            end += 1;
        }
        result[start..end].sort_by(|&a, &b| {
            bounds[a]
                .unwrap()
                .y
                .total_cmp(&bounds[b].unwrap().y)
                .then(a.cmp(&b))
        });
        let mut row = start;
        while row < end {
            let anchor = bounds[result[row]].unwrap();
            let mut last = row + 1;
            while last < end {
                let next = bounds[result[last]].unwrap();
                if next.y - anchor.y > 0.25 * anchor.height.min(next.height) {
                    break;
                }
                last += 1;
            }
            result[row..last].sort_by(|&a, &b| {
                bounds[a]
                    .unwrap()
                    .x
                    .total_cmp(&bounds[b].unwrap().x)
                    .then(a.cmp(&b))
            });
            row = last;
        }
        start = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bands_do_not_chain_and_unknowns_are_barriers() {
        let r = |x, y| Rect::new(x, y, 100., 100.);
        assert_eq!(order(&[r(20., 0.), r(10., 20.), r(0., 40.)]), vec![1, 0, 2]);
        assert_eq!(order(&[r(0., 30.), None, r(0., 0.)]), vec![0, 1, 2]);
        assert!(Rect::new(f64::NAN, 0., 1., 1.).is_none());
        assert!(Rect::new(0., 0., 0., 1.).is_none());
    }
}
