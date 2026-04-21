use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;

use crate::sdd::memory::memory_set;

pub struct ProjectContext {
    pub active_project: Value,
    pub project_profile: Value,
    pub repo_map: Value,
    pub validation_commands: Value,
}

pub async fn bootstrap_project_context(pool: &SqlitePool, root: &Path) -> Result<ProjectContext> {
    let context = inspect_project(root);

    memory_set(
        pool,
        "spex-architect",
        "active_project",
        &context.active_project.to_string(),
        None,
        Some("config"),
        None,
        None,
    )
    .await?;

    memory_set(
        pool,
        "spex-architect",
        "project_profile",
        &context.project_profile.to_string(),
        None,
        Some("config"),
        None,
        None,
    )
    .await?;

    memory_set(
        pool,
        "spex-architect",
        "repo_map",
        &context.repo_map.to_string(),
        None,
        Some("architecture"),
        None,
        None,
    )
    .await?;

    memory_set(
        pool,
        "spex-architect",
        "validation_commands",
        &context.validation_commands.to_string(),
        None,
        Some("config"),
        None,
        None,
    )
    .await?;

    Ok(context)
}

pub fn inspect_project_at_subpath(root: &Path, subpath: &str) -> Result<ProjectContext> {
    let target = resolve_project_subpath(root, subpath)?;
    Ok(inspect_project(&target))
}

fn resolve_project_subpath(root: &Path, subpath: &str) -> Result<std::path::PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let target = root.join(subpath);
    let target = target
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("Subpath does not exist: {}", subpath))?;

    if !target.starts_with(&root) {
        anyhow::bail!("Subpath escapes project root: {}", subpath);
    }
    if !target.is_dir() {
        anyhow::bail!("Subpath is not a directory: {}", subpath);
    }

    Ok(target)
}

