//! Least-squares polynomial fits between control points, in Web Mercator
//! coordinates (the space georeference annotations are solved in).

/// A 2-D polynomial map of order 1 to 3 from `(x, y)` to `(x', y')`, fitted on
/// inputs centred on their mean and scaled by their spread.
#[derive(Debug, Clone)]
pub(crate) struct Poly {
    order: u8,
    mean: (f64, f64),
    spread: (f64, f64),
    coef: [Vec<f64>; 2],
}

fn terms(order: u8) -> Vec<(u8, u8)> {
    (0..=order).flat_map(|total| (0..=total).map(move |i| (total - i, i))).collect()
}

/// How many control points an order needs.
pub(crate) fn terms_needed(order: u8) -> usize {
    terms(order).len()
}

impl Poly {
    pub(crate) fn eval(&self, x: f64, y: f64) -> (f64, f64) {
        let (nx, ny) = ((x - self.mean.0) / self.spread.0, (y - self.mean.1) / self.spread.1);
        let (mut ox, mut oy) = (0.0, 0.0);
        for (k, &(i, j)) in terms(self.order).iter().enumerate() {
            let basis = nx.powi(i32::from(i)) * ny.powi(i32::from(j));
            ox += self.coef[0][k] * basis;
            oy += self.coef[1][k] * basis;
        }
        (ox, oy)
    }
}

/// The least-squares `order` polynomial taking each `src` point to its `dst`
/// point; `None` when the points cannot determine it.
pub(crate) fn fit(order: u8, src: &[(f64, f64)], dst: &[(f64, f64)]) -> Option<Poly> {
    let n = src.len();
    let basis_terms = terms(order);
    if n != dst.len() || n < basis_terms.len() {
        return None;
    }
    let nf = n as f64;
    let mean = (src.iter().map(|p| p.0).sum::<f64>() / nf, src.iter().map(|p| p.1).sum::<f64>() / nf);
    let spread_of = |pick: fn(&(f64, f64)) -> f64, m: f64| (src.iter().map(|p| (pick(p) - m).powi(2)).sum::<f64>() / nf).sqrt();
    let spread = (spread_of(|p| p.0, mean.0), spread_of(|p| p.1, mean.1));
    if !(spread.0 > 0.0 && spread.1 > 0.0) {
        return None;
    }
    let dst_mean = (dst.iter().map(|p| p.0).sum::<f64>() / nf, dst.iter().map(|p| p.1).sum::<f64>() / nf);
    let rows: Vec<Vec<f64>> = src
        .iter()
        .map(|&(x, y)| {
            let (nx, ny) = ((x - mean.0) / spread.0, (y - mean.1) / spread.1);
            basis_terms.iter().map(|&(i, j)| nx.powi(i32::from(i)) * ny.powi(i32::from(j))).collect()
        })
        .collect();
    let k = basis_terms.len();
    let mut normal = vec![vec![0.0; k + 2]; k];
    for (row, d) in rows.iter().zip(dst) {
        for a in 0..k {
            for b in 0..k {
                normal[a][b] += row[a] * row[b];
            }
            normal[a][k] += row[a] * (d.0 - dst_mean.0);
            normal[a][k + 1] += row[a] * (d.1 - dst_mean.1);
        }
    }
    let solved = solve(normal, k, 2)?;
    let mut coef = [Vec::new(), Vec::new()];
    for (axis, c) in coef.iter_mut().enumerate() {
        *c = (0..k).map(|a| solved[a][axis]).collect();
    }
    // The constant term (basis 1) carries the destination mean.
    coef[0][0] += dst_mean.0;
    coef[1][0] += dst_mean.1;
    Some(Poly { order, mean, spread, coef })
}

/// Gaussian elimination with partial pivoting on `k` rows of `k` unknown
/// columns followed by `rhs` right-hand columns.
fn solve(mut m: Vec<Vec<f64>>, k: usize, rhs: usize) -> Option<Vec<Vec<f64>>> {
    for col in 0..k {
        let pivot = (col..k).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        for row in 0..k {
            if row != col {
                let factor = m[row][col] / m[col][col];
                for c in col..k + rhs {
                    m[row][c] -= factor * m[col][c];
                }
            }
        }
    }
    Some((0..k).map(|row| (0..rhs).map(|r| m[row][k + r] / m[row][row]).collect()).collect())
}

/// Web Mercator coordinates (radians) of a latitude/longitude in degrees.
pub(crate) fn mercator(latitude: f64, longitude: f64) -> (f64, f64) {
    (longitude.to_radians(), (std::f64::consts::FRAC_PI_4 + latitude.to_radians() / 2.0).tan().ln())
}

/// `(latitude, longitude)` in degrees of Web Mercator coordinates.
pub(crate) fn unmercator(x: f64, y: f64) -> (f64, f64) {
    ((2.0 * y.exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(), x.to_degrees())
}

/// Ground distance in metres between two latitude/longitude points.
pub(crate) fn ground_metres(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la, lb) = (a.0.to_radians(), b.0.to_radians());
    let h = ((lb - la) / 2.0).sin().powi(2) + la.cos() * lb.cos() * ((b.1 - a.1).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_affine_map_is_recovered_exactly() {
        let truth = |x: f64, y: f64| (2.0 * x - 0.5 * y + 7.0, 0.25 * x + 3.0 * y - 1.0);
        let src: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0), (4.0, 7.0)].into();
        let dst: Vec<_> = src.iter().map(|&(x, y)| truth(x, y)).collect();
        let poly = fit(1, &src, &dst).unwrap();
        for (x, y) in [(3.3, 8.1), (-5.0, 20.0), (100.0, -40.0)] {
            let (px, py) = poly.eval(x, y);
            let (tx, ty) = truth(x, y);
            assert!((px - tx).abs() < 1e-8 && (py - ty).abs() < 1e-8, "({x},{y}) -> ({px},{py}) not ({tx},{ty})");
        }
    }

    #[test]
    fn a_quadratic_is_recovered_at_order_two() {
        let truth = |x: f64, y: f64| (x * x * 0.01 + y, x * y * 0.02 - x);
        let src: Vec<_> = (0..4).flat_map(|i| (0..3).map(move |j| (f64::from(i) * 5.0, f64::from(j) * 7.0))).collect();
        let dst: Vec<_> = src.iter().map(|&(x, y)| truth(x, y)).collect();
        let poly = fit(2, &src, &dst).unwrap();
        let (px, py) = poly.eval(6.0, 9.0);
        let (tx, ty) = truth(6.0, 9.0);
        assert!((px - tx).abs() < 1e-8 && (py - ty).abs() < 1e-8);
    }

    #[test]
    fn too_few_or_collinear_points_do_not_fit() {
        assert!(fit(1, &[(0.0, 0.0), (1.0, 1.0)], &[(0.0, 0.0), (1.0, 1.0)]).is_none());
        let line: Vec<_> = (0..5).map(|i| (f64::from(i), f64::from(i))).collect();
        assert!(fit(1, &line, &line).is_none());
    }

    #[test]
    fn mercator_round_trips() {
        let (x, y) = mercator(40.7, -74.0);
        let (la, lo) = unmercator(x, y);
        assert!((la - 40.7).abs() < 1e-9 && (lo + 74.0).abs() < 1e-9);
    }

    #[test]
    fn a_degree_of_latitude_is_about_111_km() {
        let d = ground_metres((40.0, -74.0), (41.0, -74.0));
        assert!((d - 111_195.0).abs() < 50.0, "{d}");
    }
}
