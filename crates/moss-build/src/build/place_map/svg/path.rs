use std::fmt::Write;

pub(super) fn snap(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

pub(super) fn serialize_path(paths: &[Vec<(f64, f64)>], closed: bool) -> Option<String> {
    let mut output = String::new();
    let mut cursor = (0i32, 0i32);
    let mut wrote = false;
    for path in paths {
        if path.len() < if closed { 3 } else { 2 } {
            continue;
        }
        let mut points: Vec<(i32, i32)> = path.iter().map(|&(x, y)| (snap(x), snap(y))).collect();
        points.dedup();
        if closed && points.first() == points.last() {
            points.pop();
        }
        if points.len() < if closed { 3 } else { 2 } {
            continue;
        }
        let start = points[0];
        write!(output, "m{} {}", start.0 - cursor.0, start.1 - cursor.1)
            .expect("writing to String cannot fail");
        cursor = start;
        for point in points.iter().skip(1) {
            write!(output, "l{} {}", point.0 - cursor.0, point.1 - cursor.1)
                .expect("writing to String cannot fail");
            cursor = *point;
        }
        if closed {
            output.push('z');
        }
        wrote = true;
    }
    wrote.then_some(output)
}