pub fn inspect_project(root: &Path) -> ProjectContext {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    let cargo = root.join("Cargo.toml");
    let package_json = root.join("package.json");
    let pyproject = root.join("pyproject.toml");
    let requirements = root.join("requirements.txt");
    let poetry_lock = root.join("poetry.lock");
    let uv_lock = root.join("uv.lock");
    let hatch_toml = root.join("hatch.toml");
    let tox_ini = root.join("tox.ini");
    let noxfile = root.join("noxfile.py");
    let go_mod = root.join("go.mod");
    let composer = root.join("composer.json");
    let composer_lock = root.join("composer.lock");
    let artisan = root.join("artisan");
    let phpunit_xml = root.join("phpunit.xml");
    let phpunit_xml_dist = root.join("phpunit.xml.dist");
    let pest_file = root.join("pest.php");
    let phpstan_neon = root.join("phpstan.neon");
    let phpstan_neon_dist = root.join("phpstan.neon.dist");
    let gemfile = root.join("Gemfile");
    let gemfile_lock = root.join("Gemfile.lock");
    let rubocop_yml = root.join(".rubocop.yml");
    let rspec_file = root.join(".rspec");
    let rakefile = root.join("Rakefile");
    let js_package_manager = detect_js_package_manager(&root);
    let workspace_profile = detect_workspace_profile(&root, package_json.exists(), cargo.exists());
    let ci_profile = detect_ci_profile(&root);
    let workspace_packages = infer_workspace_packages(&root);
    let subprojects = inspect_workspace_subprojects(&root);

    let mut languages: Vec<String> = Vec::new();
    let mut frameworks: Vec<String> = Vec::new();
    let mut package_managers: Vec<String> = Vec::new();
    let mut commands = json!({
        "build": Value::Null,
        "test": Value::Null,
        "lint": Value::Null,
        "format": Value::Null
    });

    if cargo.exists() {
        languages.push("Rust".to_string());
        package_managers.push("cargo".to_string());
        commands["build"] = json!("cargo build");
        commands["test"] = json!("cargo test");
        commands["lint"] = json!("cargo clippy -- -D warnings");
        commands["format"] = json!("cargo fmt --check");
    }

    if package_json.exists() {
        languages.push("JavaScript/TypeScript".to_string());
        package_managers.push(js_package_manager.clone());
        let package = read_json_file(&package_json).unwrap_or_else(|| json!({}));
        if let Some(framework) = detect_js_framework(&package) {
            frameworks.push(framework);
        }
        fill_node_commands(&mut commands, &package, &js_package_manager);
    }

    if pyproject.exists() || requirements.exists() {
        languages.push("Python".to_string());
        let raw = std::fs::read_to_string(&pyproject)
            .or_else(|_| std::fs::read_to_string(&requirements))
            .unwrap_or_default()
            .to_lowercase();
        let python_tooling = detect_python_tooling(
            &root,
            pyproject.exists(),
            poetry_lock.exists(),
            uv_lock.exists(),
            hatch_toml.exists(),
            tox_ini.exists(),
            noxfile.exists(),
            &raw,
        );
        package_managers.extend(python_tooling.package_managers.clone());
        frameworks.extend(python_tooling.frameworks.clone());
        if raw.contains("fastapi") {
            frameworks.push("FastAPI".to_string());
        } else if raw.contains("django") {
            frameworks.push("Django".to_string());
        } else if raw.contains("flask") {
            frameworks.push("Flask".to_string());
        }
        maybe_set_default(&mut commands["test"], &python_tooling.test_command);
        maybe_set_default(&mut commands["lint"], &python_tooling.lint_command);
        maybe_set_default(&mut commands["format"], &python_tooling.format_command);
    }

    if go_mod.exists() {
        languages.push("Go".to_string());
        package_managers.push("go".to_string());
        maybe_set_default(&mut commands["build"], "go build ./...");
        maybe_set_default(&mut commands["test"], "go test ./...");
        maybe_set_default(&mut commands["lint"], "go vet ./...");
    }

    if composer.exists() {
        languages.push("PHP".to_string());
        let raw = std::fs::read_to_string(&composer)
            .unwrap_or_default()
            .to_lowercase();
        let php_tooling = detect_php_tooling(
            composer_lock.exists(),
            artisan.exists(),
            phpunit_xml.exists() || phpunit_xml_dist.exists(),
            pest_file.exists(),
            phpstan_neon.exists() || phpstan_neon_dist.exists(),
            &raw,
        );
        package_managers.extend(php_tooling.package_managers.clone());
        frameworks.extend(php_tooling.frameworks.clone());
        maybe_set_default(&mut commands["test"], &php_tooling.test_command);
        maybe_set_default(&mut commands["lint"], &php_tooling.lint_command);
        maybe_set_default(&mut commands["format"], &php_tooling.format_command);
    }

    if gemfile.exists() {
        languages.push("Ruby".to_string());
        let raw = std::fs::read_to_string(&gemfile)
            .unwrap_or_default()
            .to_lowercase();
        let ruby_tooling = detect_ruby_tooling(
            gemfile_lock.exists(),
            rubocop_yml.exists(),
            rspec_file.exists(),
            rakefile.exists(),
            &raw,
        );
        package_managers.extend(ruby_tooling.package_managers.clone());
        frameworks.extend(ruby_tooling.frameworks.clone());
        maybe_set_default(&mut commands["test"], &ruby_tooling.test_command);
        maybe_set_default(&mut commands["lint"], &ruby_tooling.lint_command);
        maybe_set_default(&mut commands["format"], &ruby_tooling.format_command);
    }

    dedupe(&mut languages);
    dedupe(&mut frameworks);
    dedupe(&mut package_managers);

    apply_ci_validation_hints(&mut commands, &ci_profile);

    let important_paths = collect_existing_paths(
        &root,
        &[
            ".github/workflows",
            "agents",
            "app",
            "cmd",
            "crates",
            "docs",
            "migrations",
            "routes",
            "scripts",
            "services",
            "src",
            "test",
            "tests",
        ],
    );
    let entrypoints = infer_entrypoints(&root);

    let active_project = json!({
        "name": name,
        "root": root.display().to_string(),
        "has_git": root.join(".git").exists(),
        "has_prd": root.join("docs").join("PRD.md").exists(),
        "has_ci": ci_profile["has_ci"].clone(),
        "workspace_type": workspace_profile["type"].clone(),
    });

    let project_profile = json!({
        "languages": languages,
        "frameworks": frameworks,
        "package_managers": package_managers,
        "detected_files": collect_detected_files(&root),
        "workspace": workspace_profile,
        "ci": ci_profile,
        "subprojects": subprojects,
    });

    let repo_map = json!({
        "important_paths": important_paths,
        "entrypoints": entrypoints,
        "workspace_packages": workspace_packages,
    });

    let validation_commands = json!({
        "build": commands["build"].clone(),
        "test": commands["test"].clone(),
        "lint": commands["lint"].clone(),
        "format": commands["format"].clone(),
        "primary": derive_primary_validation_command(&commands, &ci_profile),
        "fast": derive_fast_validation_command(&commands, &ci_profile),
        "full": derive_full_validation_command(&commands, &ci_profile),
        "ci_commands": ci_profile["commands"].clone(),
    });

    ProjectContext {
        active_project,
        project_profile,
        repo_map,
        validation_commands,
    }
}

fn fill_node_commands(commands: &mut Value, package: &Value, package_manager: &str) {
    let run_prefix = js_run_prefix(package_manager);
    let default_test = js_default_test_command(package_manager);

    if let Some(scripts) = package.get("scripts") {
        if let Some(value) = pick_script(scripts, &["build"]) {
            commands["build"] = json!(format!("{} {}", run_prefix, value));
        }
        if let Some(value) = pick_script(scripts, &["test", "test:unit", "test:ci"]) {
            commands["test"] = json!(format!("{} {}", run_prefix, value));
        }
        if let Some(value) = pick_script(scripts, &["lint", "check"]) {
            commands["lint"] = json!(format!("{} {}", run_prefix, value));
        }
        if let Some(value) = pick_script(scripts, &["format", "fmt"]) {
            commands["format"] = json!(format!("{} {}", run_prefix, value));
        }
    }

    maybe_set_default(&mut commands["test"], &default_test);
}

