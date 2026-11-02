//! Deterministic WASM policies with the pinned Swift opcode and stack rules.
//!
//! `scan_deterministic_module` checks syntax/types and the opcode subset only.
//! `WasmPolicy::new` additionally checks the eager runtime and policy ABI; each
//! evaluation starts a fresh instance. Module CID authentication belongs to the
//! caller and must use the original bytes, never the internal instrumented bytes.
mod instrument;
mod runtime;
mod stack_profile;
pub use runtime::RuntimeError as PolicyExecutionError;

use wasmparser::{
    BlockType, DataKind, ElementItems, ElementKind, Operator, Parser, Payload, ValType,
};

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("malformed or unsupported WASM: {0}")]
    Binary(#[from] wasmparser::BinaryReaderError),
    #[error("construct outside the deterministic policy subset: {0}")]
    Forbidden(&'static str),
}

fn value_type(ty: ValType) -> Result<(), ModuleError> {
    match ty {
        ValType::I32 | ValType::I64 => Ok(()),
        ValType::Ref(ty)
            if ty == wasmparser::RefType::FUNCREF || ty == wasmparser::RefType::EXTERNREF =>
        {
            Ok(())
        }
        _ => Err(ModuleError::Forbidden("value type")),
    }
}

fn operators(mut reader: wasmparser::OperatorsReader<'_>) -> Result<(), ModuleError> {
    while !reader.eof() {
        operator(&reader.read()?)?;
    }
    Ok(())
}

fn operator(op: &Operator<'_>) -> Result<(), ModuleError> {
    use Operator::*;
    match op {
        Unreachable
        | Nop
        | Else
        | End
        | Br { .. }
        | BrIf { .. }
        | BrTable { .. }
        | Return
        | Call { .. }
        | CallIndirect { .. }
        | Drop
        | Select
        | LocalGet { .. }
        | LocalSet { .. }
        | LocalTee { .. }
        | GlobalGet { .. }
        | GlobalSet { .. }
        | MemorySize { .. }
        | MemoryGrow { .. }
        | I32Const { .. }
        | I64Const { .. }
        | I32Eqz
        | I64Eqz
        | MemoryInit { .. }
        | DataDrop { .. }
        | MemoryCopy { .. }
        | MemoryFill { .. }
        | RefNull { .. }
        | RefIsNull
        | RefFunc { .. }
        | I32Load { .. }
        | I64Load { .. }
        | I32Load8S { .. }
        | I32Load8U { .. }
        | I32Load16S { .. }
        | I32Load16U { .. }
        | I64Load8S { .. }
        | I64Load8U { .. }
        | I64Load16S { .. }
        | I64Load16U { .. }
        | I64Load32S { .. }
        | I64Load32U { .. }
        | I32Store { .. }
        | I64Store { .. }
        | I32Store8 { .. }
        | I32Store16 { .. }
        | I64Store8 { .. }
        | I64Store16 { .. }
        | I64Store32 { .. }
        | I32Eq
        | I32Ne
        | I32LtS
        | I32LtU
        | I32GtS
        | I32GtU
        | I32LeS
        | I32LeU
        | I32GeS
        | I32GeU
        | I64Eq
        | I64Ne
        | I64LtS
        | I64LtU
        | I64GtS
        | I64GtU
        | I64LeS
        | I64LeU
        | I64GeS
        | I64GeU
        | I32Clz
        | I32Ctz
        | I32Popcnt
        | I64Clz
        | I64Ctz
        | I64Popcnt
        | I32Extend8S
        | I32Extend16S
        | I64Extend8S
        | I64Extend16S
        | I64Extend32S
        | I32Add
        | I32Sub
        | I32Mul
        | I32DivS
        | I32DivU
        | I32RemS
        | I32RemU
        | I32And
        | I32Or
        | I32Xor
        | I32Shl
        | I32ShrS
        | I32ShrU
        | I32Rotl
        | I32Rotr
        | I64Add
        | I64Sub
        | I64Mul
        | I64DivS
        | I64DivU
        | I64RemS
        | I64RemU
        | I64And
        | I64Or
        | I64Xor
        | I64Shl
        | I64ShrS
        | I64ShrU
        | I64Rotl
        | I64Rotr
        | I32WrapI64
        | I64ExtendI32S
        | I64ExtendI32U => Ok(()),
        Block { blockty } | Loop { blockty } | If { blockty } => {
            if let BlockType::Type(ty) = blockty {
                value_type(*ty)?;
            }
            Ok(())
        }
        TypedSelect { ty } => value_type(*ty),
        // Fail closed on every extension, including newly added parser opcodes.
        _ => Err(ModuleError::Forbidden("instruction")),
    }
}

/// Scan and type-check a module without executing it. No resource or fuel limits
/// are added here; policy runtime stack accounting is a separate protocol rule.
pub fn scan_deterministic_module(bytes: &[u8]) -> Result<(), ModuleError> {
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    let ty = ty?;
                    for val in ty.params().iter().chain(ty.results()) {
                        value_type(*val)?;
                    }
                }
            }
            Payload::ImportSection(imports) => {
                for import in imports.into_imports() {
                    if let wasmparser::TypeRef::Global(global) = import?.ty {
                        value_type(global.content_type)?;
                    }
                }
            }
            Payload::GlobalSection(globals) => {
                for global in globals {
                    let global = global?;
                    value_type(global.ty.content_type)?;
                    operators(global.init_expr.get_operators_reader())?;
                }
            }
            Payload::ElementSection(elements) => {
                for element in elements {
                    let element = element?;
                    if let ElementKind::Active { offset_expr, .. } = element.kind {
                        operators(offset_expr.get_operators_reader())?;
                    }
                    if let ElementItems::Expressions(ty, expressions) = element.items {
                        value_type(ValType::Ref(ty))?;
                        for expression in expressions {
                            operators(expression?.get_operators_reader())?;
                        }
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                for local in body.get_locals_reader()? {
                    value_type(local?.1)?;
                }
                operators(body.get_operators_reader()?)?;
            }
            Payload::DataSection(segments) => {
                for segment in segments {
                    if let DataKind::Active { offset_expr, .. } = segment?.kind {
                        operators(offset_expr.get_operators_reader())?;
                    }
                }
            }
            _ => {}
        }
    }
    // WasmKit's referenceTypes feature permits multi-value and sign extension.
    // Its scanner independently rejects floats, SIMD, table ops and tail calls.
    let features = wasmparser::WasmFeatures::WASM2.difference(
        wasmparser::WasmFeatures::SIMD
            | wasmparser::WasmFeatures::FLOATS
            | wasmparser::WasmFeatures::SATURATING_FLOAT_TO_INT,
    );
    wasmparser::Validator::new_with_features(features).validate_all(bytes)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("unsupported WASM policy ABI version {0}")]
    UnsupportedAbi(u16),
    #[error("policy host imports are not supplied")]
    UnsupportedImports,
    #[error(transparent)]
    Module(#[from] ModuleError),
    #[error("WASM stack instrumentation failed: {0}")]
    Instrumentation(String),
    #[error(transparent)]
    Execution(#[from] PolicyExecutionError),
}

/// Parsed and checked policy. This is not a content-authentication capability;
/// callers must bind the supplied bytes to the chain's committed module CID.
pub struct WasmPolicy {
    module: instrument::InstrumentedModule,
    entrypoint: String,
}
impl WasmPolicy {
    pub const ABI_VERSION: u16 = 1;

    /// Eagerly validates every function, instantiates (including start), and
    /// requires memory, allocator and entrypoint exports with exact signatures.
    pub fn new(bytes: &[u8], abi_version: u16, entrypoint: &str) -> Result<Self, PolicyError> {
        if abi_version != Self::ABI_VERSION {
            return Err(PolicyError::UnsupportedAbi(abi_version));
        }
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ImportSection(imports) = payload.map_err(ModuleError::from)?
                && imports.count() != 0
            {
                return Err(PolicyError::UnsupportedImports);
            }
        }
        let profiles = stack_profile::profile_module(bytes)?;
        let module =
            instrument::instrument(bytes, &profiles).map_err(PolicyError::Instrumentation)?;
        runtime::validate(&module, entrypoint)?;
        Ok(Self {
            module,
            entrypoint: entrypoint.into(),
        })
    }

    /// Invokes the allocator and policy with the caller's canonical ABI bytes.
    /// Only i32(1) accepts; all other returned integers reject. Traps are errors.
    /// There is no fuel or time limit: these are not part of the pinned protocol.
    pub fn evaluate(&self, canonical_context: &[u8]) -> Result<bool, PolicyError> {
        Ok(runtime::evaluate(
            &self.module,
            &self.entrypoint,
            canonical_context,
        )?)
    }
}
