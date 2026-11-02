//! Execution shell for already scanned and stack-instrumented modules.
//! The public policy wrapper authenticates the opcode/profile gates first.
use crate::instrument::InstrumentedModule;
use wasmi::{Config, Engine, Linker, Module, Store, Val};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Engine(#[from] wasmi::Error),
    #[error("policy must export memory as 'memory'")]
    MissingMemory,
    #[error("policy allocator returned a negative or out-of-bounds address")]
    InvalidAllocation,
    #[error("policy context is longer than Int32.max")]
    ContextLength,
    #[error("missing internal stack meter")]
    MissingStackMeter,
    #[error("policy execution worker failed: {0}")]
    Worker(String),
}

struct Instance {
    store: Store<()>,
    memory: wasmi::Memory,
    stack: wasmi::Global,
    alloc: wasmi::TypedFunc<i32, i32>,
    entry: wasmi::TypedFunc<(i32, i32), i32>,
}

fn instantiate(module: &InstrumentedModule, entrypoint: &str) -> Result<Instance, RuntimeError> {
    let mut config = Config::default();
    config
        .compilation_mode(wasmi::CompilationMode::Eager)
        .wasm_multi_memory(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_custom_page_sizes(false)
        .wasm_wide_arithmetic(false)
        .set_max_recursion_depth(65_536)
        .set_max_stack_height(usize::MAX);
    // The injected meter enforces Swift's 65,536 slots. Wasmi's own register
    // allocation is different, so its stack-byte limit must not decide validity.
    let engine = Engine::new(&config);
    let module_bytes = Module::new(&engine, &module.bytes)?;
    let mut store = Store::new(&engine, ());
    // Empty linker intentionally supplies no host imports. Start executes here,
    // with the injected global initialized to the start frame's three slots.
    let instance = Linker::<()>::new(&engine).instantiate_and_start(&mut store, &module_bytes)?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or(RuntimeError::MissingMemory)?;
    let alloc = instance.get_typed_func::<i32, i32>(&store, "lattice_alloc")?;
    let entry = instance.get_typed_func::<(i32, i32), i32>(&store, entrypoint)?;
    let stack = instance
        .get_global(&store, &module.stack_export)
        .ok_or(RuntimeError::MissingStackMeter)?;
    Ok(Instance {
        store,
        memory,
        stack,
        alloc,
        entry,
    })
}

pub(crate) fn validate(module: &InstrumentedModule, entrypoint: &str) -> Result<(), RuntimeError> {
    worker(|| {
        instantiate(module, entrypoint)?;
        Ok(())
    })
}

// Wasmi debug compilation needs more native stack than a small caller thread.
// Keep compilation/execution on a controlled stack, independent of the caller.
// This is not WASM's call-stack budget; injected checks enforce that separately.
fn worker<T: Send>(
    run: impl FnOnce() -> Result<T, RuntimeError> + Send,
) -> Result<T, RuntimeError> {
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("lattice-policy".into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, run)
            .map_err(|error| RuntimeError::Worker(error.to_string()))?;
        handle
            .join()
            .map_err(|_| RuntimeError::Worker("execution panicked".into()))?
    })
}

pub(crate) fn evaluate(
    module: &InstrumentedModule,
    entrypoint: &str,
    context: &[u8],
) -> Result<bool, RuntimeError> {
    worker(|| evaluate_inner(module, entrypoint, context))
}

fn evaluate_inner(
    module: &InstrumentedModule,
    entrypoint: &str,
    context: &[u8],
) -> Result<bool, RuntimeError> {
    let len = i32::try_from(context.len()).map_err(|_| RuntimeError::ContextLength)?;
    let mut instance = instantiate(module, entrypoint)?;
    // Every exported invocation starts a new WasmKit Execution stack.
    instance
        .stack
        .set(&mut instance.store, Val::I64(4))
        .map_err(|_| RuntimeError::MissingStackMeter)?;
    let ptr = instance.alloc.call(&mut instance.store, len)?;
    let address = usize::try_from(ptr).map_err(|_| RuntimeError::InvalidAllocation)?;
    instance
        .memory
        .write(&mut instance.store, address, context)
        .map_err(|_| RuntimeError::InvalidAllocation)?;
    instance
        .stack
        .set(&mut instance.store, Val::I64(5))
        .map_err(|_| RuntimeError::MissingStackMeter)?;
    let result = instance.entry.call(&mut instance.store, (ptr, len))?;
    Ok(result == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        instrument::{FunctionStackProfile, instrument},
        scan_deterministic_module,
    };
    use std::collections::BTreeMap;

    // These tests isolate rewriting and ABI behavior. The deliberately supplied
    // metadata is not a production profiler and cannot mint a public policy.
    fn module(wat: &str, heights: &[u64]) -> InstrumentedModule {
        let bytes = wat::parse_str(wat).unwrap();
        scan_deterministic_module(&bytes).unwrap();
        let profiles = heights
            .iter()
            .map(|&height| FunctionStackProfile {
                max_stack_height: height,
                frame_header_size: 0,
                call_addends: BTreeMap::new(),
            })
            .collect::<Vec<_>>();
        instrument(&bytes, &profiles).unwrap()
    }
    fn simple(allocator: &str, entry: &str) -> String {
        format!(
            "(module (memory (export \"memory\") 1) (func (export \"lattice_alloc\") (param i32) (result i32) {allocator}) (func (export \"policy\") (param i32 i32) (result i32) {entry}))"
        )
    }
    #[test]
    fn abi_and_pointer_bounds() {
        for (result, accept) in [(0, false), (1, true), (2, false), (-1, false)] {
            let module = module(
                &simple("i32.const 0", &format!("i32.const {result}")),
                &[4, 4],
            );
            assert_eq!(evaluate(&module, "policy", b"context").unwrap(), accept);
        }
        for pointer in [-1, 65_536, i32::MAX] {
            let module = module(
                &simple(&format!("i32.const {pointer}"), "i32.const 1"),
                &[4, 4],
            );
            assert!(matches!(
                evaluate(&module, "policy", b"x"),
                Err(RuntimeError::InvalidAllocation)
            ));
        }
        let module = module(&simple("i32.const 65536", "i32.const 1"), &[4, 4]);
        assert!(evaluate(&module, "policy", b"").unwrap());
    }
    #[test]
    fn entry_reads_context_and_each_evaluation_is_fresh() {
        let wat = "(module (memory (export \"memory\") 1) (global $seen (mut i32) (i32.const 0)) (global (export \"__lattice_stack\") i32 (i32.const 9)) (func (export \"lattice_alloc\") (param i32) (result i32) i32.const 0) (func (export \"policy\") (param i32 i32) (result i32) global.get $seen i32.eqz i32.const 1 global.set $seen local.get 0 i32.load8_u i32.const 120 i32.eq i32.and))";
        let module = module(wat, &[4, 5]);
        assert_eq!(module.stack_export, "__lattice_stack_");
        for _ in 0..3 {
            assert!(evaluate(&module, "policy", b"x").unwrap());
        }
        assert!(!evaluate(&module, "policy", b"y").unwrap());
    }
    #[test]
    fn stack_boundary_is_strict_and_start_is_metered() {
        let wat = simple("i32.const 0", "i32.const 1");
        assert!(evaluate(&module(&wat, &[4, 65_530]), "policy", b"").unwrap());
        assert!(evaluate(&module(&wat, &[4, 65_531]), "policy", b"").is_err());
        let start = "(module (func $start) (start $start))";
        let trapped = module(start, &[65_533]);
        assert!(matches!(
            validate(&trapped, "policy"),
            Err(RuntimeError::Engine(_))
        ));
    }
}

