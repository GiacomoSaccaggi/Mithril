use anyhow::Result;
use crate::index::palantir::PalantirIndex;
use crate::operators::scan::ScanOperator;

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    println!("🔭 Building Palantír index for: {cwd}");

    let scan_op = ScanOperator::new(&cwd);
    let existing = PalantirIndex::load_or_null(&cwd);

    let index = PalantirIndex::build_incremental(&cwd, &scan_op, existing);

    let file_count = index.entries.len();
    let term_count = index.idf.len();

    index.save(&cwd);

    println!("✅ Indexed {file_count} files, {term_count} unique terms");
    println!("   Saved to: {cwd}/.celebrimbot/palantir_index.json");

    Ok(())
}
