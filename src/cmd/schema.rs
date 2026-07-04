use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::schema::{self, Tier};
use clap::Args;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom schema --format json
  patchloom schema --tier medium --format prompt
  patchloom schema --tier weak --format json --examples")]
pub struct SchemaArgs {
    /// Output format: json (machine-readable) or prompt (LLM system prompt).
    #[arg(long, default_value = "json")]
    pub format: SchemaFormat,

    /// Capability tier to filter operations by (weak, medium, or strong).
    /// Omit to include all operations. Only these three names are accepted
    /// (not industry size labels like small/large).
    #[arg(long, value_enum)]
    pub tier: Option<Tier>,

    /// Include examples in the output.
    #[arg(long, default_value_t = false)]
    pub examples: bool,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SchemaFormat {
    /// JSON array of operation schemas.
    Json,
    /// Markdown text suitable for LLM system prompts.
    Prompt,
}

pub fn run(args: SchemaArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "schema: format={:?}, tier={:?}, examples={}",
        args.format,
        args.tier,
        args.examples
    );

    let ops = match args.tier {
        Some(t) => schema::operations_for_tier(t)?,
        None => schema::operation_schemas()?,
    };

    match args.format {
        SchemaFormat::Json => {
            let ops_json: Vec<serde_json::Value> = if args.examples {
                ops.iter()
                    .map(serde_json::to_value)
                    .collect::<serde_json::Result<Vec<_>>>()?
            } else {
                // Strip examples from the output.
                ops.iter()
                    .map(|op| {
                        let mut v = serde_json::to_value(op)?;
                        if let Some(obj) = v.as_object_mut() {
                            obj.remove("examples");
                        }
                        Ok(v)
                    })
                    .collect::<serde_json::Result<Vec<_>>>()?
            };
            let envelope = serde_json::json!({
                "ok": true,
                "version": schema::INTENT_FORMAT_VERSION,
                "operations": ops_json,
                "plan_envelope": {
                    "write_policy": schema::plan_write_policy_schema()
                }
            });
            let output = serde_json::to_string_pretty(&envelope)?;
            if global.quiet {
                return Ok(exit::SUCCESS);
            }
            println!("{output}");
        }
        SchemaFormat::Prompt => {
            let prompt = match args.tier {
                Some(t) => schema::system_prompt_for_tier(t)?,
                None => schema::system_prompt_for_tier(Tier::Strong)?,
            };
            if global.quiet {
                return Ok(exit::SUCCESS);
            }
            print!("{prompt}");
        }
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_format_produces_valid_json() {
        let args = SchemaArgs {
            format: SchemaFormat::Json,
            tier: None,
            examples: true,
        };
        let global = GlobalFlags::default();
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn prompt_format_produces_markdown() {
        let args = SchemaArgs {
            format: SchemaFormat::Prompt,
            tier: Some(Tier::Medium),
            examples: false,
        };
        let global = GlobalFlags::default();
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn weak_tier_runs_successfully() {
        let args = SchemaArgs {
            format: SchemaFormat::Json,
            tier: Some(Tier::Weak),
            examples: false,
        };
        let global = GlobalFlags::default();
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }
}
