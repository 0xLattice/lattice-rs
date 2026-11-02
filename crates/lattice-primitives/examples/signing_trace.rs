//! Sign the Swift envelope corpus through the production Rust implementation.
use lattice_primitives::{crypto, signing_preimage};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("expected fixture path")?;
    let mut rows: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    for row in &mut rows {
        let path: Vec<String> = serde_json::from_value(row["path"].clone())?;
        let message = signing_preimage(
            row["bodyCID"].as_str().ok_or("bodyCID")?,
            &path,
            row["nonce"].as_str().ok_or("nonce")?.parse()?,
        );
        row["signature"] = crypto::sign(&message, row["privateKey"].as_str().ok_or("privateKey")?)
            .ok_or("signing failed")?
            .into();
        row["preimage"] = message.into();
    }
    println!("{}", serde_json::to_string(&rows)?);
    Ok(())
}
