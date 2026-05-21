use anyhow::{Result, bail};

#[cfg(feature = "schema-gen")]
use crate::config::{AppConfig, GlobalConfig};

pub fn run(_schema_type: &str) -> Result<()> {
    #[cfg(not(feature = "schema-gen"))]
    {
        bail!(
            "Schema generation command is disabled in this build.\n\
             Please rebuild with the schema-gen feature:\n\
             cargo run --features schema-gen -- generate-schema <global|recipe>"
        );
    }

    #[cfg(feature = "schema-gen")]
    {
        match _schema_type {
            "global" => {
                let schema = schemars::schema_for!(GlobalConfig);
                println!("{}", serde_json::to_string_pretty(&schema)?);
            }
            "recipe" | "app" => {
                let schema = schemars::schema_for!(AppConfig);
                println!("{}", serde_json::to_string_pretty(&schema)?);
            }
            other => bail!("Unknown schema type '{}'. Use 'global' or 'recipe'.", other),
        }
        Ok(())
    }
}
