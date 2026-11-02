use lattice_cashew::ContentSource;
use lattice_primitives::*;
use std::collections::BTreeSet;

/// Signature and action-shape admission only. Policies, chain-path binding,
/// withdrawals' state witnesses and nonce floors are checked by block admission.
pub fn validate_transaction(
    transaction: &Transaction,
    source: &impl ContentSource,
    genesis: bool,
) -> Result<Option<TransactionBody>, lattice_cashew::Error> {
    let resolved = transaction.body.resolve(source, None)?;
    let body = resolved
        .node
        .as_deref()
        .ok_or_else(|| lattice_cashew::Error::MissingContent(resolved.raw_cid.clone()))?;
    if !body.state_atoms_are_valid()
        || !body.account_actions_are_valid()
        || !body.actions.iter().all(Action::verify)
    {
        return Ok(None);
    }
    if genesis {
        return Ok((body.deposit_actions.is_empty()
            && body.receipt_actions.is_empty()
            && body.withdrawal_actions.is_empty())
        .then(|| body.clone()));
    }
    if !body.deposit_actions_are_valid()
        || !body.receipt_actions_are_valid()
        || !body.withdrawal_actions_are_valid()
    {
        return Ok(None);
    }
    let Some(signatures) = transaction.normalized_signatures() else {
        return Ok(None);
    };
    if signatures.is_empty() {
        return Ok(None);
    }
    let preimage = signing_preimage(&resolved.raw_cid, &body.chain_path, body.nonce);
    let mut addresses = BTreeSet::new();
    for (key, signature) in signatures {
        if !crypto::verify(&preimage, &signature, &key) {
            return Ok(None);
        }
        addresses.insert(crypto::create_address(&key)?);
    }
    let expected: BTreeSet<_> = body.signers.iter().cloned().collect();
    Ok((addresses == expected).then(|| body.clone()))
}
