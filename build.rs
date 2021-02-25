use std::fs::File;
use std::io::{prelude::*, BufReader};

fn main() -> Result<(), Box<dyn std::error::Error>> {

    let mut configureable = tonic_build::configure()
        .build_client(true)
        .build_server(false)
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
        
        /*
    let mut config = prost_build::Config::default();
    config
        .service_generator(Box::new(rocket_grpc_generator::RocketConverterServiceGenerator));

    let mut configureable = tonic_build::configure()
        .compile_with_config(
            config,
            &[".\\proto\\corp.proto"],
            &[".\\proto\\"],
        )?;*/
        
    Ok(())
}