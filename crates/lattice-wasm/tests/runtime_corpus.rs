use lattice_wasm::WasmPolicy;
#[test]
fn pinned_swift_runtime_cases() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("runtime-cases.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let bytes =
            wat::parse_str(case["wat"].as_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let hex = case["context_hex"].as_str().unwrap();
        let context = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let result = match WasmPolicy::new(
            &bytes,
            case["abi"].as_u64().unwrap() as u16,
            case["entrypoint"].as_str().unwrap(),
        ) {
            Ok(policy) => {
                let evaluations = (0..2)
                    .map(|_| match policy.evaluate(&context) {
                        Ok(true) => "accept",
                        Ok(false) => "reject",
                        Err(_) => "error",
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({"validation":"accept","evaluations":evaluations})
            }
            Err(_) => serde_json::json!({"validation":"error"}),
        };
        if let Ok(dir) = std::env::var("LATTICE_RUNTIME_FIXTURES") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{name}.wasm"), bytes).unwrap();
            std::fs::write(format!("{dir}/{name}.json"), result.to_string()).unwrap();
        } else {
            assert_eq!(result, case["swift_result"], "runtime mismatch: {name}");
        }
    }
}
