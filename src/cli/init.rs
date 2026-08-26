//! `mithril init` — auto-analyze project and generate MITHRIL.md.
//!
//! Scans the project structure (languages, frameworks, patterns)
//! and generates a steering file that gives the LLM project context.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use walkdir::WalkDir;

/// Run the init command.
pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let project_name = cwd.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    let mithril_md = cwd.join("MITHRIL.md");
    if mithril_md.exists() {
        println!("  {} MITHRIL.md already exists. Overwrite? [y/N]", "⚠".yellow());
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("  {}", "Aborted.".dimmed());
            return Ok(());
        }
    }

    println!("  {} Analyzing project...", "🔍".bold());

    let analysis = analyze_project(&cwd);
    let content = generate_mithril_md(&project_name, &analysis);

    fs::write(&mithril_md, &content)?;

    println!("  {} Generated MITHRIL.md ({} lines)", "✅".green(), content.lines().count());
    println!("  {}", "This file will be injected into every LLM conversation as project context.".dimmed());
    println!("  {}", "Edit it to add project-specific rules, conventions, or constraints.".dimmed());
    println!("  {}", "💡 Tip: Commit MITHRIL.md to version control so your team shares the same project context.".dimmed());

    Ok(())
}

// ── Project analysis ─────────────────────────────────────────────────────────

struct ProjectAnalysis {
    languages: Vec<(String, usize)>, // (language, file count) sorted desc
    total_files: usize,
    total_lines: usize,
    has_git: bool,
    build_system: Vec<String>,
    frameworks: Vec<String>,
    entry_points: Vec<String>,
    key_dirs: Vec<String>,
    config_files: Vec<String>,
}

fn analyze_project(root: &Path) -> ProjectAnalysis {
    let mut lang_counts: HashMap<String, usize> = HashMap::new();
    let mut total_files = 0usize;
    let mut total_lines = 0usize;
    let mut build_system = Vec::new();
    let mut frameworks = Vec::new();
    let mut entry_points = Vec::new();
    let mut config_files = Vec::new();
    let mut key_dirs: Vec<String> = Vec::new();

    let has_git = root.join(".git").exists();

    // Detect build system and frameworks from known files
    let detectors: &[(&str, &str, Option<&str>)] = &[
        ("Cargo.toml", "Cargo (Rust)", Some("Rust")),
        ("package.json", "npm/bun (Node.js)", Some("Node.js")),
        ("pyproject.toml", "Python (pyproject)", Some("Python")),
        ("requirements.txt", "Python (pip)", None),
        ("go.mod", "Go modules", Some("Go")),
        ("pom.xml", "Maven (Java)", Some("Java")),
        ("build.gradle", "Gradle (Java/Kotlin)", Some("JVM")),
        ("build.gradle.kts", "Gradle Kotlin DSL", Some("Kotlin")),
        ("CMakeLists.txt", "CMake (C/C++)", Some("C/C++")),
        ("Makefile", "Make", None),
        ("Dockerfile", "Docker", None),
        ("docker-compose.yml", "Docker Compose", None),
        ("flake.nix", "Nix", None),
        (".mithril-flow.yaml", "Mithril Flow", None),
    ];

    for (file, build, framework) in detectors {
        if root.join(file).exists() {
            build_system.push(build.to_string());
            if let Some(fw) = framework {
                if !frameworks.contains(&fw.to_string()) {
                    frameworks.push(fw.to_string());
                }
            }
        }
    }

    // Walk source files
    let ignore_dirs = [
        "target", "node_modules", ".git", "dist", "build", "out",
        ".cache", "__pycache__", ".venv", "venv", ".idea", "tmp",
    ];

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !ignore_dirs.contains(&name.as_ref())
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);

        // Track key directories (first level)
        if let Some(first_component) = rel.components().next() {
            let dir_name = first_component.as_os_str().to_string_lossy().to_string();
            if rel.components().count() > 1 && !key_dirs.contains(&dir_name) && dir_name != "." {
                key_dirs.push(dir_name);
            }
        }

        // Count language by extension
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let lang = ext_to_language(ext);
            if !lang.is_empty() {
                *lang_counts.entry(lang.to_string()).or_insert(0) += 1;
                total_files += 1;

                // Count lines (quick, skip big files)
                if let Ok(content) = fs::read_to_string(path) {
                    if content.len() < 500_000 {
                        total_lines += content.lines().count();
                    }
                }
            }
        }

        // Detect entry points
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        match file_name.as_ref() {
            "main.rs" | "main.py" | "main.go" | "main.ts" | "main.js"
            | "index.ts" | "index.js" | "app.py" | "App.tsx" => {
                entry_points.push(rel.to_string_lossy().to_string());
            }
            _ => {}
        }

        // Detect config files
        if (file_name.ends_with(".yaml") || file_name.ends_with(".yml")
            || file_name.ends_with(".toml") || file_name.ends_with(".json"))
            && rel.components().count() <= 2 {
                config_files.push(rel.to_string_lossy().to_string());
            }
    }

    // Sort languages by count
    let mut languages: Vec<(String, usize)> = lang_counts.into_iter().collect();
    languages.sort_by_key(|a| std::cmp::Reverse(a.1));
    languages.truncate(8);

    key_dirs.sort();
    key_dirs.truncate(15);
    config_files.sort();
    config_files.truncate(10);
    entry_points.truncate(5);

    ProjectAnalysis {
        languages,
        total_files,
        total_lines,
        has_git,
        build_system,
        frameworks,
        entry_points,
        key_dirs,
        config_files,
    }
}

