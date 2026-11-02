use lattice_wasm::scan_deterministic_module;

#[test]
fn deterministic_subset() {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("scan-cases.json")).unwrap();
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let wasm = wat::parse_str(row["wat"].as_str().unwrap()).unwrap();
        let verdict = scan_deterministic_module(&wasm);
        assert_eq!(
            verdict.is_ok(),
            row["accept"].as_bool().unwrap(),
            "{name}: {verdict:?}"
        );
        if let Some(dir) = std::env::var_os("LATTICE_WASM_FIXTURES") {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{name}.wasm")),
                wasm,
            )
            .unwrap();
        }
    }
}

#[test]
fn rejects_invalid_uncalled_body_and_truncated_binary() {
    let invalid = wat::parse_str("(module (func (result i32) i64.const 0))").unwrap();
    assert!(scan_deterministic_module(&invalid).is_err());
    let valid = wat::parse_str("(module (func (result i32) i32.const 0))").unwrap();
    for end in (0..8).chain(std::iter::once(valid.len() - 1)) {
        assert!(scan_deterministic_module(&valid[..end]).is_err(), "{end}");
    }
}

#[test]
fn deep_control_nesting_does_not_use_native_recursion() {
    let wat = format!(
        "(module (func {} nop {}))",
        "block ".repeat(20_000),
        "end ".repeat(20_000)
    );
    let bytes = wat::parse_str(wat).unwrap();
    let thread = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || scan_deterministic_module(&bytes))
        .unwrap();
    thread.join().unwrap().unwrap();
}
