//! WasmKit Translator.swift register-stack accounting for the admitted integer subset.
//! This models value sources and translator control frames, not WASM operand maxima.
use crate::{ModuleError, instrument::FunctionStackProfile};
use std::collections::{BTreeMap, BTreeSet};
use wasmparser::{BlockType, FuncType, Operator, Parser, Payload, TypeRef};
#[derive(Clone, Copy, Debug)]
enum Source {
    Stack,
    Local(u32),
    Constant,
}
#[derive(Clone, Copy, Debug)]
enum Kind {
    Root,
    Block,
    Loop,
    If,
}
#[derive(Clone, Debug)]
struct Frame {
    base: usize,
    params: usize,
    results: usize,
    reachable: bool,
    kind: Kind,
}
struct Translator<'a> {
    values: Vec<Source>,
    frames: Vec<Frame>,
    max: usize,
    base: usize,
    constant_limit: usize,
    constants: BTreeSet<u64>,
    types: &'a [FuncType],
    functions: &'a [u32],
    calls: BTreeMap<u64, u64>,
    returns: usize,
}
fn invalid() -> ModuleError {
    ModuleError::Forbidden("WasmKit stack profile")
}
impl Translator<'_> {
    fn push(&mut self) {
        self.max = self.max.max(self.values.len());
        self.values.push(Source::Stack)
    }
    fn pop(&mut self) -> Result<Option<Source>, ModuleError> {
        let f = self.frames.last().ok_or_else(invalid)?;
        self.pop_in(f.clone())
    }
    fn pop_in(&mut self, f: Frame) -> Result<Option<Source>, ModuleError> {
        if self.values.len() <= f.base {
            if f.reachable {
                return Err(invalid());
            }
            Ok(None)
        } else {
            Ok(self.values.pop())
        }
    }
    fn pop_n(&mut self, n: usize) -> Result<(), ModuleError> {
        for _ in 0..n {
            self.pop()?;
        }
        Ok(())
    }
    fn preserve(&mut self, depth: usize, constants: bool) {
        let start = self.values.len().saturating_sub(depth);
        for v in &mut self.values[start..] {
            if matches!(v, Source::Local(_)) || (constants && matches!(v, Source::Constant)) {
                *v = Source::Stack
            }
        }
    }
    fn preserve_local(&mut self, index: u32) {
        for v in &mut self.values {
            if matches!(v,Source::Local(i)if *i==index) {
                *v = Source::Stack
            }
        }
    }
    fn pop_push(&mut self, n: usize) -> Result<usize, ModuleError> {
        let mut values = vec![];
        for _ in 0..n {
            values.push(self.pop()?)
        }
        let base = self.values.len();
        for source in values.into_iter().rev() {
            match source {
                Some(Source::Local(i)) => self.values.push(Source::Local(i)),
                Some(Source::Constant) => self.values.push(Source::Constant),
                _ => self.push(),
            }
        }
        Ok(base)
    }
    fn unreachable(&mut self) -> Result<(), ModuleError> {
        let f = self.frames.last_mut().ok_or_else(invalid)?;
        f.reachable = false;
        self.values.truncate(f.base);
        Ok(())
    }
    fn signature(&self, block: BlockType) -> Result<(usize, usize), ModuleError> {
        match block {
            BlockType::Empty => Ok((0, 0)),
            BlockType::Type(_) => Ok((0, 1)),
            BlockType::FuncType(i) => self
                .types
                .get(i as usize)
                .map(|t| (t.params().len(), t.results().len()))
                .ok_or_else(invalid),
        }
    }
    fn target(&self, depth: u32) -> Result<Frame, ModuleError> {
        let i = self
            .frames
            .len()
            .checked_sub(depth as usize + 1)
            .ok_or_else(invalid)?;
        self.frames.get(i).cloned().ok_or_else(invalid)
    }
    fn copies(f: &Frame) -> usize {
        if matches!(f.kind, Kind::Loop) {
            f.params
        } else {
            f.results
        }
    }
    fn call(&mut self, t: &FuncType, offset: u64, emit: bool) -> Result<(), ModuleError> {
        for _ in t.params() {
            if self.pop()?.is_none() {
                return Ok(());
            }
        }
        let addend = self.base + self.values.len() + t.params().len().max(t.results().len()) + 3;
        for _ in t.results() {
            self.push()
        }
        if emit {
            self.calls.insert(offset, addend as u64);
        }
        Ok(())
    }
    fn constant(&mut self, bits: u64) {
        if self.constants.contains(&bits) || self.constants.len() < self.constant_limit {
            self.constants.insert(bits);
            self.values.push(Source::Constant)
        } else {
            self.push()
        }
    }
    fn visit(&mut self, op: Operator<'_>, offset: u64) -> Result<(), ModuleError> {
        use Operator::*;
        match op {
            Unreachable => self.unreachable()?,
            Nop | DataDrop { .. } => {}
            Block { blockty } => {
                let (p, r) = self.signature(blockty)?;
                self.preserve(self.values.len(), false);
                let base = self.pop_push(p)?;
                self.frames.push(Frame {
                    base,
                    params: p,
                    results: r,
                    reachable: true,
                    kind: Kind::Block,
                });
            }
            Loop { blockty } | If { blockty } => {
                let is_if = matches!(op, If { .. });
                if is_if {
                    self.pop()?;
                    self.preserve(self.values.len(), false);
                }
                let (p, r) = self.signature(blockty)?;
                self.preserve(p, true);
                self.pop_n(p)?;
                let base = self.values.len();
                for _ in 0..p {
                    self.push()
                }
                self.frames.push(Frame {
                    base,
                    params: p,
                    results: r,
                    reachable: true,
                    kind: if is_if { Kind::If } else { Kind::Loop },
                });
            }
            Else => {
                let f = self.frames.last().cloned().ok_or_else(invalid)?;
                if !matches!(f.kind, Kind::If) {
                    return Err(invalid());
                }
                self.preserve(self.values.len().saturating_sub(f.base), true);
                for _ in 0..f.results {
                    self.pop_in(f.clone())?;
                }
                if self.values.len() != f.base {
                    return Err(invalid());
                }
                self.frames.last_mut().ok_or_else(invalid)?.reachable = true;
                for _ in 0..f.params {
                    self.push()
                }
            }
            End => {
                let f = self.frames.last().cloned().ok_or_else(invalid)?;
                if matches!(f.kind, Kind::Root) {
                    self.pop_n(self.returns)?;
                    if self.values.len() != f.base {
                        return Err(invalid());
                    }
                } else {
                    self.preserve(self.values.len().saturating_sub(f.base), true);
                    for _ in 0..f.results {
                        self.pop_in(f.clone())?;
                    }
                    if self.values.len() != f.base {
                        return Err(invalid());
                    }
                    for _ in 0..f.results {
                        self.push()
                    }
                    self.frames.pop();
                }
            }
            Br { relative_depth } => {
                let f = self.target(relative_depth)?;
                self.preserve(
                    Self::copies(&f).min(self.values.len().saturating_sub(f.base)),
                    true,
                );
                self.pop_n(Self::copies(&f))?;
                self.unreachable()?
            }
            BrIf { relative_depth } => {
                let f = self.target(relative_depth)?;
                self.pop()?;
                if Self::copies(&f) > 0 {
                    self.preserve(self.values.len().saturating_sub(f.base), true);
                    self.pop_push(Self::copies(&f))?;
                }
            }
            BrTable { targets } => {
                if self.pop()?.is_some() {
                    let f = self.target(targets.default())?;
                    let current = self.frames.last().ok_or_else(invalid)?.base;
                    self.preserve(
                        Self::copies(&f).min(self.values.len().saturating_sub(current)),
                        true,
                    );
                    for target in targets.targets() {
                        let target = self.target(target?)?;
                        self.preserve(
                            Self::copies(&target)
                                .min(self.values.len().saturating_sub(target.base)),
                            true,
                        );
                    }
                    self.preserve(
                        Self::copies(&f).min(self.values.len().saturating_sub(f.base)),
                        true,
                    );
                    self.pop_n(Self::copies(&f))?;
                    self.unreachable()?
                }
            }
            Return => {
                self.pop_n(self.returns)?;
                self.unreachable()?
            }
            Call { function_index } => {
                let ty = *self
                    .functions
                    .get(function_index as usize)
                    .ok_or_else(invalid)?;
                let t = self.types.get(ty as usize).ok_or_else(invalid)?.clone();
                self.call(&t, offset, true)?
            }
            CallIndirect { type_index, .. } => {
                let emit = self.pop()?.is_some();
                let t = self
                    .types
                    .get(type_index as usize)
                    .ok_or_else(invalid)?
                    .clone();
                self.call(&t, offset, emit)?
            }
            Drop | GlobalSet { .. } => {
                self.pop()?;
            }
            Select | TypedSelect { .. } => {
                self.pop_n(3)?;
                self.push()
            }
            LocalGet { local_index } => self.values.push(Source::Local(local_index)),
            LocalSet { local_index } | LocalTee { local_index } => {
                self.preserve_local(local_index);
                self.pop()?;
                if matches!(op, LocalTee { .. }) {
                    self.values.push(Source::Local(local_index));
                }
            }
            I32Const { value } => self.constant(value as u32 as u64),
            I64Const { value } => self.constant(value as u64),
            GlobalGet { .. } | MemorySize { .. } | RefNull { .. } | RefFunc { .. } => self.push(),
            RefIsNull => {
                self.values.pop().ok_or_else(invalid)?;
                self.push()
            }
            I32Load { .. }
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
            | MemoryGrow { .. }
            | I32Eqz
            | I64Eqz
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
            | I32WrapI64
            | I64ExtendI32S
            | I64ExtendI32U => {
                self.pop()?;
                self.push()
            }
            I32Store { .. }
            | I64Store { .. }
            | I32Store8 { .. }
            | I32Store16 { .. }
            | I64Store8 { .. }
            | I64Store16 { .. }
            | I64Store32 { .. } => {
                self.pop_n(2)?;
            }
            MemoryInit { .. } | MemoryCopy { .. } | MemoryFill { .. } => {
                for _ in 0..3 {
                    if self.pop()?.is_none() {
                        break;
                    }
                }
            }
            I32Eq | I32Ne | I32LtS | I32LtU | I32GtS | I32GtU | I32LeS | I32LeU | I32GeS
            | I32GeU | I64Eq | I64Ne | I64LtS | I64LtU | I64GtS | I64GtU | I64LeS | I64LeU
            | I64GeS | I64GeU | I32Add | I32Sub | I32Mul | I32DivS | I32DivU | I32RemS
            | I32RemU | I32And | I32Or | I32Xor | I32Shl | I32ShrS | I32ShrU | I32Rotl
            | I32Rotr | I64Add | I64Sub | I64Mul | I64DivS | I64DivU | I64RemS | I64RemU
            | I64And | I64Or | I64Xor | I64Shl | I64ShrS | I64ShrU | I64Rotl | I64Rotr => {
                self.pop_n(2)?;
                self.push()
            }
            _ => return Err(ModuleError::Forbidden("WasmKit stack profile instruction")),
        }
        Ok(())
    }
}

