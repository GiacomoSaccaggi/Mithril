//! Debug entry point - metti breakpoint qui per capire il flusso!
//! 
//! In RustRover:
//! 1. Click sulla linea a sinistra del numero → pallino rosso = breakpoint
//! 2. Click destro sul main → "Debug 'debug'"
//! 3. Quando si ferma, guarda pannello "Variables" in basso
//!
//! Run with: cargo run --bin debug

use anyhow::Result;
use mithril::config::MithrilConfig;
use mithril::providers::{self, ChatMessage, ChatProvider};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔧 Mithril Debug - Segui il flusso!\n");

    // ============================================================
    // STEP 1: CARICAMENTO CONFIG
    // ============================================================
    // BREAKPOINT QUI → poi guarda la variabile `config`
    println!("STEP 1: Carico configurazione...");
    
    let config = MithrilConfig::load()?;  // <-- BREAKPOINT QUI
    
    // Dopo il breakpoint, nel pannello Variables vedrai:
    // - config.default_provider = "local" o "gemini" etc
    // - config.default_model = "qwen-1.5b"
    // - config.credentials = HashMap con le chiavi (criptate)
    
    println!("  → Provider di default: {}", config.default_provider);
    println!("  → Modello di default: {}", config.default_model);
    println!("  → Credenziali salvate: {:?}", config.list_credentials());
    println!();

    // ============================================================
    // STEP 2: CREAZIONE PROVIDER
    // ============================================================
    // BREAKPOINT QUI → entra dentro create_provider con F7 (Step Into)
    println!("STEP 2: Creo il provider...");
    
    let provider_name = &config.default_provider;
    let provider: Box<dyn ChatProvider> = providers::create_provider(provider_name, &config)?;  // <-- BREAKPOINT QUI
    
    // create_provider fa:
    // 1. Legge il nome ("local", "gemini", "openai", "anthropic")
    // 2. Se cloud → decripta la API key da config.credentials
    // 3. Crea l'oggetto provider specifico
    
    println!("  → Provider creato: {}", provider.name());
    println!("  → Modello usato: {}", provider.model());
    println!();

    // ============================================================
    // STEP 3: PREPARAZIONE MESSAGGI
    // ============================================================
    // BREAKPOINT QUI → guarda la struttura di messages
    println!("STEP 3: Preparo i messaggi...");
    
    let messages = vec![
        ChatMessage::system("Sei un assistente utile. Rispondi in italiano."),
        ChatMessage::user("Ciao! Come ti chiami?"),
    ];  // <-- BREAKPOINT QUI
    
    // messages è un Vec<ChatMessage> dove ogni ChatMessage ha:
    // - role: "system", "user", o "assistant"
    // - content: il testo del messaggio
    
    println!("  → Messaggi preparati: {} messaggi", messages.len());
    for (i, msg) in messages.iter().enumerate() {
        println!("    [{}] {}: {}...", i, msg.role, &msg.content[..msg.content.len().min(30)]);
    }
    println!();

    // ============================================================
    // STEP 4: CHIAMATA AL PROVIDER
    // ============================================================
    // BREAKPOINT QUI → F7 per entrare dentro provider.chat()
    println!("STEP 4: Chiamo il provider...");
    println!("  (questo può richiedere qualche secondo)\n");
    
    let response = provider.chat(&messages).await?;  // <-- BREAKPOINT QUI
    
    // Dentro provider.chat():
    // - Se LOCAL: formatta con chat template → chiama llama-cpp → genera token
    // - Se GEMINI: costruisce JSON → POST a googleapis.com → parsa risposta
    // - Se OPENAI: costruisce JSON → POST a api.openai.com → parsa risposta
    // - Se ANTHROPIC: costruisce JSON → POST a api.anthropic.com → parsa risposta

    // ============================================================
    // STEP 5: RISPOSTA
    // ============================================================
    // BREAKPOINT QUI → guarda response
    println!("STEP 5: Risposta ricevuta!");
    println!();
    println!("═══════════════════════════════════════");
    println!("{}", response);  // <-- BREAKPOINT QUI
    println!("═══════════════════════════════════════");
    println!();

    // ============================================================
    // STEP 6: SECONDO MESSAGGIO (per vedere la conversazione)
    // ============================================================
    println!("STEP 6: Continuo la conversazione...");
    
    let mut conversation = messages.clone();
    conversation.push(ChatMessage::assistant(&response));  // Aggiungo risposta precedente
    conversation.push(ChatMessage::user("Qual è la capitale dell'Italia?"));
    
    // BREAKPOINT QUI → guarda conversation, ora ha 4 messaggi
    let response2 = provider.chat(&conversation).await?;  // <-- BREAKPOINT QUI
    
    println!();
    println!("═══════════════════════════════════════");
    println!("{}", response2);
    println!("═══════════════════════════════════════");

    println!("\n✅ Debug completato!");
    Ok(())
}