fn ext_to_language(ext: &str) -> &'static str {
    match ext {
        "rs" => "Rust",
        "py" => "Python",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "tsx" => "TypeScript (React)",
        "jsx" => "JavaScript (React)",
        "go" => "Go",
        "java" => "Java",
        "kt" => "Kotlin",
        "scala" => "Scala",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "rb" => "Ruby",
        "swift" => "Swift",
        "lua" => "Lua",
        "sh" | "bash" => "Shell",
        "sql" => "SQL",
        "html" => "HTML",
        "css" | "scss" | "sass" => "CSS",
        "md" => "Markdown",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "json" => "JSON",
        _ => "",
    }
}

// ── MITHRIL.md generation ────────────────────────────────────────────────────

fn generate_mithril_md(name: &str, a: &ProjectAnalysis) -> String {
    let mut s = String::new();

    s.push_str(&format!("# {}\n\n", name));

    // Summary
    s.push_str("## Project Overview\n\n");
    if !a.frameworks.is_empty() {
        s.push_str(&format!("**Stack:** {}\n", a.frameworks.join(", ")));
    }
    if !a.build_system.is_empty() {
        s.push_str(&format!("**Build:** {}\n", a.build_system.join(", ")));
    }
    s.push_str(&format!("**Files:** {} source files, ~{} lines\n", a.total_files, format_count(a.total_lines)));
    if a.has_git {
        s.push_str("**VCS:** Git\n");
    }
    s.push('\n');

    // Languages
    if !a.languages.is_empty() {
        s.push_str("## Languages\n\n");
        s.push_str("| Language | Files |\n|----------|-------|\n");
        for (lang, count) in &a.languages {
            s.push_str(&format!("| {} | {} |\n", lang, count));
        }
        s.push('\n');
    }

    // Structure
    if !a.key_dirs.is_empty() {
        s.push_str("## Structure\n\n");
        s.push_str("```\n");
        for dir in &a.key_dirs {
            s.push_str(&format!("{}/\n", dir));
        }
        s.push_str("```\n\n");
    }

    // Entry points
    if !a.entry_points.is_empty() {
        s.push_str("## Entry Points\n\n");
        for ep in &a.entry_points {
            s.push_str(&format!("- `{}`\n", ep));
        }
        s.push('\n');
    }

    // Config files
    if !a.config_files.is_empty() {
        s.push_str("## Configuration Files\n\n");
        for cf in &a.config_files {
            s.push_str(&format!("- `{}`\n", cf));
        }
        s.push('\n');
    }

    // Conventions placeholder
    s.push_str("## Conventions\n\n");
    s.push_str("<!-- Add your project's coding conventions here -->\n");
    s.push_str("<!-- Examples: -->\n");
    s.push_str("<!-- - Use X pattern for error handling -->\n");
    s.push_str("<!-- - Tests go in tests/ not alongside source -->\n");
    s.push_str("<!-- - Commit messages follow conventional commits -->\n\n");

    // Rules placeholder
    s.push_str("## Rules\n\n");
    s.push_str("<!-- Add constraints the AI should follow -->\n");
    s.push_str("<!-- Examples: -->\n");
    s.push_str("<!-- - Do not modify files in vendor/ -->\n");
    s.push_str("<!-- - Always run tests after changes -->\n");
    s.push_str("<!-- - Use the existing error type, don't create new ones -->\n");

    s
}

