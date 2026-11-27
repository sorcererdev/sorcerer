//! What an offer says a task will cost before it runs.

/// After this many finished tasks, the estimate is their median.
pub const TASKS_BEFORE_MEDIAN: usize = 5;

/// The estimate in dollars: the median of the project's finished tasks once there are five,
/// else the configured start (0.50 by default).
///
/// Example: tasks of $0.30, $0.40, $0.71, $0.90 and $2.00 give $0.71.
pub fn estimate_usd(finished_costs: &[f64], start: f64) -> f64 {
    if finished_costs.len() < TASKS_BEFORE_MEDIAN {
        return start;
    }
    let mut sorted = finished_costs.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_until_five_then_the_median() {
        assert_eq!(estimate_usd(&[1.0, 2.0], 0.5), 0.5);
        assert_eq!(estimate_usd(&[2.0, 0.3, 0.71, 0.9, 0.4], 0.5), 0.71);
        assert_eq!(estimate_usd(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 0.5), 3.5);
    }
}