fn pick_script(scripts: &Value, names: &[&str]) -> Option<String> {
    for name in names {
        if scripts.get(*name).and_then(|v| v.as_str()).is_some() {
            return Some((*name).to_string());
        }
    }
    None
}

fn detect_js_framework(package: &Value) -> Option<String> {
    let combined = package.to_string().to_lowercase();
    if combined.contains("\"next\"") {
        Some("Next.js".to_string())
    } else if combined.contains("\"react\"") {
        Some("React".to_string())
    } else if combined.contains("\"vue\"") {
        Some("Vue".to_string())
    } else if combined.contains("\"svelte\"") {
        Some("Svelte".to_string())
    } else if combined.contains("\"express\"") {
        Some("Express".to_string())
    } else {
        None
    }
}

fn detect_js_package_manager(root: &Path) -> String {
    if root.join("pnpm-lock.yaml").exists() {
        "pnpm".to_string()
    } else if root.join("yarn.lock").exists() {
        "yarn".to_string()
    } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        "bun".to_string()
    } else {
        "npm".to_string()
    }
}

fn js_run_prefix(package_manager: &str) -> &'static str {
    match package_manager {
        "pnpm" => "pnpm run",
        "yarn" => "yarn",
        "bun" => "bun run",
        _ => "npm run",
    }
}

fn js_default_test_command(package_manager: &str) -> String {
    match package_manager {
        "pnpm" => "pnpm test".to_string(),
        "yarn" => "yarn test".to_string(),
        "bun" => "bun test".to_string(),
        _ => "npm test".to_string(),
    }
}

struct PythonToolingProfile {
    package_managers: Vec<String>,
    frameworks: Vec<String>,
    test_command: String,
    lint_command: String,
    format_command: String,
}

struct PhpToolingProfile {
    package_managers: Vec<String>,
    frameworks: Vec<String>,
    test_command: String,
    lint_command: String,
    format_command: String,
}

struct RubyToolingProfile {
    package_managers: Vec<String>,
    frameworks: Vec<String>,
    test_command: String,
    lint_command: String,
    format_command: String,
}

#[allow(clippy::too_many_arguments)]
fn detect_python_tooling(
    root: &Path,
    has_pyproject: bool,
    has_poetry_lock: bool,
    has_uv_lock: bool,
    has_hatch_toml: bool,
    has_tox_ini: bool,
    has_noxfile: bool,
    raw: &str,
) -> PythonToolingProfile {
    let mut package_managers = Vec::new();
    let mut frameworks = Vec::new();

    let uses_poetry = has_poetry_lock || raw.contains("[tool.poetry]");
    let uses_uv = has_uv_lock || raw.contains("[tool.uv") || root.join("uv.toml").exists();
    let uses_hatch = has_hatch_toml || raw.contains("[tool.hatch");
    let uses_tox = has_tox_ini || raw.contains("[tool.tox") || raw.contains("tox");
    let uses_nox = has_noxfile;
    let uses_ruff = raw.contains("[tool.ruff") || raw.contains("ruff");
    let uses_pytest = raw.contains("pytest") || root.join("pytest.ini").exists();

    if uses_poetry {
        package_managers.push("poetry".to_string());
    }
    if uses_uv {
        package_managers.push("uv".to_string());
    }
    if uses_hatch {
        package_managers.push("hatch".to_string());
    }
    if package_managers.is_empty() {
        package_managers.push("pip".to_string());
    }

    if uses_tox {
        frameworks.push("tox".to_string());
    }
    if uses_nox {
        frameworks.push("nox".to_string());
    }
    if uses_pytest {
        frameworks.push("pytest".to_string());
    }
    if uses_ruff {
        frameworks.push("ruff".to_string());
    }

    let runner = if uses_uv {
        "uv run"
    } else if uses_poetry {
        "poetry run"
    } else if uses_hatch {
        "hatch run"
    } else {
        ""
    };

    let test_command = if uses_nox {
        if uses_uv {
            "uv run nox".to_string()
        } else if uses_poetry {
            "poetry run nox".to_string()
        } else {
            "nox".to_string()
        }
    } else if uses_tox {
        if uses_uv {
            "uv run tox".to_string()
        } else if uses_poetry {
            "poetry run tox".to_string()
        } else {
            "tox".to_string()
        }
    } else if !runner.is_empty() {
        format!("{} pytest", runner)
    } else {
        "pytest".to_string()
    };

    let lint_command = if !runner.is_empty() {
        format!("{} ruff check .", runner)
    } else {
        "ruff check .".to_string()
    };

    let format_command = if !runner.is_empty() {
        format!("{} ruff format --check .", runner)
    } else {
        "ruff format --check .".to_string()
    };

    dedupe(&mut package_managers);
    dedupe(&mut frameworks);

    let _ = has_pyproject;

    PythonToolingProfile {
        package_managers,
        frameworks,
        test_command,
        lint_command,
        format_command,
    }
}