fn format_count(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ext_to_language() {
        assert_eq!(ext_to_language("rs"), "Rust");
        assert_eq!(ext_to_language("py"), "Python");
        assert_eq!(ext_to_language("xyz"), "");
    }

    #[test]
    fn test_analyze_project() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub mod foo;\n").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"test\"").unwrap();

        let analysis = analyze_project(dir.path());
        assert!(analysis.languages.iter().any(|(l, _)| l == "Rust"));
        assert!(analysis.build_system.iter().any(|b| b.contains("Cargo")));
    }

    #[test]
    fn test_generate_mithril_md() {
        let analysis = ProjectAnalysis {
            languages: vec![("Rust".to_string(), 10), ("TOML".to_string(), 2)],
            total_files: 12,
            total_lines: 1500,
            has_git: true,
            build_system: vec!["Cargo (Rust)".to_string()],
            frameworks: vec!["Rust".to_string()],
            entry_points: vec!["src/main.rs".to_string()],
            key_dirs: vec!["src".to_string(), "docs".to_string()],
            config_files: vec!["Cargo.toml".to_string()],
        };

        let output = generate_mithril_md("my-project", &analysis);
        assert!(output.contains("# my-project"));
        assert!(output.contains("Rust"));
        assert!(output.contains("Cargo"));
        assert!(output.contains("src/main.rs"));
    }

    #[test]
    fn test_ext_to_language_all_extensions() {
        assert_eq!(ext_to_language("js"), "JavaScript");
        assert_eq!(ext_to_language("ts"), "TypeScript");
        assert_eq!(ext_to_language("tsx"), "TypeScript (React)");
        assert_eq!(ext_to_language("jsx"), "JavaScript (React)");
        assert_eq!(ext_to_language("go"), "Go");
        assert_eq!(ext_to_language("java"), "Java");
        assert_eq!(ext_to_language("kt"), "Kotlin");
        assert_eq!(ext_to_language("scala"), "Scala");
        assert_eq!(ext_to_language("c"), "C");
        assert_eq!(ext_to_language("h"), "C");
        assert_eq!(ext_to_language("cpp"), "C++");
        assert_eq!(ext_to_language("cc"), "C++");
        assert_eq!(ext_to_language("cxx"), "C++");
        assert_eq!(ext_to_language("hpp"), "C++");
        assert_eq!(ext_to_language("rb"), "Ruby");
        assert_eq!(ext_to_language("swift"), "Swift");
        assert_eq!(ext_to_language("lua"), "Lua");
        assert_eq!(ext_to_language("sh"), "Shell");
        assert_eq!(ext_to_language("bash"), "Shell");
        assert_eq!(ext_to_language("sql"), "SQL");
        assert_eq!(ext_to_language("html"), "HTML");
        assert_eq!(ext_to_language("css"), "CSS");
        assert_eq!(ext_to_language("scss"), "CSS");
        assert_eq!(ext_to_language("sass"), "CSS");
        assert_eq!(ext_to_language("md"), "Markdown");
        assert_eq!(ext_to_language("yaml"), "YAML");
        assert_eq!(ext_to_language("yml"), "YAML");
        assert_eq!(ext_to_language("toml"), "TOML");
        assert_eq!(ext_to_language("json"), "JSON");
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(500), "500");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1000), "1.0k");
        assert_eq!(format_count(1500), "1.5k");
        assert_eq!(format_count(10_000), "10.0k");
        assert_eq!(format_count(1_000_000), "1.0M");
        assert_eq!(format_count(1_500_000), "1.5M");
    }

    #[test]
    fn test_analyze_empty_project() {
        let dir = tempdir().unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.languages.is_empty());
        assert_eq!(analysis.total_files, 0);
        assert_eq!(analysis.total_lines, 0);
        assert!(!analysis.has_git);
    }

    #[test]
    fn test_analyze_project_with_git() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.has_git);
    }

    #[test]
    fn test_analyze_project_detects_frameworks() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.build_system.iter().any(|b| b.contains("npm")));
        assert!(analysis.frameworks.contains(&"Node.js".to_string()));
    }

    #[test]
    fn test_analyze_project_python() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]").unwrap();
        fs::write(dir.path().join("main.py"), "print('hello')").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.build_system.iter().any(|b| b.contains("Python")));
        assert!(analysis.languages.iter().any(|(l, _)| l == "Python"));
    }

    #[test]
    fn test_analyze_project_go() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.build_system.iter().any(|b| b.contains("Go")));
    }

    #[test]
    fn test_analyze_project_java() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pom.xml"), "<project></project>").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.build_system.iter().any(|b| b.contains("Maven")));
    }

    #[test]
    fn test_analyze_project_docker() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.build_system.iter().any(|b| b.contains("Docker")));
    }

    #[test]
    fn test_analyze_project_entry_points() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("main.py"), "if __name__=='__main__':").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.entry_points.iter().any(|e| e.contains("main.rs")));
        assert!(analysis.entry_points.iter().any(|e| e.contains("main.py")));
    }

    #[test]
    fn test_analyze_project_config_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("config.yaml"), "key: value").unwrap();
        fs::write(dir.path().join("settings.json"), "{}").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.config_files.iter().any(|c| c.contains("config.yaml")));
        assert!(analysis.config_files.iter().any(|c| c.contains("settings.json")));
    }

    #[test]
    fn test_analyze_project_key_dirs() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        fs::create_dir(dir.path().join("tests")).unwrap();
        fs::write(dir.path().join("tests/test.rs"), "").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.key_dirs.contains(&"src".to_string()));
        assert!(analysis.key_dirs.contains(&"tests".to_string()));
    }

    #[test]
    fn test_analyze_project_ignores_target() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(dir.path().join("target/debug/main.rs"), "ignored").unwrap();
        let analysis = analyze_project(dir.path());
        // target directory should be ignored
        assert!(!analysis.key_dirs.contains(&"target".to_string()));
    }

    #[test]
    fn test_analyze_project_ignores_node_modules() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "ignored").unwrap();
        let analysis = analyze_project(dir.path());
        // node_modules should be ignored
        assert!(!analysis.key_dirs.contains(&"node_modules".to_string()));
    }

    #[test]
    fn test_generate_mithril_md_empty_analysis() {
        let analysis = ProjectAnalysis {
            languages: vec![],
            total_files: 0,
            total_lines: 0,
            has_git: false,
            build_system: vec![],
            frameworks: vec![],
            entry_points: vec![],
            key_dirs: vec![],
            config_files: vec![],
        };
        let output = generate_mithril_md("empty-project", &analysis);
        assert!(output.contains("# empty-project"));
        assert!(output.contains("Conventions"));
        assert!(output.contains("Rules"));
    }

    #[test]
    fn test_generate_mithril_md_has_conventions_section() {
        let analysis = ProjectAnalysis {
            languages: vec![],
            total_files: 0,
            total_lines: 0,
            has_git: false,
            build_system: vec![],
            frameworks: vec![],
            entry_points: vec![],
            key_dirs: vec![],
            config_files: vec![],
        };
        let output = generate_mithril_md("test", &analysis);
        assert!(output.contains("## Conventions"));
        assert!(output.contains("<!-- Add your project"));
    }

    #[test]
    fn test_generate_mithril_md_has_rules_section() {
        let analysis = ProjectAnalysis {
            languages: vec![],
            total_files: 0,
            total_lines: 0,
            has_git: false,
            build_system: vec![],
            frameworks: vec![],
            entry_points: vec![],
            key_dirs: vec![],
            config_files: vec![],
        };
        let output = generate_mithril_md("test", &analysis);
        assert!(output.contains("## Rules"));
        assert!(output.contains("<!-- Add constraints"));
    }

    #[test]
    fn test_analyze_multiple_languages() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("script.py"), "print('hi')").unwrap();
        fs::write(dir.path().join("app.js"), "console.log('hi')").unwrap();
        let analysis = analyze_project(dir.path());
        assert!(analysis.languages.len() >= 3);
    }

    #[test]
    fn test_generate_mithril_md_languages_table() {
        let analysis = ProjectAnalysis {
            languages: vec![("Rust".to_string(), 10), ("Python".to_string(), 5)],
            total_files: 15,
            total_lines: 1000,
            has_git: false,
            build_system: vec![],
            frameworks: vec![],
            entry_points: vec![],
            key_dirs: vec![],
            config_files: vec![],
        };
        let output = generate_mithril_md("test", &analysis);
        assert!(output.contains("| Rust | 10 |"));
        assert!(output.contains("| Python | 5 |"));
    }

    #[test]
    fn test_generate_mithril_md_structure_section() {
        let analysis = ProjectAnalysis {
            languages: vec![],
            total_files: 0,
            total_lines: 0,
            has_git: false,
            build_system: vec![],
            frameworks: vec![],
            entry_points: vec![],
            key_dirs: vec!["src".to_string(), "lib".to_string()],
            config_files: vec![],
        };
        let output = generate_mithril_md("test", &analysis);
        assert!(output.contains("## Structure"));
        assert!(output.contains("src/"));
        assert!(output.contains("lib/"));
    }
}
