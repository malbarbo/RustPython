use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const CRATE_ROOT: &str = "../..";

fn main() {
    println!("cargo:rerun-if-env-changed=FREEZE_SEEDS");
    println!("cargo:rerun-if-env-changed=FREEZE_ALLOWLIST");

    // If FREEZE_SEEDS is set, generate the allowlist from seed modules.
    // If FREEZE_ALLOWLIST is set (as a file path), use it directly.
    // Otherwise, freeze everything (no filtering).
    if let Ok(seeds) = std::env::var("FREEZE_SEEDS") {
        let lib_dir = resolve_lib_dir();
        let seeds: Vec<&str> = seeds.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let modules = collect_deps(&seeds, &lib_dir);
        let mut sorted: Vec<&str> = modules.iter().map(|s| s.as_str()).collect();
        sorted.sort();

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let allowlist_path = Path::new(&out_dir).join("freeze_allowlist.txt");
        fs::write(&allowlist_path, sorted.join("\n") + "\n").unwrap();
        println!("cargo:rustc-env=FREEZE_ALLOWLIST={}", allowlist_path.display());
    } else if let Ok(path) = std::env::var("FREEZE_ALLOWLIST") {
        println!("cargo:rerun-if-changed={path}");
    }

    process_python_libs(format!("{CRATE_ROOT}/vm/Lib/python_builtins/*").as_str());
    process_python_libs(format!("{CRATE_ROOT}/vm/Lib/core_modules/*").as_str());

    #[cfg(feature = "freeze-stdlib")]
    if cfg!(windows) {
        process_python_libs(format!("{CRATE_ROOT}/Lib/**/*").as_str());
    } else {
        process_python_libs("./Lib/**/*");
    }

    if cfg!(windows) {
        let lib_path = if let Ok(real_path) = fs::read_to_string("Lib") {
            PathBuf::from(real_path.trim())
        } else {
            PathBuf::from("Lib")
        };

        if let Ok(canonicalized_path) = fs::canonicalize(&lib_path) {
            let path_str = canonicalized_path.to_str().unwrap();
            let path_str = path_str.strip_prefix(r"\\?\").unwrap_or(path_str);
            println!("cargo:rustc-env=win_lib_path={path_str}");
        }
    }
}

// ── Freeze seed resolution ──────────────────────────────────────────

fn resolve_lib_dir() -> PathBuf {
    let lib = PathBuf::from("Lib");
    if lib.is_dir() {
        return lib;
    }
    // Symlink target
    if let Ok(target) = fs::read_to_string(&lib) {
        let p = PathBuf::from(target.trim());
        if p.is_dir() {
            return p;
        }
    }
    lib
}

fn resolve_module(name: &str, lib_dir: &Path) -> Option<(PathBuf, bool)> {
    let parts: Vec<&str> = name.split('.').collect();
    // Try as package: foo/bar/__init__.py
    let mut pkg_path = lib_dir.to_path_buf();
    for p in &parts {
        pkg_path.push(p);
    }
    pkg_path.push("__init__.py");
    if pkg_path.is_file() {
        return Some((pkg_path, true));
    }
    // Try as module: foo/bar.py
    let mut mod_path = lib_dir.to_path_buf();
    for p in &parts {
        mod_path.push(p);
    }
    mod_path.set_extension("py");
    if mod_path.is_file() {
        return Some((mod_path, false));
    }
    None
}

fn collect_deps(seeds: &[&str], lib_dir: &Path) -> HashSet<String> {
    let mut seen_modules = HashSet::new();
    let mut seen_files = HashSet::new();
    let mut stack: Vec<String> = seeds.iter().map(|s| s.to_string()).collect();

    while let Some(module) = stack.pop() {
        if !seen_modules.insert(module.clone()) {
            continue;
        }

        // Ensure parent packages are included
        let parts: Vec<&str> = module.split('.').collect();
        for i in 1..parts.len() {
            let parent = parts[..i].join(".");
            if !seen_modules.contains(&parent) {
                stack.push(parent);
            }
        }

        let (file_path, is_package) = match resolve_module(&module, lib_dir) {
            Some(r) => r,
            None => continue,
        };

        if !seen_files.insert(file_path.clone()) {
            continue;
        }

        let pkg_context = if is_package {
            Some(module.as_str())
        } else {
            module.rsplit_once('.').map(|(parent, _)| parent)
        };

        for imp in get_imports(&file_path, pkg_context) {
            if !seen_modules.contains(&imp) {
                stack.push(imp);
            }
        }
    }

    // The encodings package loads codecs dynamically by name at runtime.
    if seen_modules.contains("encodings") {
        for enc in ["encodings.aliases", "encodings.ascii", "encodings.latin_1", "encodings.utf_8"] {
            seen_modules.insert(enc.to_string());
        }
    }

    // Keep only modules that resolve to actual files
    seen_modules.retain(|m| resolve_module(m, lib_dir).is_some());
    seen_modules
}

/// Parse a Python file for top-level import statements.
///
/// Handles `import X`, `from X import Y`, relative imports, and skips
/// imports inside `if __name__ == "__main__"` guards. Only processes
/// imports at module level and inside class/if/try bodies (not functions).
fn get_imports(filepath: &Path, package: Option<&str>) -> Vec<String> {
    let content = match fs::read_to_string(filepath) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut imports = Vec::new();
    // Track indent to skip function bodies.
    // We process: indent 0 (module level), and statements inside class/if/try
    // We skip: `def` bodies and `if __name__ == "__main__"` guards
    let mut skip_indent: Option<usize> = None;

    // Build a mapping of line -> indent for block tracking
    let lines: Vec<&str> = content.lines().collect();

    for line in &lines {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // If we're skipping a block, check if we've dedented past it
        if let Some(skip) = skip_indent {
            if indent > skip {
                continue;
            }
            skip_indent = None;
        }

        // Skip function bodies
        if stripped.starts_with("def ") {
            skip_indent = Some(indent);
            continue;
        }

        // Skip `if __name__ == "__main__":` guard
        if stripped.contains("__name__") && stripped.contains("__main__") && stripped.ends_with(':') {
            skip_indent = Some(indent);
            continue;
        }

        // Parse import statements
        if stripped.starts_with("import ") {
            parse_import(stripped, &mut imports);
        } else if stripped.starts_with("from ") {
            parse_from_import(stripped, package, &mut imports);
        }
    }

    imports
}

fn parse_import(line: &str, imports: &mut Vec<String>) {
    // import foo, bar as baz, quux
    let rest = &line[7..]; // skip "import "
    for part in rest.split(',') {
        let name = part.trim().split_whitespace().next().unwrap_or("");
        if !name.is_empty() {
            imports.push(name.to_string());
        }
    }
}

fn parse_from_import(line: &str, package: Option<&str>, imports: &mut Vec<String>) {
    // from foo.bar import baz, quux
    // from . import baz
    // from .foo import bar
    let Some(import_pos) = line.find(" import ") else {
        return;
    };
    let from_part = line[5..import_pos].trim(); // between "from " and " import "
    let import_part = &line[import_pos + 8..]; // after " import "

    // Count leading dots for relative imports
    let dots = from_part.chars().take_while(|c| *c == '.').count();
    let module_name = &from_part[dots..];

    let resolved = if dots > 0 {
        resolve_relative(module_name, dots, package)
    } else {
        module_name.to_string()
    };

    if !resolved.is_empty() {
        imports.push(resolved.clone());
    }

    for part in import_part.split(',') {
        let name = part.trim().split_whitespace().next().unwrap_or("");
        if !name.is_empty() && name != "*" {
            if resolved.is_empty() {
                imports.push(name.to_string());
            } else {
                imports.push(format!("{resolved}.{name}"));
            }
        }
    }
}

fn resolve_relative(module: &str, level: usize, package: Option<&str>) -> String {
    let Some(pkg) = package else {
        return module.to_string();
    };
    let parts: Vec<&str> = pkg.split('.').collect();
    if level > parts.len() {
        return module.to_string();
    }
    let base = parts[..parts.len() - level + 1].join(".");
    if module.is_empty() {
        base
    } else {
        format!("{base}.{module}")
    }
}

// ── Process Python libs for rerun-if-changed ────────────────────────

fn process_python_libs(pattern: &str) {
    let glob = glob::glob(pattern).unwrap_or_else(|e| panic!("failed to glob {pattern:?}: {e}"));
    for entry in glob.flatten() {
        if entry.is_dir() {
            continue;
        }
        let display = entry.display();
        if display.to_string().ends_with(".pyc") {
            if fs::remove_file(&entry).is_err() {
                println!("cargo:warning=failed to remove {display}")
            }
            continue;
        }
        println!("cargo:rerun-if-changed={display}");
    }
}
