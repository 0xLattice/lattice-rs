//! Consensus state-growth accounting (§6.6), separate from encoded block size.
use crate::*;
fn length(value: &str) -> Result<i64, TransitionError> {
    i64::try_from(value.len()).map_err(|_| TransitionError::GrowthOverflow)
}
fn add(total: &mut i64, value: i64) -> Result<(), TransitionError> {
    *total = total
        .checked_add(value)
        .ok_or(TransitionError::GrowthOverflow)?;
    Ok(())
}
fn account_size(owner: &str) -> Result<i64, TransitionError> {
    length(owner)?
        .checked_add(8)
        .ok_or(TransitionError::GrowthOverflow)
}
pub fn action_state_delta(body: &TransactionBody) -> Result<i64, TransitionError> {
    let mut total = 0;
    for action in &body.actions {
        add(
            &mut total,
            action
                .state_delta()
                .ok_or(TransitionError::GrowthOverflow)?,
        )?;
    }
    for action in &body.deposit_actions {
        // Admitted atoms are ASCII; Swift counts Characters for this one field.
        if !account_atom(&action.demander) {
            return Err(TransitionError::Conflict);
        }
        add(&mut total, 32)?;
        add(&mut total, length(&action.demander)?)?;
    }
    for action in &body.receipt_actions {
        add(&mut total, 24)?;
        for value in [&action.withdrawer, &action.demander, &action.directory] {
            add(&mut total, length(value)?)?;
        }
    }
    for action in &body.withdrawal_actions {
        add(&mut total, 32)?;
        for value in [&action.withdrawer, &action.demander] {
            add(&mut total, length(value)?)?;
        }
    }
    Ok(total)
}
pub fn state_growth(
    bodies: &[TransactionBody],
    account_growth: i64,
) -> Result<i64, TransitionError> {
    let mut total = account_growth;
    for body in bodies {
        add(&mut total, action_state_delta(body)?)?;
    }
    Ok(total)
}
pub fn state_growth_floor(bodies: &[TransactionBody]) -> Result<i64, TransitionError> {
    let actions = TransitionActions::from_bodies(bodies);
    let mut removable = 0i64;
    for (owner, delta) in net_balance_deltas(&actions.accounts, &actions.receipts)? {
        if matches!(delta, AccountBalanceDelta::Debit(_)) {
            add(&mut removable, account_size(&owner)?)?;
        }
    }
    state_growth(bodies, -removable)
}
pub fn state_growth_ceiling(
    bodies: &[TransactionBody],
    recipient: Option<&str>,
) -> Result<i64, TransitionError> {
    let mut created = recipient.map(account_size).transpose()?.unwrap_or(0);
    for body in bodies {
        if !body.actions.iter().all(Action::verify) {
            return Err(TransitionError::Conflict);
        }
        for (owner, delta) in body.net_balance_deltas()? {
            if matches!(delta, AccountBalanceDelta::Credit(_)) {
                add(&mut created, account_size(&owner)?)?;
            }
        }
        if let Some(signer) = body.signers.first() {
            add(
                &mut created,
                account_size(&format!("{NONCE_PREFIX}{signer}"))?,
            )?;
        }
    }
    state_growth(bodies, created)
}
