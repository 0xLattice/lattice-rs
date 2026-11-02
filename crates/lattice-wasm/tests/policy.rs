use lattice_wasm::{PolicyError, WasmPolicy};

#[test]
fn public_policy_requires_abi_exports_and_returns_only_one() {
    let bytes = wat::parse_str(
        r#"(module (memory (export "memory") 1)
        (func (export "lattice_alloc") (param i32) (result i32) i32.const 0)
        (func (export "check") (param i32 i32) (result i32) local.get 0 i32.load8_u))"#,
    )
    .unwrap();
    let policy = WasmPolicy::new(&bytes, 1, "check").unwrap();
    assert!(policy.evaluate(&[1]).unwrap());
    assert!(!policy.evaluate(&[2]).unwrap());
    assert!(matches!(
        WasmPolicy::new(&[], 2, "check"),
        Err(PolicyError::UnsupportedAbi(2))
    ));
    assert!(WasmPolicy::new(&bytes, 1, "absent").is_err());
}

#[test]
fn imports_and_trapping_start_never_become_validated_policies() {
    let imported = wat::parse_str(r#"(module (import "host" "f" (func)))"#).unwrap();
    assert!(WasmPolicy::new(&imported, 1, "check").is_err());
    let trapped = wat::parse_str(r#"(module (func $start unreachable) (start $start))"#).unwrap();
    assert!(WasmPolicy::new(&trapped, 1, "check").is_err());
}