fn detect_php_tooling(
    has_composer_lock: bool,
    has_artisan: bool,
    has_phpunit: bool,
    has_pest: bool,
    has_phpstan: bool,
    raw: &str,
) -> PhpToolingProfile {
    let mut package_managers = vec!["composer".to_string()];
    let mut frameworks = Vec::new();

    let uses_laravel = has_artisan || raw.contains("laravel/framework");
    let uses_pest = has_pest || raw.contains("pestphp/pest");
    let uses_phpunit = has_phpunit || raw.contains("phpunit/phpunit");
    let uses_phpstan = has_phpstan || raw.contains("phpstan/phpstan");
    let uses_pint = raw.contains("laravel/pint") || uses_laravel;

    if uses_laravel {
        frameworks.push("Laravel".to_string());
    }
    if uses_pest {
        frameworks.push("Pest".to_string());
    }
    if uses_phpunit {
        frameworks.push("PHPUnit".to_string());
    }
    if uses_phpstan {
        frameworks.push("PHPStan".to_string());
    }
    if uses_pint {
        frameworks.push("Pint".to_string());
    }
    if has_composer_lock {
        package_managers.push("composer-lock".to_string());
    }

    dedupe(&mut package_managers);
    dedupe(&mut frameworks);

    let test_command = if uses_laravel {
        "php artisan test".to_string()
    } else if uses_pest {
        "./vendor/bin/pest".to_string()
    } else if uses_phpunit {
        "./vendor/bin/phpunit".to_string()
    } else {
        "composer test".to_string()
    };

    let lint_command = if uses_phpstan {
        "./vendor/bin/phpstan analyse".to_string()
    } else if uses_pint {
        "./vendor/bin/pint --test".to_string()
    } else {
        "composer test".to_string()
    };

    let format_command = if uses_pint {
        "./vendor/bin/pint --test".to_string()
    } else {
        "composer test".to_string()
    };

    PhpToolingProfile {
        package_managers,
        frameworks,
        test_command,
        lint_command,
        format_command,
    }
}

fn detect_ruby_tooling(
    has_gemfile_lock: bool,
    has_rubocop: bool,
    has_rspec: bool,
    has_rakefile: bool,
    raw: &str,
) -> RubyToolingProfile {
    let mut package_managers = vec!["bundler".to_string()];
    let mut frameworks = Vec::new();

    let uses_rails = raw.contains("rails");
    let uses_rspec = has_rspec || raw.contains("rspec") || raw.contains("rspec-rails");
    let uses_rubocop = has_rubocop || raw.contains("rubocop");

    if uses_rails {
        frameworks.push("Rails".to_string());
    }
    if uses_rspec {
        frameworks.push("RSpec".to_string());
    }
    if uses_rubocop {
        frameworks.push("RuboCop".to_string());
    }
    if has_rakefile {
        frameworks.push("Rake".to_string());
    }
    if has_gemfile_lock {
        package_managers.push("bundler-lock".to_string());
    }

    dedupe(&mut package_managers);
    dedupe(&mut frameworks);

    let test_command = if uses_rspec {
        "bundle exec rspec".to_string()
    } else if has_rakefile {
        "bundle exec rake test".to_string()
    } else {
        "bundle exec ruby -Itest".to_string()
    };

    let lint_command = if uses_rubocop {
        "bundle exec rubocop --parallel".to_string()
    } else {
        test_command.clone()
    };

    let format_command = if uses_rubocop {
        "bundle exec rubocop --parallel --format simple".to_string()
    } else {
        test_command.clone()
    };

    RubyToolingProfile {
        package_managers,
        frameworks,
        test_command,
        lint_command,
        format_command,
    }
}

fn detect_workspace_profile(root: &Path, has_package_json: bool, has_cargo: bool) -> Value {
    let mut markers = Vec::new();
    let mut workspace_type = "single-package".to_string();

    if root.join("pnpm-workspace.yaml").exists() {
        markers.push("pnpm-workspace".to_string());
        workspace_type = "javascript-monorepo".to_string();
    }
    if root.join("turbo.json").exists() {
        markers.push("turbo".to_string());
        workspace_type = "javascript-monorepo".to_string();
    }
    if root.join("nx.json").exists() {
        markers.push("nx".to_string());
        workspace_type = "javascript-monorepo".to_string();
    }
    if root.join("lerna.json").exists() {
        markers.push("lerna".to_string());
        workspace_type = "javascript-monorepo".to_string();
    }

    if has_package_json {
        if let Some(package) = read_json_file(&root.join("package.json")) {
            if package.get("workspaces").is_some() {
                markers.push("package.json#workspaces".to_string());
                workspace_type = "javascript-monorepo".to_string();
            }
        }
    }

    if has_cargo {
        let raw = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
        if raw.contains("[workspace]") || raw.contains("members = [") {
            markers.push("cargo-workspace".to_string());
            workspace_type = "rust-workspace".to_string();
        }
    }

    dedupe(&mut markers);
    json!({
        "type": workspace_type,
        "is_monorepo": workspace_type != "single-package",
        "markers": markers,
    })
}

