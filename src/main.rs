mod config;
mod llm;
mod tools;
mod agent;

use clap::Parser;
use dialoguer::{theme::ColorfulTheme, Password, Select};
use dotenvy::dotenv;
use std::env;
use std::io::{self, IsTerminal, Read};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use crate::agent::AgentCore;
use crate::config::AbaConfig;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "")]
    workspace: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let _args = Args::parse();
    
    let mut config = AbaConfig::load();

    if config.openai_api_key.is_none() && config.anthropic_api_key.is_none() && config.use_openai_oauth.is_none() {
        println!("🚀 Welcome to ABA! Let's set up your environment.");
        
        let options = &[
            "1. Login with OpenAI Codex Subscription (OAuth Device Flow)",
            "2. Provide an OpenAI API Key",
            "3. Provide an Anthropic API Key"
        ];
        
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("How would you like to authenticate the agent?")
            .default(0)
            .items(&options[..])
            .interact()
            .unwrap();
            
        match selection {
            0 => {
                config.use_openai_oauth = Some(true);
                config.default_model = Some("gpt-5.4".to_string());
                println!("Great! The agent will negotiate an OAuth flow when it first connects.");
            }
            1 => {
                let key = Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter your OpenAI API Key")
                    .interact()
                    .unwrap();
                config.openai_api_key = Some(key);
                config.default_model = Some("gpt-4o".to_string());
            }
            2 => {
                let key = Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter your Anthropic API Key")
                    .interact()
                    .unwrap();
                config.anthropic_api_key = Some(key);
                config.default_model = Some("claude-3-5-sonnet-20241022".to_string());
            }
            _ => unreachable!(),
        }
        
        config.save().expect("Failed to save config");
        println!("✅ Configuration saved to ~/.config/ABA/config.toml\n");
    }

    let client: Box<dyn llm::LlmClient>;
    
    if env::var("ANTHROPIC_API_KEY").is_ok() || config.anthropic_api_key.is_some() {
        let key = env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| config.anthropic_api_key.clone().unwrap());
        let model = config.default_model.clone().unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string());
        client = Box::new(llm::AnthropicClient::new(key, model));
    } else if env::var("OPENAI_API_KEY").is_ok() || config.openai_api_key.is_some() {
        let key = env::var("OPENAI_API_KEY").unwrap_or_else(|_| config.openai_api_key.clone().unwrap());
        let model = config.default_model.clone().unwrap_or_else(|| "gpt-4o".to_string());
        client = Box::new(llm::OpenAiOAuthClient::new(key, model, false)); 
    } else if config.use_openai_oauth.unwrap_or(false) {
        let client_id = env::var("OAUTH_CLIENT_ID")
            .unwrap_or_else(|_| "YOUR_OAUTH_CLIENT_ID".to_string());
        let model = config.default_model.clone().unwrap_or_else(|| "gpt-5.4".to_string());
        client = Box::new(llm::OpenAiOAuthClient::new(client_id, model, true));
    } else {
        println!("No valid authentication method found. Please reset config.");
        return Ok(());
    }

    let mut prompt = String::new();
    if std::io::stdin().is_terminal() {
        info!("No stdin detected. Please pipe PROMPT.md to aba.");
        return Ok(());
    }
    let _ = io::stdin().read_to_string(&mut prompt);

    let mut agent = AgentCore::new(client);
    info!("Starting ABA Core Agent...");
    agent.run_cycle(prompt).await?;
    info!("ABA Execution finished.");

    Ok(())
}
