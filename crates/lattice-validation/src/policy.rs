//! Canonical policy ABI context. Runtime execution is a separate validation gate.
use crate::*;
#[derive(Debug, thiserror::Error)]
pub enum PolicyContextError {
    #[error("policy context field exceeds UInt32 length")]
    Length,
    #[error(transparent)]
    Encoding(#[from] lattice_cashew::Error),
}
#[derive(Debug, Clone)]
pub struct WasmPolicyContext<'a> {
    pub abi_version: u16,
    pub scope: PolicyScope,
    pub height: u64,
    pub timestamp: i64,
    pub chain_spec: &'a ChainSpec,
    pub chain_path: &'a [String],
    pub transaction: Option<&'a TransactionBody>,
    pub action: Option<&'a Action>,
    pub action_index: Option<u64>,
}
impl WasmPolicyContext<'_> {
    pub const CANONICAL_ENCODING_VERSION: u16 = 2;
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, PolicyContextError> {
        fn prefixed(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), PolicyContextError> {
            let len = u32::try_from(bytes.len()).map_err(|_| PolicyContextError::Length)?;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(bytes);
            Ok(())
        }
        fn optional<T: serde::Serialize>(
            out: &mut Vec<u8>,
            node: Option<&T>,
        ) -> Result<(), PolicyContextError> {
            match node {
                None => out.push(0),
                Some(node) => {
                    out.push(1);
                    prefixed(out, &canonical_bytes(node)?)?;
                }
            }
            Ok(())
        }
        let mut bytes = b"LWPCTX".to_vec();
        bytes.extend_from_slice(&Self::CANONICAL_ENCODING_VERSION.to_be_bytes());
        bytes.extend_from_slice(&self.abi_version.to_be_bytes());
        bytes.push(match self.scope {
            PolicyScope::Transaction => 0,
            PolicyScope::Action => 1,
        });
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.timestamp.to_be_bytes());
        prefixed(&mut bytes, &canonical_bytes(self.chain_spec)?)?;
        bytes.extend_from_slice(
            &u32::try_from(self.chain_path.len())
                .map_err(|_| PolicyContextError::Length)?
                .to_be_bytes(),
        );
        for component in self.chain_path {
            prefixed(&mut bytes, component.as_bytes())?;
        }
        optional(&mut bytes, self.transaction)?;
        optional(&mut bytes, self.action)?;
        match self.action_index {
            None => bytes.push(0),
            Some(index) => {
                bytes.push(1);
                bytes.extend_from_slice(&index.to_be_bytes());
            }
        }
        Ok(bytes)
    }
}