fn detect_ci_profile(root: &Path) -> Value {
    let workflows_dir = root.join(".github").join("workflows");
    if !workflows_dir.exists() {
        return json!({
            "has_ci": false,
            "provider": Value::Null,
            "workflows": [],
            "commands": [],
        });
    }

    let mut workflows = Vec::new();
    let mut commands = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&workflows_dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            let ext = path.extension().and_then(|ext| ext.to_str());
            if ext != Some("yml") && ext != Some("yaml") {
                continue;
            }
            workflows.push(relativize(root, &path));

            let raw = std::fs::read_to_string(&path).unwrap_or_default();
            commands.extend(extract_ci_run_commands(&raw));
        }
    }

    dedupe(&mut workflows);
    dedupe(&mut commands);
    json!({
        "has_ci": true,
        "provider": "github-actions",
        "workflows": workflows,
        "commands": commands,
    })
}

fn extract_ci_run_commands(raw: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let lines: Vec<&str> = raw.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let run_candidate = trimmed
            .strip_prefix("run:")
            .or_else(|| trimmed.strip_prefix("- run:"));
        if let Some(rest) = run_candidate {
            let inline = rest.trim();
            let indent = line.len() - trimmed.len();

            if !inline.is_empty() && inline != "|" && inline != ">" {
                commands.push(inline.to_string());
                i += 1;
                continue;
            }

            i += 1;
            while i < lines.len() {
                let nested = lines[i];
                let nested_trimmed = nested.trim();
                let nested_indent = nested.len() - nested.trim_start().len();
                if nested_trimmed.is_empty() {
                    i += 1;
                    continue;
                }
                if nested_indent <= indent {
                    break;
                }
                if !nested_trimmed.starts_with('#') {
                    commands.push(nested_trimmed.to_string());
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }

    commands
}

fn apply_ci_validation_hints(commands: &mut Value, ci_profile: &Value) {
    let Some(ci_commands) = ci_profile.get("commands").and_then(|v| v.as_array()) else {
        return;
    };

    for command in ci_commands.iter().filter_map(|v| v.as_str()) {
        if commands["format"].is_null() && looks_like_format_command(command) {
            commands["format"] = json!(command);
        }
        if commands["lint"].is_null() && looks_like_lint_command(command) {
            commands["lint"] = json!(command);
        }
        if commands["build"].is_null() && looks_like_build_command(command) {
            commands["build"] = json!(command);
        }
        if commands["test"].is_null() && looks_like_test_command(command) {
            commands["test"] = json!(command);
        }
    }
}

fn derive_primary_validation_command(commands: &Value, ci_profile: &Value) -> Value {
    let ci_commands: Vec<&str> = ci_profile
        .get("commands")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();

    if let Some(command) = pick_primary_command(
        commands.get("test").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_test_command,
    ) {
        return json!(command);
    }
    if let Some(command) = pick_primary_command(
        commands.get("lint").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_lint_command,
    ) {
        return json!(command);
    }
    if let Some(command) = pick_primary_command(
        commands.get("build").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_build_command,
    ) {
        return json!(command);
    }
    if let Some(command) = pick_primary_command(
        commands.get("format").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_format_command,
    ) {
        return json!(command);
    }

    ci_commands
        .first()
        .map(|command| json!(command))
        .unwrap_or(Value::Null)
}

fn derive_fast_validation_command(commands: &Value, ci_profile: &Value) -> Value {
    let ci_commands: Vec<&str> = ci_profile
        .get("commands")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();

    if let Some(command) = commands.get("lint").and_then(|v| v.as_str()) {
        return json!(command);
    }
    if let Some(command) = commands.get("test").and_then(|v| v.as_str()) {
        return json!(command);
    }
    if let Some(command) = commands.get("build").and_then(|v| v.as_str()) {
        return json!(command);
    }
    if let Some(command) = commands.get("format").and_then(|v| v.as_str()) {
        return json!(command);
    }

    ci_commands
        .iter()
        .copied()
        .find(|command| looks_like_lint_command(command) || looks_like_test_command(command))
        .or_else(|| ci_commands.first().copied())
        .map(|command| json!(command))
        .unwrap_or(Value::Null)
}

fn derive_full_validation_command(commands: &Value, ci_profile: &Value) -> Value {
    let ci_commands: Vec<&str> = ci_profile
        .get("commands")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();

    let format = prefer_stronger_ci_command(
        commands.get("format").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_format_command,
    );
    let lint = prefer_stronger_ci_command(
        commands.get("lint").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_lint_command,
    );
    let build = prefer_stronger_ci_command(
        commands.get("build").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_build_command,
    );
    let test = prefer_stronger_ci_command(
        commands.get("test").and_then(|v| v.as_str()),
        &ci_commands,
        looks_like_test_command,
    );

    let mut ordered = Vec::new();
    push_unique_command(&mut ordered, format.as_deref());
    push_unique_command(&mut ordered, lint.as_deref());
    push_unique_command(&mut ordered, build.as_deref());
    push_unique_command(&mut ordered, test.as_deref());

    if ordered.is_empty() {
        return Value::Null;
    }

    json!(ordered.join(" && "))
}

fn pick_primary_command<F>(
    explicit: Option<&str>,
    ci_commands: &[&str],
    predicate: F,
) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    let ci_match = ci_commands
        .iter()
        .copied()
        .find(|command| predicate(command));

    match (explicit, ci_match) {
        (_, Some(ci))
            if ci.contains("--all-targets") || ci.contains("./...") || ci.contains("-q") =>
        {
            Some(ci.to_string())
        }
        (Some(explicit), _) => Some(explicit.to_string()),
        (_, Some(ci)) => Some(ci.to_string()),
        _ => None,
    }
}

fn prefer_stronger_ci_command<F>(
    explicit: Option<&str>,
    ci_commands: &[&str],
    predicate: F,
) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    let ci_match = ci_commands
        .iter()
        .copied()
        .find(|command| predicate(command));

    match (explicit, ci_match) {
        (_, Some(ci)) if is_stronger_ci_variant(ci) => Some(ci.to_string()),
        (Some(explicit), _) => Some(explicit.to_string()),
        (_, Some(ci)) => Some(ci.to_string()),
        _ => None,
    }
}

