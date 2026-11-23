//! The plans made and not applied yet, kept ten minutes.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::domain::plan::Plan;

/// How long a plan waits for its apply.
const KEPT: i64 = 10;

/// Plans by id, with when each was made.
#[derive(Default)]
pub(super) struct PlanCache {
    made: HashMap<String, (DateTime<Utc>, Plan)>,
}

impl PlanCache {
    /// Keeps a plan from `now`; plans older than ten minutes are dropped.
    pub(super) fn keep(&mut self, plan: Plan, now: DateTime<Utc>) {
        self.drop_expired(now);
        self.made.insert(plan.id.clone(), (now, plan));
    }

    /// Takes the plan out: a plan is applied once. `None` for an unknown or expired id.
    pub(super) fn take(&mut self, id: &str, now: DateTime<Utc>) -> Option<Plan> {
        self.drop_expired(now);
        self.made.remove(id).map(|(_, plan)| plan)
    }

    fn drop_expired(&mut self, now: DateTime<Utc>) {
        let oldest = now - Duration::minutes(KEPT);
        self.made.retain(|_, (made, _)| *made > oldest);
    }
}