/// A completed deterministic failure versus a node that cannot decide yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyFailureVerdict {
    Invalid,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyBatchError {
    #[error("policy module {cid} is unavailable: {source}")]
    Unavailable {
        cid: String,
        source: lattice_cashew::Error,
    },
    #[error("invalid authenticated policy content: {0}")]
    Content(#[from] lattice_cashew::Error),
    #[error(transparent)]
    Context(#[from] PolicyContextError),
    #[error(transparent)]
    Execution(#[from] lattice_wasm::PolicyError),
}
impl PolicyBatchError {
    pub fn verdict(&self) -> PolicyFailureVerdict {
        use lattice_wasm::{PolicyError as P, PolicyExecutionError as E};
        match self {
            Self::Unavailable { .. } => PolicyFailureVerdict::Unavailable,
            // Instrumentation inconsistencies and worker failures are local
            // failures, never proof that the committed policy is invalid.
            Self::Execution(
                P::Instrumentation(_) | P::Execution(E::Worker(_) | E::MissingStackMeter),
            ) => PolicyFailureVerdict::Unavailable,
            Self::Content(_) | Self::Context(_) => PolicyFailureVerdict::Invalid,
            Self::Execution(
                P::UnsupportedAbi(_)
                | P::UnsupportedImports
                | P::Module(_)
                | P::Execution(
                    E::Engine(_) | E::MissingMemory | E::InvalidAllocation | E::ContextLength,
                ),
            ) => PolicyFailureVerdict::Invalid,
        }
    }
}

fn policy_modules(
    policies: &[&WasmPolicyRef],
    source: &impl lattice_cashew::ContentSource,
) -> Result<BTreeMap<String, WasmPolicyModule>, PolicyBatchError> {
    let mut modules = BTreeMap::new();
    for policy in policies {
        if modules.contains_key(&policy.module_cid) {
            continue;
        }
        let cid = lattice_cashew::canonical_cid(&policy.module_cid)?;
        // Separate fetching from authentication so even an arbitrary provider
        // error cannot masquerade as a completed validation verdict.
        let bytes = source
            .fetch(&cid)
            .map_err(|source| PolicyBatchError::Unavailable {
                cid: cid.clone(),
                source,
            })?;
        lattice_cashew::verify_content_address(&cid, &bytes)?;
        let module: WasmPolicyModule = lattice_cashew::cbor::decode(&bytes)?;
        modules.insert(policy.module_cid.clone(), module);
    }
    Ok(modules)
}

/// Genesis validates every configured entrypoint, even without transactions or
/// actions. Missing bytes remain retriable; deterministic module defects reject.
pub fn validate_configured_policy_modules(
    spec: &ChainSpec,
    source: &impl lattice_cashew::ContentSource,
) -> Result<bool, PolicyBatchError> {
    let policies: Vec<_> = spec.wasm_policies.iter().collect();
    if policies
        .iter()
        .any(|p| p.abi_version != lattice_wasm::WasmPolicy::ABI_VERSION)
    {
        return Ok(false);
    }
    let modules = policy_modules(&policies, source)?;
    for policy in policies {
        match lattice_wasm::WasmPolicy::new(
            &modules[&policy.module_cid].bytes,
            policy.abi_version,
            &policy.entrypoint,
        ) {
            Ok(_) => {}
            Err(error) => {
                let error = PolicyBatchError::Execution(error);
                if error.verdict() == PolicyFailureVerdict::Invalid {
                    return Ok(false);
                }
                return Err(error);
            }
        }
    }
    Ok(true)
}

pub struct PolicyBatch<'a> {
    pub bodies: &'a [TransactionBody],
    pub spec: &'a ChainSpec,
    pub chain_path: &'a [String],
    pub height: u64,
    pub timestamp: i64,
    /// None selects all policies; an empty slice selects none.
    pub scopes: Option<&'a [PolicyScope]>,
}
impl PolicyBatch<'_> {
    /// Module bytes are loaded once per distinct configured CID. Action scope
    /// applies to `body.actions` only, in their original order, as in Swift.
    pub fn verify(
        &self,
        source: &impl lattice_cashew::ContentSource,
    ) -> Result<bool, PolicyBatchError> {
        if self.chain_path.first().map(String::as_str) != Some("Nexus") {
            return Ok(false);
        }
        let policies: Vec<_> = self
            .spec
            .wasm_policies
            .iter()
            .filter(|p| self.scopes.is_none_or(|scopes| scopes.contains(&p.scope)))
            .collect();
        if policies
            .iter()
            .any(|p| p.abi_version != lattice_wasm::WasmPolicy::ABI_VERSION)
        {
            return Ok(false);
        }
        let modules = policy_modules(&policies, source)?;
        for policy in policies {
            let mut executable = None;
            for body in self.bodies {
                let contexts: Vec<_> = match policy.scope {
                    PolicyScope::Transaction => vec![(None, None)],
                    PolicyScope::Action => body
                        .actions
                        .iter()
                        .enumerate()
                        .map(|(i, a)| (Some(a), Some(i as u64)))
                        .collect(),
                };
                for (action, action_index) in contexts {
                    let context = WasmPolicyContext {
                        abi_version: policy.abi_version,
                        scope: policy.scope,
                        height: self.height,
                        timestamp: self.timestamp,
                        chain_spec: self.spec,
                        chain_path: self.chain_path,
                        transaction: Some(body),
                        action,
                        action_index,
                    }
                    .canonical_bytes()?;
                    if executable.is_none() {
                        executable = Some(lattice_wasm::WasmPolicy::new(
                            &modules[&policy.module_cid].bytes,
                            policy.abi_version,
                            &policy.entrypoint,
                        )?);
                    }
                    if !executable
                        .as_ref()
                        .expect("initialized above")
                        .evaluate(&context)?
                    {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
}