fn is_stronger_ci_variant(command: &str) -> bool {
    command.contains("--all-targets")
        || command.contains("--all-features")
        || command.contains("./...")
        || command.contains("--check")
}

fn push_unique_command(commands: &mut Vec<String>, candidate: Option<&str>) {
    if let Some(candidate) = candidate {
        if !commands.iter().any(|existing| existing == candidate) {
            commands.push(candidate.to_string());
        }
    }
}

fn looks_like_format_command(command: &str) -> bool {
    command.contains("fmt") || command.contains("format")
}

fn looks_like_lint_command(command: &str) -> bool {
    command.contains("clippy")
        || command.contains("lint")
        || command.contains("eslint")
        || command.contains("ruff check")
        || command.contains("go vet")
}

fn looks_like_build_command(command: &str) -> bool {
    command.contains(" build")
        || command.starts_with("build")
        || command.contains("cargo build")
        || command.contains("go build")
}

fn looks_like_test_command(command: &str) -> bool {
    command.contains(" test")
        || command.starts_with("test")
        || command.contains("pytest")
        || command.contains("rspec")
}

fn infer_workspace_packages(root: &Path) -> Vec<String> {
    collect_existing_paths(root, &["apps", "packages", "crates", "services"])
}

fn inspect_workspace_subprojects(root: &Path) -> Vec<Value> {
    let mut subprojects = Vec::new();

    for base in ["apps", "packages", "crates", "services"] {
        let base_dir = root.join(base);
        if !base_dir.exists() {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&base_dir) else {
            continue;
        };

        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if !path.is_dir() || !looks_like_project_dir(&path) {
                continue;
            }

            if let Some(summary) = summarize_subproject(root, &path) {
                subprojects.push(summary);
            }
        }
    }

    subprojects.sort_by(|a, b| {
        let a_path = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        let b_path = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        a_path.cmp(b_path)
    });
    subprojects
}

fn looks_like_project_dir(path: &Path) -> bool {
    path.join("package.json").exists()
        || path.join("Cargo.toml").exists()
        || path.join("pyproject.toml").exists()
        || path.join("requirements.txt").exists()
        || path.join("go.mod").exists()
        || path.join("composer.json").exists()
        || path.join("Gemfile").exists()
}

fn summarize_subproject(root: &Path, path: &Path) -> Option<Value> {
    let summary = inspect_project(path);
    let name = path.file_name()?.to_str()?.to_string();

    Some(json!({
        "name": name,
        "path": relativize(root, path),
        "workspace_group": path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or_default(),
        "languages": summary.project_profile.get("languages").cloned().unwrap_or_else(|| json!([])),
        "frameworks": summary.project_profile.get("frameworks").cloned().unwrap_or_else(|| json!([])),
        "package_managers": summary.project_profile.get("package_managers").cloned().unwrap_or_else(|| json!([])),
        "entrypoints": summary.repo_map.get("entrypoints").cloned().unwrap_or_else(|| json!([])),
        "validation_commands": summary.validation_commands,
    }))
}

fn maybe_set_default(slot: &mut Value, command: &str) {
    if slot.is_null() {
        *slot = json!(command);
    }
}

fn collect_existing_paths(root: &Path, candidates: &[&str]) -> Vec<String> {
    candidates
        .iter()
        .map(|candidate| root.join(candidate))
        .filter(|path| path.exists())
        .map(|path| relativize(root, &path))
        .collect()
}

fn infer_entrypoints(root: &Path) -> Vec<String> {
    let candidates = [
        "src/main.rs",
        "src/lib.rs",
        "src/index.ts",
        "src/index.js",
        "src/main.ts",
        "src/main.js",
        "app/main.py",
        "main.py",
        "main.go",
    ];

    collect_existing_paths(root, &candidates)
}

fn collect_detected_files(root: &Path) -> Vec<String> {
    let candidates = [
        "Cargo.toml",
        "package.json",
        "pnpm-lock.yaml",
        "pnpm-workspace.yaml",
        "turbo.json",
        "nx.json",
        "lerna.json",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "pyproject.toml",
        "poetry.lock",
        "uv.lock",
        "hatch.toml",
        "tox.ini",
        "noxfile.py",
        "pytest.ini",
        "requirements.txt",
        "go.mod",
        "composer.json",
        "composer.lock",
        "artisan",
        "phpunit.xml",
        "phpunit.xml.dist",
        "pest.php",
        "phpstan.neon",
        "phpstan.neon.dist",
        "Gemfile",
        "Gemfile.lock",
        ".rubocop.yml",
        ".rspec",
        "Rakefile",
        "Makefile",
        "justfile",
    ];
    collect_existing_paths(root, &candidates)
}

