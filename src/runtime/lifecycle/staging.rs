//! Native writer staging profile: at most 8 MiB of retained payload bytes per
//! batch. Readers remain unchanged. Account for every owned string/path before
//! appending; one bounded candidate may exist temporarily beyond this budget.
use crate::{Error, Result, error::require};

pub(in crate::runtime) const MAX_STAGING: usize = 8 * 1_048_576;

pub(in crate::runtime) struct Staged<T> {
    items: Vec<T>,
    bytes: usize,
    limit: usize,
}

impl<T> Staged<T> {
    pub(in crate::runtime) fn new(limit: usize) -> Self {
        Self {
            items: Vec::new(),
            bytes: 0,
            limit,
        }
    }

    pub(in crate::runtime) fn push(&mut self, item: T, sizes: &[usize]) -> Result<()> {
        let bytes = sizes.iter().try_fold(self.bytes, |bytes, size| {
            bytes
                .checked_add(*size)
                .ok_or(Error("unsupported_runtime_staging_profile"))
        })?;
        require(bytes <= self.limit, "unsupported_runtime_staging_profile")?;
        self.items.push(item);
        self.bytes = bytes;
        Ok(())
    }

    pub(in crate::runtime) fn finish(self) -> Vec<T> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_cap_accepts_and_one_over_refuses_before_append() {
        let mut draft = Staged::new(8);
        draft.push("first", &[2, 2]).unwrap();
        draft.push("at cap", &[1, 3]).unwrap();
        assert_eq!(draft.bytes, 8);
        assert_eq!(
            draft.push("over cap", &[1]).unwrap_err().code(),
            "unsupported_runtime_staging_profile"
        );
        assert_eq!(draft.bytes, 8);
        assert_eq!(draft.finish(), ["first", "at cap"]);
    }

    #[test]
    fn checked_sum_overflow_refuses_before_append_or_accounting_change() {
        let mut draft = Staged::new(usize::MAX);
        draft.push("first", &[1]).unwrap();
        assert_eq!(
            draft.push("overflow", &[usize::MAX]).unwrap_err().code(),
            "unsupported_runtime_staging_profile"
        );
        assert_eq!(draft.bytes, 1);
        assert_eq!(draft.finish(), ["first"]);
        let mut draft = Staged::new(usize::MAX);
        assert!(
            draft
                .push("overflow within item", &[usize::MAX, 1])
                .is_err()
        );
        assert_eq!(draft.bytes, 0);
        assert!(draft.finish().is_empty());
    }
}
