//! Package name auto-detection for Kotlin projects
//!
//! This module attempts to detect the correct package name for generated Kotlin code
//! by analyzing the existing project structure and configuration files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

/// Detected package information
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// The base package name (e.g., "com.bersihir")
    pub base_package: String,
    /// The source of this detection
    pub source: PackageSource,
}

/// How the package was detected
#[derive(Debug, Clone)]
pub enum PackageSource {
    /// Detected from build.gradle.kts namespace
    GradleNamespace(PathBuf),
    /// Detected from build.gradle.kts SQLDelight packageName
    SqlDelightPackage(PathBuf),
    /// Detected from existing Kotlin files
    ExistingKotlinFiles,
    /// Default fallback
    Default,
}

/// Detect the package name from a Kotlin project directory
///
/// # Arguments
/// * `project_dir` - Path to the project (e.g., apps/mobileapp/shared/src/commonMain)
///                  The function will search up the directory tree for build.gradle.kts
///
/// # Returns
/// The detected package information
///
/// # Detection Strategy (in priority order):
/// 1. Parse `build.gradle.kts` for `android { namespace = "..." }` (searching up the tree)
/// 2. Parse `build.gradle.kts` for `sqldelight { packageName.set("...") }`
/// 3. Scan existing Kotlin source files for package declarations
/// 4. Fall back to "id.startapp"
pub fn detect_package(project_dir: &Path) -> PackageInfo {
    // First, try to find build.gradle.kts by walking up the directory tree
    if let Some((build_gradle, content)) = find_build_gradle(project_dir) {
        // Try SQLDelight packageName first (most accurate)
        if let Some(pkg) = parse_sqldelight_package(&content) {
            return PackageInfo {
                base_package: pkg,
                source: PackageSource::SqlDelightPackage(build_gradle),
            };
        }

        // Try android namespace
        if let Some(pkg) = parse_android_namespace(&content) {
            return PackageInfo {
                base_package: pkg,
                source: PackageSource::GradleNamespace(build_gradle),
            };
        }
    }

    // Try scanning existing Kotlin files (from the current directory)
    let kotlin_base = project_dir.join("kotlin");
    if !kotlin_base.exists() {
        // If kotlin/ doesn't exist here, we might be in commonMain directly
        let kotlin_base_alt = project_dir.join("../kotlin");
        if let Some(pkg) = kotlin_base_alt.exists().then(|| scan_kotlin_packages(&kotlin_base_alt)).flatten() {
            return PackageInfo {
                base_package: pkg,
                source: PackageSource::ExistingKotlinFiles,
            };
        }
    }

    if kotlin_base.exists() {
        if let Some(pkg) = scan_kotlin_packages(&kotlin_base) {
            return PackageInfo {
                base_package: pkg,
                source: PackageSource::ExistingKotlinFiles,
            };
        }
    }

    // Fallback to default
    PackageInfo {
        base_package: "id.startapp".to_string(),
        source: PackageSource::Default,
    }
}

/// Find build.gradle.kts by walking up the directory tree
///
/// Returns (path_to_build_gradle, content) if found
fn find_build_gradle(start_dir: &Path) -> Option<(PathBuf, String)> {
    let mut current = start_dir;

    // Search up to 5 levels up
    for _ in 0..5 {
        let build_gradle = current.join("build.gradle.kts");

        if build_gradle.exists() {
            if let Ok(content) = fs::read_to_string(&build_gradle) {
                return Some((build_gradle, content));
            }
        }

        // Go up one directory
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    None
}

/// Parse android namespace from build.gradle.kts
///
/// Extracts "com.bersihir" from `namespace = "com.bersihir.shared"`
fn parse_android_namespace(content: &str) -> Option<String> {
    // Look for: namespace = "com.bersihir.shared"
    let namespace_pattern = regex::Regex::new(r#"namespace\s*=\s*"([^"]+)""#).ok()?;

    namespace_pattern.captures(content).map(|caps| {
        let full_namespace = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        // Strip ".shared" suffix if present
        let base = full_namespace
            .strip_suffix(".shared")
            .or_else(|| full_namespace.strip_suffix(".android"))
            .unwrap_or(full_namespace);
        base.to_string()
    })
}

/// Parse SQLDelight packageName from build.gradle.kts
///
/// Extracts "com.bersihir" from `packageName.set("com.bersihir")`
fn parse_sqldelight_package(content: &str) -> Option<String> {
    // Look for: packageName.set("com.bersihir")
    let package_pattern = regex::Regex::new(r#"packageName\.set\("([^"]+)"\)"#).ok()?;

    package_pattern
        .captures(content)
        .and_then(|caps| caps.get(1).map(|m| m.as_str()))
        .map(|s| s.to_string())
}

/// Scan existing Kotlin files for package declarations
///
/// Returns the most common package found in the source directory
fn scan_kotlin_packages(kotlin_dir: &Path) -> Option<String> {
    let mut package_counts: HashMap<String, usize> = HashMap::new();

    // Find all .kt files
    let kt_files = find_kt_files(kotlin_dir);
    let kt_files: Vec<_> = kt_files
        .into_iter()
        .filter(|p| {
            // Skip build directories
            !p.to_string_lossy().contains("/build/")
        })
        .collect();

    // Count package declarations
    for file in kt_files {
        if let Ok(content) = fs::read_to_string(file) {
            if let Some(pkg) = extract_package_from_kt(&content) {
                // Get base package (first 2 components: com.bersihir)
                let base = base_package_from(&pkg);
                *package_counts.entry(base).or_insert(0) += 1;
            }
        }
    }

    // Return most common package
    package_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(pkg, _)| pkg)
}

/// Find all .kt files in a directory recursively
fn find_kt_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_kt_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("kt") {
                files.push(path);
            }
        }
    }

    files
}