pub(crate) fn profile_module(bytes: &[u8]) -> Result<Vec<FunctionStackProfile>, ModuleError> {
    crate::scan_deterministic_module(bytes)?;
    let mut types = vec![];
    let mut functions = vec![];
    let mut defined = vec![];
    let mut profiles = vec![];
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::TypeSection(section) => {
                for ty in section.into_iter_err_on_gc_types() {
                    types.push(ty?)
                }
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    if let TypeRef::Func(i) = import?.ty {
                        functions.push(i)
                    }
                }
            }
            Payload::FunctionSection(section) => {
                for i in section {
                    let i = i?;
                    functions.push(i);
                    defined.push(i)
                }
            }
            Payload::CodeSectionEntry(body) => {
                let t = types
                    .get(*defined.get(profiles.len()).ok_or_else(invalid)? as usize)
                    .ok_or_else(invalid)?;
                let mut locals = 0usize;
                for local in body.get_locals_reader()? {
                    locals = locals.checked_add(local?.0 as usize).ok_or_else(invalid)?;
                }
                let mut ops = body.get_operators_reader()?;
                let code_size = body.range().end - ops.original_position();
                let constant_limit = (code_size / 20).clamp(4, 128) as usize;
                let base = locals.checked_add(constant_limit).ok_or_else(invalid)?;
                if base >= i16::MAX as usize {
                    return Err(invalid());
                }
                let mut tr = Translator {
                    values: vec![],
                    frames: vec![Frame {
                        base: 0,
                        params: t.params().len(),
                        results: t.results().len(),
                        reachable: true,
                        kind: Kind::Root,
                    }],
                    max: 0,
                    base,
                    constant_limit,
                    constants: BTreeSet::new(),
                    types: &types,
                    functions: &functions,
                    calls: BTreeMap::new(),
                    returns: t.results().len(),
                };
                while !ops.eof() {
                    let offset = ops.original_position();
                    tr.visit(ops.read()?, offset)?;
                }
                profiles.push(FunctionStackProfile {
                    max_stack_height: (base + tr.max) as u64,
                    call_addends: tr.calls,
                    frame_header_size: (t.params().len().max(t.results().len()) + 3) as u64,
                });
            }
            _ => {}
        }
    }
    Ok(profiles)
}

#[cfg(test)]
#[path = "stack_profile_tests.rs"]
mod tests;
