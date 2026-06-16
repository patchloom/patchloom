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

    /// Capability tier to filter operations by.
    /// Omit to include all operations.
    #[arg(long)]
    pub tier: Option<String>,

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
    let tier: Option<Tier> = match &args.tier {
        Some(s) => {
            let t: Tier = s.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?;
            Some(t)
        }
        None => None,
    };

    let ops = match tier {
        Some(t) => schema::operations_for_tier(t),
        None => schema::operation_schemas(),
    };

    match args.format {
        SchemaFormat::Json => {
            let ops_json: Vec<serde_json::Value> = if args.examples {
                ops.iter()
                    .map(|op| serde_json::to_value(op).unwrap_or_default())
                    .collect()
            } else {
                // Strip examples from the output.
                ops.iter()
                    .map(|op| {
                        let mut v = serde_json::to_value(op).unwrap_or_default();
                        if let Some(obj) = v.as_object_mut() {
                            obj.remove("examples");
                        }
                        v
                    })
                    .collect()
            };
            let envelope = serde_json::json!({
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
            let prompt = match tier {
                Some(t) => schema::system_prompt_for_tier(t),
                None => schema::system_prompt_for_tier(Tier::Strong),
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
    fn invalid_tier_returns_error() {
        let args = SchemaArgs {
            format: SchemaFormat::Json,
            tier: Some("invalid".into()),
            examples: false,
        };
        let global = GlobalFlags::default();
        assert!(run(args, &global).is_err());
    }

    #[test]
    fn prompt_format_produces_markdown() {
        let args = SchemaArgs {
            format: SchemaFormat::Prompt,
            tier: Some("medium".into()),
            examples: false,
        };
        let global = GlobalFlags::default();
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }
}
