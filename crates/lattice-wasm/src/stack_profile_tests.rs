use super::*;
fn profile(wat: &str) -> Vec<FunctionStackProfile> {
    profile_module(&wat::parse_str(wat).unwrap()).unwrap()
}
#[test]
fn cached_constants_and_local_sources_skip_peak_updates() {
    let p = profile("(module (func (param i32) (result i32) local.get 0))");
    assert_eq!(p[0].max_stack_height, 4);
    assert_eq!(p[0].frame_header_size, 4);
    let p =
        profile("(module (func (result i32) i32.const 1 i32.const 2 i32.const 3 i32.add i32.add))");
    assert_eq!(p[0].max_stack_height, 5);
    let p = profile("(module (func (result i32) (local i32 i32) local.get 0 local.get 1 i32.add))");
    assert_eq!(p[0].max_stack_height, 6);
}
#[test]
fn call_frame_uses_post_argument_height_and_callee_header() {
    let p = profile(
        "(module (func $f (param i32 i32) (result i32) local.get 0) (func (result i32) (local i32) i32.const 9 i32.const 1 i32.const 2 call $f i32.add))",
    );
    assert_eq!(
        p[1].call_addends.values().copied().collect::<Vec<_>>(),
        vec![11]
    );
    assert_eq!(p[1].max_stack_height, 6);
}
#[test]
fn blocks_preserve_sources_and_end_results_use_normal_push() {
    let p = profile("(module (func (param i32) (result i32) (block (result i32) local.get 0)))");
    assert_eq!(p[0].max_stack_height, 4);
    let p = profile(
        "(module (type $t (func (param i32 i32) (result i32))) (func (param i32) (result i32) local.get 0 local.get 0 block (type $t) i32.add end))",
    );
    assert_eq!(p[0].max_stack_height, 5);
    let p = profile("(module (func (result i32) unreachable i32.add i32.const 4 i32.add))");
    assert_eq!(p[0].max_stack_height, 4);
}
#[test]
fn differential_case_profiles() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../tests/stack-profile-cases.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let bytes = wat::parse_str(case["wat"].as_str().unwrap()).unwrap();
        let result=match profile_module(&bytes){Ok(p)=>serde_json::json!(p.iter().map(|p|serde_json::json!({"max":p.max_stack_height,"calls":p.call_addends.values().collect::<Vec<_>>()})).collect::<Vec<_>>()),Err(_)=>serde_json::json!("error")};
        if std::env::var_os("LATTICE_STACK_FIXTURES").is_none() {
            assert_eq!(
                result, case["swift_profile"],
                "Swift translator mismatch: {name}"
            );
        }
        if let Ok(dir) = std::env::var("LATTICE_STACK_FIXTURES") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{name}.wasm"), bytes).unwrap();
            std::fs::write(format!("{dir}/{name}.json"), result.to_string()).unwrap();
        }
    }
}
