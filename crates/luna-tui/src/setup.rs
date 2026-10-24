//! Interactive first-run setup: creates ~/.luna/config.toml.
//!
//! Plain stdin/stdout (no ratatui) so it runs before the TUI takes over.

use luna_core::config::{
    Config, CustomProviderConfig, OllamaConfig, PROVIDER_META, ProviderConfig,
};
use std::io::{BufRead, Write};

fn read_line() -> String {
    let mut s = String::new();
    std::io::stdin().lock().read_line(&mut s).expect("stdin");
    s.trim().to_string()
}

fn ask(prompt: &str, default: Option<&str>) -> String {
    loop {
        match default {
            Some(d) => {
                print!("{prompt} [{d}]: ");
            }
            None => {
                print!("{prompt}: ");
            }
        }
        std::io::stdout().flush().ok();
        let line = read_line();
        if !line.is_empty() {
            return line;
        }
        if let Some(d) = default {
            return d.to_string();
        }
        println!("  please enter a value");
    }
}

fn ask_bool(prompt: &str, default: bool) -> bool {
    let hint = if default { "Y/n" } else { "y/N" };
    loop {
        print!("{prompt} [{hint}]: ");
        std::io::stdout().flush().ok();
        match read_line().to_lowercase().as_str() {
            "" => return default,
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => println!("  please answer y or n"),
        }
    }
}

/// Run the first-run wizard. Writes `~/.luna/config.toml`.
pub fn run_wizard() -> anyhow_noop::Result<()> {
    println!("Welcome to Luna!");
    println!("This wizard creates ~/.luna/config.toml");
    println!();

    let mut cfg = Config::default();

    // 1. Provider choice.
    println!("Choose a provider:");
    println!("  1) Anthropic (Claude)");
    for (i, m) in PROVIDER_META.iter().enumerate() {
        println!("  {}) {}", i + 2, m.label);
    }
    println!("  {}) Ollama (local, no API key)", PROVIDER_META.len() + 2);
    println!(
        "  {}) Custom OpenAI-compatible endpoint",
        PROVIDER_META.len() + 3
    );

    let choice = loop {
        let raw = ask("Provider number", Some("1"));
        match raw.parse::<usize>() {
            Ok(n) if n >= 1 && n <= PROVIDER_META.len() + 3 => break n,
            _ => println!("  enter a number between 1 and {}", PROVIDER_META.len() + 3),
        }
    };

    let default_provider: String = if choice == 1 {
        // Anthropic.
        let api_key = ask("Anthropic API key (ANTHROPIC_API_KEY)", None);
        let model = ask("Model", Some("claude-sonnet-4-5"));
        cfg.providers.anthropic = ProviderConfig {
            enabled: true,
            model,
            api_key,
            ..Default::default()
        };
        "anthropic".into()
    } else if choice <= PROVIDER_META.len() + 1 {
        // One of the OpenAI-compatible built-ins.
        let meta = &PROVIDER_META[choice - 2];
        let api_key = ask(&format!("{} API key ({})", meta.label, meta.env_var), None);
        let model = ask("Model", Some(meta.default_model));
        let pc = ProviderConfig {
            enabled: true,
            model,
            api_key,
            ..Default::default()
        };
        match meta.key {
            "openai" => cfg.providers.openai = pc,
            "google" => cfg.providers.google = pc,
            "xai" => cfg.providers.xai = pc,
            "deepseek" => cfg.providers.deepseek = pc,
            "mistral" => cfg.providers.mistral = pc,
            "openrouter" => cfg.providers.openrouter = pc,
            "groq" => cfg.providers.groq = pc,
            _ => unreachable!(),
        }
        meta.key.to_string()
    } else if choice == PROVIDER_META.len() + 2 {
        // Ollama.
        let base_url = ask("Ollama URL", Some("http://localhost:11434"));
        let model = ask("Model (see `ollama list`)", Some("llama3.2"));
        cfg.providers.ollama = OllamaConfig {
            enabled: true,
            base_url,
            model,
            ..Default::default()
        };
        "ollama".into()
    } else {
        // Custom OpenAI-compatible endpoint.
        let name = ask("Provider name (used as key, e.g. `my_llama`)", None)
            .to_lowercase()
            .replace(' ', "_");
        let base_url = ask("Base URL (…/v1)", None);
        let model = ask("Model name", None);
        let api_key = ask("API key (empty = none)", Some(""));
        cfg.providers.custom.insert(
            name.clone(),
            CustomProviderConfig {
                base_url,
                api_key,
                api_key_env: None,
                model,
                ..Default::default()
            },
        );
        name
    };

    cfg.default_provider = default_provider;

    // 2. Optional: enable additional providers.
    if ask_bool("Enable any additional providers now?", false) {
        for meta in PROVIDER_META {
            let already = match meta.key {
                "openai" => cfg.providers.openai.enabled,
                "google" => cfg.providers.google.enabled,
                "xai" => cfg.providers.xai.enabled,
                "deepseek" => cfg.providers.deepseek.enabled,
                "mistral" => cfg.providers.mistral.enabled,
                "openrouter" => cfg.providers.openrouter.enabled,
                "groq" => cfg.providers.groq.enabled,
                _ => false,
            };
            if already {
                continue;
            }
            if ask_bool(&format!("Enable {}?", meta.label), false) {
                let api_key = ask(&format!("{} API key", meta.label), None);
                let model = ask("Model", Some(meta.default_model));
                let pc = ProviderConfig {
                    enabled: true,
                    model,
                    api_key,
                    ..Default::default()
                };
                match meta.key {
                    "openai" => cfg.providers.openai = pc,
                    "google" => cfg.providers.google = pc,
                    "xai" => cfg.providers.xai = pc,
                    "deepseek" => cfg.providers.deepseek = pc,
                    "mistral" => cfg.providers.mistral = pc,
                    "openrouter" => cfg.providers.openrouter = pc,
                    "groq" => cfg.providers.groq = pc,
                    _ => unreachable!(),
                }
            }
        }
    }

    // 3. Write config.
    cfg.save()?;
    println!();
    println!(
        "Config written to {}",
        luna_core::config::config_path().display()
    );
    println!("Starting Luna…");
    println!();
    Ok(())
}

// Reuse the main crate's error alias.
mod anyhow_noop {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}
