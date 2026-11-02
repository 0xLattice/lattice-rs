//! Internal stack-meter instrumentation. This transforms execution bytes only;
//! content addressing and policy identity always use the original module bytes.
use std::collections::{BTreeMap, BTreeSet};
use wasm_encoder::{
    reencode::{self, Reencode},
    *,
};

#[derive(Debug, Clone)]
pub(crate) struct FunctionStackProfile {
    pub max_stack_height: u64,
    // Retained for differential verification of the source VM frame layout.
    #[allow(dead_code)]
    pub frame_header_size: u64,
    pub call_addends: BTreeMap<u64, u64>,
}

pub(crate) struct InstrumentedModule {
    pub bytes: Vec<u8>,
    pub stack_export: String,
}

/// Original module must have passed the deterministic scanner before this call.
/// The injected global cannot be named by any original, validated instruction.
pub(crate) fn instrument(
    bytes: &[u8],
    profiles: &[FunctionStackProfile],
) -> Result<InstrumentedModule, String> {
    let mut global_index = 0;
    let mut names = BTreeSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.map_err(|e| e.to_string())? {
            wasmparser::Payload::ImportSection(imports) if imports.count() != 0 => {
                return Err("policy imports are not supplied".into());
            }
            wasmparser::Payload::GlobalSection(globals) => global_index = globals.count(),
            wasmparser::Payload::ExportSection(exports) => {
                for export in exports {
                    names.insert(export.map_err(|e| e.to_string())?.name.to_owned());
                }
            }
            _ => {}
        }
    }
    let mut name = "__lattice_stack".to_string();
    while names.contains(&name) {
        name.push('_');
    }
    let mut rewrite = StackReencoder {
        profiles,
        current: 0,
        global_index,
        name: &name,
        added_global: false,
        added_export: false,
    };
    let mut module = Module::new();
    rewrite
        .parse_core_module(&mut module, wasmparser::Parser::new(0), bytes)
        .map_err(|e| e.to_string())?;
    if rewrite.current != profiles.len() {
        return Err("function profile count mismatch".into());
    }
    Ok(InstrumentedModule {
        bytes: module.finish(),
        stack_export: name,
    })
}

struct StackReencoder<'a> {
    profiles: &'a [FunctionStackProfile],
    current: usize,
    global_index: u32,
    name: &'a str,
    added_global: bool,
    added_export: bool,
}
impl StackReencoder<'_> {
    fn add_global(&mut self, globals: &mut GlobalSection) {
        globals.global(
            GlobalType {
                val_type: ValType::I64,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i64_const(3),
        ); // start function: max(0,0)+three saved registers
        self.added_global = true;
    }
    fn add_export(&mut self, exports: &mut ExportSection) {
        exports.export(self.name, ExportKind::Global, self.global_index);
        self.added_export = true;
    }
    fn adjust(&self, f: &mut Function, addend: u64, entering: bool) {
        f.instruction(&Instruction::GlobalGet(self.global_index));
        f.instruction(&Instruction::I64Const(addend as i64));
        f.instruction(&if entering {
            Instruction::I64Add
        } else {
            Instruction::I64Sub
        });
        f.instruction(&Instruction::GlobalSet(self.global_index));
    }
}
impl Reencode for StackReencoder<'_> {
    type Error = &'static str;
    fn parse_global_section(
        &mut self,
        globals: &mut GlobalSection,
        section: wasmparser::GlobalSectionReader<'_>,
    ) -> Result<(), reencode::Error<Self::Error>> {
        reencode::utils::parse_global_section(self, globals, section)?;
        self.add_global(globals);
        Ok(())
    }
    fn parse_export_section(
        &mut self,
        exports: &mut ExportSection,
        section: wasmparser::ExportSectionReader<'_>,
    ) -> Result<(), reencode::Error<Self::Error>> {
        reencode::utils::parse_export_section(self, exports, section)?;
        self.add_export(exports);
        Ok(())
    }
    fn intersperse_section_hook(
        &mut self,
        module: &mut Module,
        _after: Option<SectionId>,
        before: Option<SectionId>,
    ) -> Result<(), reencode::Error<Self::Error>> {
        if !self.added_global && before.is_none_or(|s| u8::from(s) > u8::from(SectionId::Global)) {
            let mut globals = GlobalSection::new();
            self.add_global(&mut globals);
            module.section(&globals);
        }
        if !self.added_export && before.is_none_or(|s| u8::from(s) > u8::from(SectionId::Export)) {
            let mut exports = ExportSection::new();
            self.add_export(&mut exports);
            module.section(&exports);
        }
        Ok(())
    }
    fn parse_function_body(
        &mut self,
        code: &mut CodeSection,
        body: wasmparser::FunctionBody<'_>,
    ) -> Result<(), reencode::Error<Self::Error>> {
        let profile = self
            .profiles
            .get(self.current)
            .ok_or(reencode::Error::UserError("missing function stack profile"))?;
        self.current += 1;
        let mut f = self.new_function_with_parsed_locals(&body)?;
        // WasmKit uses a strict pointer comparison: newSP + maxHeight < end.
        f.instruction(&Instruction::GlobalGet(self.global_index));
        f.instruction(&Instruction::I64Const(profile.max_stack_height as i64));
        f.instruction(&Instruction::I64Add);
        f.instruction(&Instruction::I64Const(65_536));
        f.instruction(&Instruction::I64GeU);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::Unreachable);
        f.instruction(&Instruction::End);
        let mut reader = body.get_operators_reader()?;
        while !reader.eof() {
            let offset = reader.original_position();
            let instruction = self.parse_instruction(&mut reader)?;
            if let Some(addend) = profile.call_addends.get(&offset) {
                if !matches!(
                    instruction,
                    Instruction::Call(_) | Instruction::CallIndirect { .. }
                ) {
                    return Err(reencode::Error::UserError(
                        "stack profile offset is not a call",
                    ));
                }
                self.adjust(&mut f, *addend, true);
                f.instruction(&instruction);
                self.adjust(&mut f, *addend, false);
            } else {
                f.instruction(&instruction);
            }
        }
        code.function(&f);
        Ok(())
    }
}
