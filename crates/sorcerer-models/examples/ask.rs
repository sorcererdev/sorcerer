//! Asks Jev one question and runs one research errand through each
//! provider it can build, then prints the ledger rows it added. Run from
//! inside a project:
//!
//! ```text
//! cargo run -p sorcerer-models --example ask [-- --provider NAME …]
//! ```
//!
//! Spends real money: a fraction of a cent for Jev, a few cents a provider.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::decide::jev::Jev;
use sorcerer_models::decide::{Decider, Question, Shape};
use sorcerer_models::providers::provider_named;
use sorcerer_models::research::claude_code::ClaudeCodeResearcher;
use sorcerer_models::research::tool_loop::LoopResearcher;
use sorcerer_models::research::{Research, Researcher, Tool};

fn main() -> anyhow::Result<()> {
    let env = SystemEnv;
    let project = Arc::new(Project::open(&std::env::current_dir()?)?);
    let ledger: Arc<dyn Ledger> = Arc::new(JsonlLedger::for_project(&project)?);
    let before = ledger.read()?.errands.len();
    let cancel = Cancel::default();

    // 1. Jev, one question
    let jev =
        Jev::from_env(&project.config.decide, &env, Some(ledger.clone()))?.for_product("demo");
    let q = Question {
        kind: "demo".into(),
        instructions: "Is this sentence about code?".into(),
        shape: Shape::Noul {
            yes: "it is about how a program behaves".into(),
            no: "it is about something else".into(),
        },
        context: vec![(
            "sentence".into(),
            "The function returns None when the order is missing.".into(),
        )],
        unsure_allowed: false,
    };
    let w = jev.ask(&[q], &cancel)?;
    println!("jev: {:?}\n", w[0].answer);

    // 2. one research errand a provider
    let mut names: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--provider")
        .collect();
    if names.is_empty() {
        if env.which("claude").is_some() {
            names.push("claude".into());
        }
        if env.var("ANTHROPIC_API_KEY").is_some() {
            names.push("anthropic".into());
        }
        names.extend(
            project
                .config
                .model
                .providers
                .iter()
                .filter(|(_, c)| c.kind.as_deref() == Some("openai-compatible"))
                .map(|(n, _)| n.clone()),
        );
    }
    let view = Arc::new(FilesView::new(project.clone(), GrammarCache::user(&env)));
    for name in names {
        let research = Research {
            product: "demo".into(),
            verb: "research".into(),
            prompt: "Name one function defined in this repository and the file it is in. Answer in one line.".into(),
            model: "haiku".into(),
            tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
            cap_usd: 0.05,
            timeout: Duration::from_secs(120),
        };
        let researcher: Box<dyn Researcher> = if name == "claude" {
            let cfg = project
                .config
                .model
                .providers
                .get("claude")
                .cloned()
                .unwrap_or_default();
            Box::new(ClaudeCodeResearcher::new(&project, &cfg)?)
        } else {
            let provider = provider_named(&project, &name, &env)?;
            Box::new(LoopResearcher::new(
                provider,
                view.clone(),
                Fences::for_project(&project)?,
                ledger.clone(),
            ))
        };
        match researcher
            .research(research, &cancel)
            .with_context(|| name.clone())
        {
            Ok(r) => println!(
                "{name}: {}\n  ${:.4}, {:.1} s\n",
                r.answer.trim(),
                r.errand.cost_usd.unwrap_or(0.0),
                r.errand.seconds
            ),
            Err(e) => println!("{name}: failed: {e:#}\n"),
        }
    }

    // 3. what the ledger recorded
    println!("ledger rows added:");
    for e in &ledger.read()?.errands[before..] {
        println!(
            "  {} {} {} {} ${:.5} ok={}",
            e.product,
            e.verb,
            e.provider,
            e.model,
            e.cost_usd.unwrap_or(0.0),
            e.ok
        );
    }
    Ok(())
}