fn relativize(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn read_json_file(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn dedupe(items: &mut Vec<String>) {
    items.sort();
    items.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn inspect_project_detects_rust_commands_and_layout() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();

        let context = inspect_project(dir.path());

        assert_eq!(context.project_profile["languages"][0], "Rust");
        assert_eq!(context.validation_commands["test"], "cargo test");
        assert_eq!(context.validation_commands["primary"], "cargo test");
        assert_eq!(
            context.validation_commands["fast"],
            "cargo clippy -- -D warnings"
        );
        assert_eq!(
            context.validation_commands["full"],
            "cargo fmt --check && cargo clippy -- -D warnings && cargo build && cargo test"
        );
        assert!(context.repo_map["important_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "src"));
        assert!(context.repo_map["entrypoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "src/main.rs"));
    }

    #[test]
    fn inspect_project_detects_node_scripts_and_framework() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
                "dependencies": {"react": "18.0.0"},
                "scripts": {
                    "build": "vite build",
                    "test": "vitest",
                    "lint": "eslint ."
                }
            }"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.ts"), "console.log('hi')\n").unwrap();

        let context = inspect_project(dir.path());

        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "React"));
        assert!(context.project_profile["package_managers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "pnpm"));
        assert_eq!(context.validation_commands["build"], "pnpm run build");
        assert_eq!(context.validation_commands["lint"], "pnpm run lint");
        assert_eq!(context.validation_commands["primary"], "pnpm run test");
        assert_eq!(context.validation_commands["fast"], "pnpm run lint");
        assert_eq!(
            context.validation_commands["full"],
            "pnpm run lint && pnpm run build && pnpm run test"
        );
    }

    #[test]
    fn inspect_project_detects_workspace_and_ci_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
                "private": true,
                "workspaces": ["apps/*", "packages/*"]
            }"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("turbo.json"), "{}\n").unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "# lock\n").unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/ui")).unwrap();
        std::fs::write(
            dir.path().join("apps/web/package.json"),
            r#"{"scripts": {"test": "vitest"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/ui/package.json"),
            r#"{"scripts": {"build": "tsup"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
        std::fs::write(
            dir.path().join(".github/workflows/ci.yml"),
            "name: CI\nsteps:\n  - run: yarn lint\n  - run: |\n      yarn test\n      yarn build\n",
        )
        .unwrap();

        let context = inspect_project(dir.path());

        assert_eq!(context.active_project["has_ci"], true);
        assert_eq!(
            context.active_project["workspace_type"],
            "javascript-monorepo"
        );
        assert_eq!(context.project_profile["workspace"]["is_monorepo"], true);
        assert!(context.project_profile["workspace"]["markers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "turbo"));
        assert!(context.project_profile["ci"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "yarn lint"));
        assert_eq!(context.validation_commands["test"], "yarn test");
        assert_eq!(context.validation_commands["build"], "yarn build");
        assert_eq!(context.validation_commands["primary"], "yarn test");
        assert_eq!(context.validation_commands["fast"], "yarn lint");
        assert_eq!(
            context.validation_commands["full"],
            "yarn lint && yarn build && yarn test"
        );
        assert!(context.repo_map["workspace_packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "apps"));
        assert!(context.project_profile["subprojects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == "apps/web"));
        assert!(context.project_profile["subprojects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == "packages/ui"));
    }

    #[test]
    fn inspect_project_summarizes_subprojects_with_validation_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
                "private": true,
                "workspaces": ["apps/*", "packages/*"]
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
        std::fs::write(
            dir.path().join("apps/web/package.json"),
            r#"{
                "dependencies": {"next": "14.0.0"},
                "scripts": {"build": "next build", "lint": "next lint", "test": "vitest"}
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("apps/web/src/main.ts"),
            "console.log('web')\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/Cargo.toml"),
            "[package]\nname='core'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/src/lib.rs"),
            "pub fn x() {}\n",
        )
        .unwrap();

        let context = inspect_project(dir.path());
        let subprojects = context.project_profile["subprojects"].as_array().unwrap();

        let web = subprojects
            .iter()
            .find(|v| v["path"] == "apps/web")
            .expect("apps/web subproject");
        assert!(web["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Next.js"));
        assert_eq!(web["validation_commands"]["primary"], "npm run test");

        let core = subprojects
            .iter()
            .find(|v| v["path"] == "packages/core")
            .expect("packages/core subproject");
        assert!(core["languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Rust"));
        assert_eq!(
            core["validation_commands"]["fast"],
            "cargo clippy -- -D warnings"
        );
    }

    #[test]
    fn inspect_project_detects_rust_workspace() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("crates/core")).unwrap();

        let context = inspect_project(dir.path());

        assert_eq!(context.active_project["workspace_type"], "rust-workspace");
        assert_eq!(context.project_profile["workspace"]["is_monorepo"], true);
        assert!(context.project_profile["workspace"]["markers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "cargo-workspace"));
        assert_eq!(context.validation_commands["primary"], "cargo test");
        assert_eq!(
            context.validation_commands["fast"],
            "cargo clippy -- -D warnings"
        );
    }

    #[test]
    fn inspect_project_detects_poetry_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[tool.poetry]
name = "demo"
[tool.poetry.dependencies]
python = "^3.12"
fastapi = "*"
[tool.poetry.group.dev.dependencies]
pytest = "*"
ruff = "*"
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("poetry.lock"), "# lock\n").unwrap();

        let context = inspect_project(dir.path());

        assert!(context.project_profile["package_managers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "poetry"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "FastAPI"));
        assert_eq!(context.validation_commands["test"], "poetry run pytest");
        assert_eq!(
            context.validation_commands["lint"],
            "poetry run ruff check ."
        );
        assert_eq!(context.validation_commands["primary"], "poetry run pytest");
        assert_eq!(
            context.validation_commands["fast"],
            "poetry run ruff check ."
        );
        assert_eq!(
            context.validation_commands["full"],
            "poetry run ruff format --check . && poetry run ruff check . && poetry run pytest"
        );
    }

    #[test]
    fn inspect_project_detects_uv_and_nox_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "demo"
dependencies = []
[tool.uv]
managed = true
[tool.ruff]
line-length = 100
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("uv.lock"), "version = 1\n").unwrap();
        std::fs::write(dir.path().join("noxfile.py"), "import nox\n").unwrap();

        let context = inspect_project(dir.path());

        assert!(context.project_profile["package_managers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "uv"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "nox"));
        assert_eq!(context.validation_commands["test"], "uv run nox");
        assert_eq!(
            context.validation_commands["format"],
            "uv run ruff format --check ."
        );
        assert_eq!(context.validation_commands["primary"], "uv run nox");
        assert_eq!(context.validation_commands["fast"], "uv run ruff check .");
    }

    #[test]
    fn inspect_project_detects_hatch_and_tox_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
[tool.hatch.envs.default]
dependencies = ["pytest", "ruff"]
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("hatch.toml"), "[envs.default]\n").unwrap();
        std::fs::write(dir.path().join("tox.ini"), "[tox]\nenvlist = py\n").unwrap();

        let context = inspect_project(dir.path());

        assert!(context.project_profile["package_managers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "hatch"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "tox"));
        assert_eq!(context.validation_commands["test"], "tox");
        assert_eq!(
            context.validation_commands["lint"],
            "hatch run ruff check ."
        );
        assert_eq!(context.validation_commands["primary"], "tox");
        assert_eq!(
            context.validation_commands["fast"],
            "hatch run ruff check ."
        );
    }

    #[test]
    fn inspect_project_detects_laravel_php_tooling() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{
                "require": {"laravel/framework": "^11.0"},
                "require-dev": {
                    "pestphp/pest": "^3.0",
                    "phpstan/phpstan": "^1.0",
                    "laravel/pint": "^1.0"
                }
            }"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("composer.lock"), "{}\n").unwrap();
        std::fs::write(dir.path().join("artisan"), "#!/usr/bin/env php\n").unwrap();
        std::fs::write(dir.path().join("pest.php"), "<?php\n").unwrap();
        std::fs::write(dir.path().join("phpstan.neon"), "parameters:\n").unwrap();

        let context = inspect_project(dir.path());

        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Laravel"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Pest"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "PHPStan"));
        assert_eq!(context.validation_commands["test"], "php artisan test");
        assert_eq!(
            context.validation_commands["lint"],
            "./vendor/bin/phpstan analyse"
        );
        assert_eq!(
            context.validation_commands["format"],
            "./vendor/bin/pint --test"
        );
        assert_eq!(context.validation_commands["primary"], "php artisan test");
        assert_eq!(
            context.validation_commands["fast"],
            "./vendor/bin/phpstan analyse"
        );
    }

    #[test]
    fn inspect_project_detects_rails_ruby_tooling() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("Gemfile"),
            r#"source 'https://rubygems.org'
gem 'rails'
gem 'rspec-rails'
gem 'rubocop'
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("Gemfile.lock"), "GEM\n").unwrap();
        std::fs::write(dir.path().join(".rubocop.yml"), "AllCops:\n").unwrap();
        std::fs::write(dir.path().join(".rspec"), "--format documentation\n").unwrap();
        std::fs::write(dir.path().join("Rakefile"), "task default: []\n").unwrap();

        let context = inspect_project(dir.path());

        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Rails"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "RSpec"));
        assert!(context.project_profile["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "RuboCop"));
        assert_eq!(context.validation_commands["test"], "bundle exec rspec");
        assert_eq!(
            context.validation_commands["lint"],
            "bundle exec rubocop --parallel"
        );
        assert_eq!(
            context.validation_commands["format"],
            "bundle exec rubocop --parallel --format simple"
        );
        assert_eq!(context.validation_commands["primary"], "bundle exec rspec");
        assert_eq!(
            context.validation_commands["fast"],
            "bundle exec rubocop --parallel"
        );
    }
}
