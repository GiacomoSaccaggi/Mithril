use anyhow::Result;
use crate::operators::shadow::ShadowOperator;

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let op = ShadowOperator::new(&cwd, 10);
    let result = op.undo_last_session();

    if result.session_id == "none" {
        println!("No shadow log sessions found.");
        return Ok(());
    }

    println!("↩️  Undoing session: {}", result.session_id);

    if result.restored.is_empty()
        && result.deleted_new.is_empty()
        && result.recreated.is_empty()
        && result.errors.is_empty()
    {
        println!("Nothing to undo.");
        return Ok(());
    }

    for f in &result.restored {
        println!("  ✅ Restored:      {f}");
    }
    for f in &result.deleted_new {
        println!("  🗑  Removed (new): {f}");
    }
    for f in &result.recreated {
        println!("  ✅ Recreated:     {f}");
    }
    for e in &result.errors {
        eprintln!("  ❌ Error: {e}");
    }

    if result.errors.is_empty() {
        println!("\n✅ Undo complete.");
    } else {
        eprintln!("\n⚠️  Undo completed with {} error(s).", result.errors.len());
    }

    Ok(())
}
