//! Single-transaction classification against immutable tip content. The host
//! must independently require this tip on its executed-from-genesis frontier,
//! bind the chain path/parent context, and fence completion against tip changes.
use crate::*;
use lattice_cashew::ContentSource;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightDisposition {
    Ready,
    Future,
    Invalid,
    Unavailable,
}
/// A conditional classification, not a block-admission or graph-execution proof.
/// Fields are private so an admission job cannot replace authenticated balances.
#[derive(Debug)]
pub struct TransactionPreflightResult {
    transaction_cid: String,
    tip_cid: String,
    parent_state_cid: Option<String>,
    chain_path: Vec<String>,
    checks_policies: bool,
    disposition: PreflightDisposition,
    debit_balances: BTreeMap<String, u64>,
}
impl TransactionPreflightResult {
    pub fn transaction_cid(&self) -> &str {
        &self.transaction_cid
    }
    pub fn tip_cid(&self) -> &str {
        &self.tip_cid
    }
    pub fn parent_state_cid(&self) -> Option<&str> {
        self.parent_state_cid.as_deref()
    }
    pub fn chain_path(&self) -> &[String] {
        &self.chain_path
    }
    pub fn checks_policies(&self) -> bool {
        self.checks_policies
    }
    pub fn disposition(&self) -> PreflightDisposition {
        self.disposition
    }
    pub fn debit_balances(&self) -> &BTreeMap<String, u64> {
        &self.debit_balances
    }
}
pub struct TransactionPreflight<'a> {
    pub transaction: &'a Transaction,
    pub tip: &'a BlockHeader,
    pub chain_path: &'a [String],
    pub parent_state: Option<&'a LatticeStateHeader>,
    pub now_milliseconds: i64,
    pub checks_policies: bool,
}
struct Source<'a, S> {
    source: &'a S,
    failed: Mutex<bool>,
}
impl<S: ContentSource> ContentSource for Source<'_, S> {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
        self.source
            .fetch(cid)
            .inspect_err(|_| *self.failed.lock().expect("source lock") = true)
    }
}
impl TransactionPreflight<'_> {
    pub fn classify(
        &self,
        source: &impl ContentSource,
    ) -> Result<TransactionPreflightResult, lattice_cashew::Error> {
        // Canonical envelope identity is required even for a refusal result.
        let transaction_cid = content_id(self.transaction)?;
        let source = Source {
            source,
            failed: Mutex::new(false),
        };
        let mut debit_balances = BTreeMap::new();
        let verdict = self.classify_inner(&source, &mut debit_balances);
        let disposition = if source.failed.into_inner().expect("source lock") {
            PreflightDisposition::Unavailable
        } else {
            match verdict {
                Ok(d) => d,
                Err(error) if error.is_retriable() => PreflightDisposition::Unavailable,
                Err(_) => PreflightDisposition::Invalid,
            }
        };
        if disposition != PreflightDisposition::Ready {
            debit_balances.clear();
        }
        Ok(TransactionPreflightResult {
            transaction_cid,
            tip_cid: self.tip.0.raw_cid.clone(),
            parent_state_cid: self.parent_state.map(|s| s.0.raw_cid.clone()),
            chain_path: self.chain_path.to_vec(),
            checks_policies: self.checks_policies,
            disposition,
            debit_balances,
        })
    }
    fn classify_inner(
        &self,
        source: &impl ContentSource,
        balances: &mut BTreeMap<String, u64>,
    ) -> Result<PreflightDisposition, BlockValidationFailure> {
        use PreflightDisposition::*;
        // Authenticate the supplied immutable snapshot; execution is a separate
        // host graph condition and cannot be inferred from these bytes.
        let tip = authenticated::load(&self.tip.0, source)?;
        let Some(body) = validate_transaction(self.transaction, source, false)? else {
            return Ok(Invalid);
        };
        if body.miner_surplus().is_none()
            || body.chain_path != self.chain_path
            || self.chain_path.len() <= 1
                && (!body.deposit_actions.is_empty() || !body.withdrawal_actions.is_empty())
        {
            return Ok(Invalid);
        }
        let spec = authenticated::load(&tip.spec.0, source)?;
        if !spec.is_valid()
            || state_growth_floor(std::slice::from_ref(&body))? > spec.max_state_growth
        {
            return Ok(Invalid);
        }
        let judged = |disposition| -> Result<PreflightDisposition, BlockValidationFailure> {
            if !self.checks_policies {
                return Ok(disposition);
            }
            let Some(timestamp) = tip.timestamp.checked_add(1) else {
                return Ok(Unavailable);
            };
            let Some(height) = tip.height.checked_add(1) else {
                return Ok(Unavailable);
            };
            if !(PolicyBatch {
                bodies: std::slice::from_ref(&body),
                spec: &spec,
                chain_path: self.chain_path,
                height,
                timestamp: timestamp.max(self.now_milliseconds),
                scopes: None,
            })
            .verify(source)?
            {
                return Ok(Invalid);
            }
            Ok(disposition)
        };
        let state = authenticated::load(&tip.post_state.0, source)?;
        if let Some(signer) = body.signers.first() {
            let key = format!("{NONCE_PREFIX}{signer}");
            let stored = authenticated::targeted(
                &state.account_state,
                &BTreeSet::from([key.clone()]),
                source,
            )?
            .1
            .get(&key)
            .copied();
            let expected = next_expected_nonce(stored).map_err(TransitionError::from)?;
            if body.nonce < expected {
                return Ok(Invalid);
            }
            if body.nonce > expected {
                return judged(Future);
            }
        }
        if !body.withdrawal_actions.is_empty() {
            let Some(parent_header) = self.parent_state else {
                return judged(Unavailable);
            };
            let parent = authenticated::load(&parent_header.0, source)?;
            let Some(directory) = self.chain_path.last() else {
                return judged(Unavailable);
            };
            for action in &body.withdrawal_actions {
                let receipt = ReceiptKey {
                    directory: directory.clone(),
                    demander: action.demander.clone(),
                    amount_demanded: action.amount_demanded,
                    nonce: action.nonce,
                }
                .storage_key();
                let deposit = DepositKey {
                    demander: action.demander.clone(),
                    amount_demanded: action.amount_demanded,
                    nonce: action.nonce,
                }
                .to_string();
                let receipts = authenticated::targeted(
                    &parent.receipt_state,
                    &BTreeSet::from([receipt.clone()]),
                    source,
                )?
                .1;
                let deposits = authenticated::targeted(
                    &state.deposit_state,
                    &BTreeSet::from([deposit.clone()]),
                    source,
                )?
                .1;
                if receipts
                    .get(&receipt)
                    .is_some_and(|w| w != &action.withdrawer)
                {
                    return Ok(Invalid);
                }
                if !receipts.contains_key(&receipt) || !deposits.contains_key(&deposit) {
                    return Ok(Unavailable);
                }
            }
            validate_authenticated_withdrawals(
                &tip.post_state,
                parent_header,
                directory,
                &body.withdrawal_actions,
                source,
            )?;
        }
        let actions = TransitionActions::from_bodies(std::slice::from_ref(&body));
        AuthenticatedBlockExecution {
            previous: &tip.post_state,
            parent: self.parent_state.unwrap_or(&tip.post_state),
            directory: self.chain_path.last().map(String::as_str).unwrap_or(""),
            actions: &actions,
            transaction_bodies: std::slice::from_ref(&body),
            reward: 0,
            recipient: None,
        }
        .execute_template(source, None)?;
        let disposition = judged(Ready)?;
        if disposition == Ready {
            let keys = body
                .net_balance_deltas()
                .map_err(TransitionError::from)?
                .into_iter()
                .filter_map(|(key, delta)| {
                    matches!(delta, AccountBalanceDelta::Debit(_)).then_some(key)
                })
                .collect::<BTreeSet<_>>();
            let values = authenticated::targeted(&state.account_state, &keys, source)?.1;
            for key in keys {
                let value = values.get(&key).copied().unwrap_or(0);
                balances.insert(key, value);
            }
        }
        Ok(disposition)
    }
}
