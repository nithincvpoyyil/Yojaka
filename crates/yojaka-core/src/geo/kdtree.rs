//! A static, implicit 3-D k-d tree (nearest-neighbour only). Built once, never modified.

#[derive(Debug)]
pub(crate) struct KdTree {
    /// Points in tree order (median layout); `ids[i]` is the caller's index of `pts[i]`.
    pts: Vec<[f64; 3]>,
    ids: Vec<u32>,
}

fn d2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

impl KdTree {
    pub fn build(points: Vec<[f64; 3]>) -> Self {
        let mut items: Vec<(u32, [f64; 3])> = points
            .into_iter()
            .enumerate()
            .map(|(i, p)| (i as u32, p))
            .collect();
        Self::arrange(&mut items, 0);
        Self {
            ids: items.iter().map(|(i, _)| *i).collect(),
            pts: items.into_iter().map(|(_, p)| p).collect(),
        }
    }

    fn arrange(items: &mut [(u32, [f64; 3])], axis: usize) {
        if items.len() <= 1 {
            return;
        }
        let mid = items.len() / 2;
        items.select_nth_unstable_by(mid, |a, b| a.1[axis].total_cmp(&b.1[axis]));
        let (l, r) = items.split_at_mut(mid);
        Self::arrange(l, (axis + 1) % 3);
        Self::arrange(&mut r[1..], (axis + 1) % 3);
    }

    /// Index of the nearest point and its squared chord distance.
    pub fn nearest(&self, q: [f64; 3]) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        self.search(0, self.pts.len(), 0, &q, &mut best);
        best.map(|(i, d)| (self.ids[i] as usize, d))
    }

    fn search(
        &self,
        lo: usize,
        hi: usize,
        axis: usize,
        q: &[f64; 3],
        best: &mut Option<(usize, f64)>,
    ) {
        if lo >= hi {
            return;
        }
        let mid = lo + (hi - lo) / 2;
        let d = d2(&self.pts[mid], q);
        if best.is_none_or(|(_, bd)| d < bd) {
            *best = Some((mid, d));
        }
        let diff = q[axis] - self.pts[mid][axis];
        let (near, far) = if diff < 0.0 {
            ((lo, mid), (mid + 1, hi))
        } else {
            ((mid + 1, hi), (lo, mid))
        };
        let next = (axis + 1) % 3;
        self.search(near.0, near.1, next, q, best);
        if best.is_none_or(|(_, bd)| diff * diff < bd) {
            self.search(far.0, far.1, next, q, best);
        }
    }
}
