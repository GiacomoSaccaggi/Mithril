use anyhow::Result;
use crate::engine::{
    chat_template::{format_chat, get_stop_tokens, ChatMessage, ChatTemplate},
    lazy_model::LazyModelManager,
    model_catalog::MODELS,
};

pub async fn run(prompt: &str) -> Result<()> {
    let model_info = &MODELS[0]; // Default: qwen-1.5b

    let model_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril/models");
    let model_path = model_dir.join(model_info.file_name);

    if !model_path.exists() {
        eprintln!(
            "Model not found at {:?}\nRun: mithril download-model --model {}",
            model_path, model_info.id
        );
        std::process::exit(1);
    }

    let manager = LazyModelManager::new(model_path, 300);

    let messages = vec![ChatMessage::new("user", prompt)];
    let formatted = format_chat(ChatTemplate::ChatML, &messages);
    let stop_tokens = get_stop_tokens(ChatTemplate::ChatML);

    let response = manager.infer(&formatted, &stop_tokens, 0.7, 2048)?;
    println!("{response}");

    Ok(())
}
