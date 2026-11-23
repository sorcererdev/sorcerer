//! Clock in memory: for tests, and until a real adapter exists.

use super::Clock;

#[derive(Default)]
pub struct InMemoryClock;

impl Clock for InMemoryClock {
    fn todo_method(&self) -> () {
        todo!("hole body: what that method does in memory")
    }
}
