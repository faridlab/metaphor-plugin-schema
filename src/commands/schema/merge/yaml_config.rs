//! YAML config merge — user values always win.
//!
//! Used for `config/application*.yml`. The generator emits a default set of
//! keys; on regen we deep-merge that into the existing on-disk YAML so that
//! user-set values (most importantly `database.url`) are never overwritten,
//! while *new* keys added in newer generator versions still land.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Merge generated YAML config with the existing file, preserving user values.
///
/// - User-defined values are kept unchanged.
/// - New keys from the generator that don't exist in the user file are added.
/// - For nested mappings, recursion is applied with the same precedence rule.
pub(in crate::commands::schema) fn merge_yaml_config(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing config file: {:?}", existing_path))?;

    let generated_value: serde_yaml::Value = serde_yaml::from_str(generated_content)
        .with_context(|| format!("Failed to parse generated config: {:?}", existing_path))?;
    let existing_value: serde_yaml::Value = serde_yaml::from_str(&existing_content)
        .with_context(|| format!("Failed to parse existing config: {:?}", existing_path))?;

    // Existing (user) config takes precedence over generated — keeps
    // database.url and other user settings from ever being overwritten.
    let merged = deep_merge_yaml_preserve_user(existing_value, generated_value);

    serde_yaml::to_string(&merged)
        .with_context(|| format!("Failed to serialize merged config: {:?}", existing_path))
}

/// Deep merge two YAML values with `user` taking precedence over `generated`.
fn deep_merge_yaml_preserve_user(
    user: serde_yaml::Value,
    generated: serde_yaml::Value,
) -> serde_yaml::Value {
    match (user, generated) {
        (serde_yaml::Value::Mapping(user_map), serde_yaml::Value::Mapping(generated_map)) => {
            let mut merged = user_map.clone();

            for (key, generated_value) in generated_map {
                let merged_value = match merged.get(&key) {
                    Some(user_value) => {
                        if let (serde_yaml::Value::Mapping(_), serde_yaml::Value::Mapping(_)) =
                            (user_value, &generated_value)
                        {
                            deep_merge_yaml_preserve_user(user_value.clone(), generated_value)
                        } else {
                            // Non-mapping user value wins outright.
                            user_value.clone()
                        }
                    }
                    None => generated_value,
                };
                merged.insert(key, merged_value);
            }
            serde_yaml::Value::Mapping(merged)
        }
        (user_value, _) => user_value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deep_merge_yaml_preserve_user_adds_new_keys() {
        let user = serde_yaml::from_str::<serde_yaml::Value>("a: 1").unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>("b: 2").unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        assert!(result.get("a").is_some());
        assert!(result.get("b").is_some());
    }

    #[test]
    fn test_deep_merge_yaml_user_takes_precedence() {
        let user = serde_yaml::from_str::<serde_yaml::Value>("a: 1\nb: 2").unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>("b: 999\nc: 3").unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        let b_value = result.get("b").unwrap().as_i64().unwrap();
        assert_eq!(b_value, 2);
        assert!(result.get("c").is_some());
    }

    #[test]
    fn test_deep_merge_yaml_database_url_never_overridden() {
        let user = serde_yaml::from_str::<serde_yaml::Value>(
            "database:\n  url: postgresql://user:pass@localhost/db\n  max_connections: 10",
        )
        .unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>(
            "database:\n  url: postgresql://default:123@host/defaultdb\n  pool_size: 5",
        )
        .unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        let db = result.get("database").unwrap().as_mapping().unwrap();
        assert_eq!(
            db.get(&serde_yaml::Value::String("url".to_string()))
                .unwrap()
                .as_str()
                .unwrap(),
            "postgresql://user:pass@localhost/db"
        );
        assert_eq!(
            db.get(&serde_yaml::Value::String("max_connections".to_string()))
                .unwrap()
                .as_i64()
                .unwrap(),
            10
        );
        assert!(db
            .get(&serde_yaml::Value::String("pool_size".to_string()))
            .is_some());
    }

    #[test]
    fn test_deep_merge_yaml_recursive_merge() {
        let user =
            serde_yaml::from_str::<serde_yaml::Value>("server:\n  host: localhost\n  port: 3000")
                .unwrap();
        let generated =
            serde_yaml::from_str::<serde_yaml::Value>("server:\n  port: 8080\n  ssl: true")
                .unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        let server = result.get("server").unwrap().as_mapping().unwrap();
        assert_eq!(
            server
                .get(&serde_yaml::Value::String("host".to_string()))
                .unwrap()
                .as_str()
                .unwrap(),
            "localhost"
        );
        // User's port (3000) wins over generated (8080).
        // YAML parses unquoted integers as Number, not String — compare numerically.
        assert_eq!(
            server
                .get(&serde_yaml::Value::String("port".to_string()))
                .unwrap()
                .as_i64()
                .unwrap(),
            3000
        );
        assert!(server
            .get(&serde_yaml::Value::String("ssl".to_string()))
            .is_some());
    }
}
