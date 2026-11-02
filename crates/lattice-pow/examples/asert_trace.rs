//! Recompute the formal project's vector inputs through the real Rust function.
//! The output is accepted directly by lattice-formal's Lean `asertcheck`.
use lattice_pow::{asert_target, WorkSum};
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: asert_trace <vectors.json>")?;
    let mut rows: Vec<serde_json::Value> = serde_json::from_str(&fs::read_to_string(path)?)?;
    for row in &mut rows {
        let field = |key: &str| -> Result<&str, Box<dyn std::error::Error>> {
            row[key]
                .as_str()
                .ok_or_else(|| format!("missing string {key}").into())
        };
        let target = WorkSum::from_hex(field("anchorTarget")?)
            .and_then(|work| work.as_target())
            .ok_or("invalid anchorTarget")?;
        let actual = asert_target(
            target,
            field("anchorTimestamp")?.parse()?,
            field("anchorHeight")?.parse()?,
            field("blockTimestamp")?.parse()?,
            field("blockHeight")?.parse()?,
            field("targetBlockTime")?.parse()?,
            field("halfLife")?.parse()?,
        );
        row["nextTarget"] = WorkSum::from(actual).to_hex().into();
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