/// Extract package name from Kotlin source code
///
/// Returns "com.bersihir.domain.auth" from "package com.bersihir.domain.auth"
fn extract_package_from_kt(content: &str) -> Option<String> {
    content
        .lines()
        .find(|line| line.trim().starts_with("package "))
        .and_then(|line| {
            line.trim()
                .strip_prefix("package ")
                .map(|s| s.trim().trim_end_matches(';').to_string())
        })
}

/// Get base package from full package path
///
/// Returns "com.bersihir" from "com.bersihir.domain.auth.entity"
fn base_package_from(full_package: &str) -> String {
    let parts: Vec<&str> = full_package.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[0], parts[1])
    } else {
        full_package.to_string()
    }
}

/// Resolve the actual package name to use for code generation
///
/// This function combines the detected base package with the module name
/// to create the full package path for generated code.
///
/// # Arguments
/// * `base_package` - The base package (e.g., "com.bersihir")
/// * `module_name` - The module name (e.g., "sapiens", "bersihir")
/// * `layer` - The architecture layer (e.g., "domain", "application")
///
/// # Returns
/// The full package path for generated code
///
/// # Examples
/// ```
/// let pkg = resolve_package("com.bersihir", "sapiens", "domain");
/// assert_eq!(pkg, "com.bersihir.domain.sapiens");
///
/// let pkg = resolve_package("com.bersihir", "sapiens", "domain");
/// // For entity subpackage:
/// let entity_pkg = format!("{}.entity", pkg);
/// assert_eq!(entity_pkg, "com.bersihir.domain.sapiens.entity");
/// ```
pub fn resolve_package(base_package: &str, module_name: &str, layer: &str) -> String {
    // Pattern: {base_package}.{layer}.{module_name}
    // Example: com.bersihir.domain.sapiens
    format!("{}.{}.{}", base_package, layer, module_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_android_namespace() {
        let gradle = r#"
            android {
                namespace = "com.bersihir.shared"
                compileSdk = 35
            }
        "#;
        let result = parse_android_namespace(gradle);
        assert_eq!(result, Some("com.bersihir".to_string()));
    }

    #[test]
    fn test_parse_android_namespace_without_suffix() {
        let gradle = r#"
            android {
                namespace = "com.example.mobile"
            }
        "#;
        let result = parse_android_namespace(gradle);
        assert_eq!(result, Some("com.example".to_string()));
    }

    #[test]
    fn test_parse_sqldelight_package() {
        let gradle = r#"
            sqldelight {
                databases {
                    create("AppDatabase") {
                        packageName.set("com.bersihir")
                    }
                }
            }
        "#;
        let result = parse_sqldelight_package(gradle);
        assert_eq!(result, Some("com.bersihir".to_string()));
    }

    #[test]
    fn test_extract_package_from_kt() {
        let kt_code = r#"
            package com.bersihir.domain.auth.entity

            import kotlinx.serialization.Serializable
        "#;
        let result = extract_package_from_kt(kt_code);
        assert_eq!(result, Some("com.bersihir.domain.auth.entity".to_string()));
    }

    #[test]
    fn test_base_package_from() {
        assert_eq!(base_package_from("com.bersihir.domain.auth.entity"), "com.bersihir");
        assert_eq!(base_package_from("id.startapp.domain.sapiens"), "id.startapp");
        assert_eq!(base_package_from("single"), "single");
    }

    #[test]
    fn test_resolve_package() {
        assert_eq!(
            resolve_package("com.bersihir", "sapiens", "domain"),
            "com.bersihir.domain.sapiens"
        );
        assert_eq!(
            resolve_package("id.startapp", "bersihir", "application"),
            "id.startapp.application.bersihir"
        );
    }
}
