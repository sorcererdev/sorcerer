//! Cancel table.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelTableInput {
    pub todo_field: (),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelTableFailure {
    NotFound { reason: String },
}

#[derive(Default)]
pub struct CancelTable;

impl CancelTable {
    pub fn handle(&self, input: CancelTableInput) -> Result<(), CancelTableFailure> {
        todo!("hole handle: its first case, from the test below")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_table_succeeds() {
        let input = CancelTableInput { todo_field: Default::default() };
        assert_eq!(CancelTable.handle(input), Ok(Default::default()));
    }
}
