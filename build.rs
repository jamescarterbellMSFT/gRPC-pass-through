use std::fs::File;
use std::io::{prelude::*, BufReader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut configureable = tonic_build::configure()
        .build_client(true)
        .format(false)
        .type_attribute(
            ".",
            "#[derive(serde::Serialize, serde::Deserialize)]"
        )
        .type_attribute(
            ".",
            r#"#[serde(rename_all = "camelCase")]"#
        )
        .compile(
            &[".\\proto\\corp.proto"],
            &[".\\proto"],
        )?;
    Ok(())
}