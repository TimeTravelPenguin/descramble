use crate::{Error, Result};

pub(crate) fn validate_order(order: &[usize], count: usize) -> Result<()> {
    let mut seen = vec![false; count];

    if order.len() != count {
        return Err(Error::InvalidInput(
            "permutation has the wrong length".into(),
        ));
    }

    for &item in order {
        if item >= count || seen[item] {
            return Err(Error::InvalidInput("order must be a permutation".into()));
        }

        seen[item] = true;
    }

    Ok(())
}