#[cfg(test)]
mod profile_integration {
    use super::*;
    use crate::{instrument::instrument, stack_profile::profile_module};

    fn prepare(wat: &str) -> InstrumentedModule {
        let bytes = wat::parse_str(wat).unwrap();
        let profiles = profile_module(&bytes).unwrap();
        instrument(&bytes, &profiles).unwrap()
    }

    #[test]
    fn matches_pinned_swift_recursion_edges() {
        for (locals, depth) in [(0, 8189u32), (64, 909u32)] {
            let declared = if locals == 0 {
                String::new()
            } else {
                format!("(local {})", "i64 ".repeat(locals))
            };
            let module = prepare(&format!(
                r#"(module
                (memory (export "memory") 1)
                (global $heap (mut i32) (i32.const 1024))
                (func (export "lattice_alloc") (param $len i32) (result i32)
                    global.get $heap global.get $heap local.get $len i32.add global.set $heap)
                (func $down (param $n i32) (result i32) {declared}
                    (if (result i32) (i32.eqz (local.get $n))
                        (then (i32.const 1))
                        (else (call $down (i32.sub (local.get $n) (i32.const 1))))))
                (func (export "policy") (param $ptr i32) (param $len i32) (result i32)
                    (call $down (i32.load (local.get $ptr)))))"#
            ));
            assert!(
                evaluate(&module, "policy", &depth.to_le_bytes()).unwrap(),
                "locals {locals}"
            );
            assert!(
                evaluate(&module, "policy", &(depth + 1).to_le_bytes()).is_err(),
                "locals {locals}"
            );
            let thread = std::thread::Builder::new()
                .stack_size(128 * 1024)
                .spawn(move || {
                    assert!(evaluate(&module, "policy", &depth.to_le_bytes()).unwrap());
                    assert!(evaluate(&module, "policy", &(depth + 1).to_le_bytes()).is_err());
                })
                .unwrap();
            thread.join().unwrap();
        }
    }

    #[test]
    fn indirect_calls_restore_stack_and_preserve_arguments_results() {
        let module = prepare(
            r#"(module
            (memory (export "memory") 1) (type $t (func (param i32) (result i32)))
            (table 1 funcref) (elem (i32.const 0) $inc)
            (func $inc (type $t) local.get 0 i32.const 1 i32.add)
            (func (export "lattice_alloc") (param i32) (result i32) i32.const 0)
            (func (export "policy") (param i32 i32) (result i32)
                i32.const 1 i32.const 0 call_indirect (type $t)
                i32.const 0 call_indirect (type $t) i32.const 3 i32.eq))"#,
        );
        assert!(evaluate(&module, "policy", b"abc").unwrap());
    }
    #[test]
    fn compiles_deep_control_flow_and_rejects_bad_exports() {
        let wat = format!(
            r#"(module (memory (export "memory") 1)
            (func (export "lattice_alloc") (param i32) (result i32) i32.const 0)
            (func (export "policy") (param i32 i32) (result i32) {} nop {} i32.const 1))"#,
            "block ".repeat(20_000),
            "end ".repeat(20_000)
        );
        let module = prepare(&wat);
        assert!(evaluate(&module, "policy", b"").unwrap());
        let wrong = prepare(
            r#"(module (memory (export "memory") 1)
            (func (export "lattice_alloc") (param i32) (result i64) i64.const 0)
            (func (export "policy") (param i32 i32) (result i32) i32.const 1))"#,
        );
        assert!(validate(&wrong, "policy").is_err());
        assert!(validate(&module, "missing").is_err());
    }
}
