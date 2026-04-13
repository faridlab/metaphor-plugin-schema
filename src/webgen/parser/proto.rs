//! Proto file parser

use std::fs;
use std::path::PathBuf;
use crate::webgen::{Error, Result};

/// Proto entity information
#[derive(Debug, Clone)]
pub struct ProtoEntity {
    /// Entity name in PascalCase
    pub name: String,
    /// Proto file name
    pub proto_file: String,
    /// Fields in the entity
    pub fields: Vec<ProtoField>,
}

/// Proto field information
#[derive(Debug, Clone)]
pub struct ProtoField {
    /// Field name
    pub name: String,
    /// Field type name
    pub type_name: String,
    /// Whether this is a repeated field
    pub repeated: bool,
    /// Whether this is an optional field
    pub optional: bool,
}

/// Proto parser for finding entities in proto files
pub struct ProtoParser;

impl ProtoParser {
    /// Find all proto entities in a module's proto directory
    pub fn find_entities(proto_dir: &PathBuf, entity_filter: Option<&str>) -> Result<Vec<ProtoEntity>> {
        if !proto_dir.exists() {
            return Err(Error::ProtoNotFound(proto_dir.clone()));
        }

        let proto_files = Self::find_proto_files(proto_dir)?;
        let mut entities = Vec::new();

        for proto_file in proto_files {
            let content = fs::read_to_string(&proto_file)
                .map_err(|e| Error::Parse(format!("Failed to read {}: {}", proto_file.display(), e)))?;

            // Find message definitions (simplified proto parsing)
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("message ") {
                    let name = line
                        .strip_prefix("message ")
                        .and_then(|s| s.split_whitespace().next())
                        .map(|s| s.trim_end_matches('{').trim())
                        .unwrap_or("");

                    // Filter out Request/Response messages and empty names
                    if !name.is_empty()
                        && !name.contains("Request")
                        && !name.contains("Response")
                        && !name.ends_with("Request")
                        && !name.ends_with("Response")
                    {
                        if let Some(filter) = entity_filter {
                            if name.eq_ignore_ascii_case(filter)
                                || name.eq_ignore_ascii_case(&to_pascal_case(filter))
                            {
                                entities.push(ProtoEntity {
                                    name: name.to_string(),
                                    proto_file: proto_file.file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string(),
                                    fields: Vec::new(), // Fields can be parsed later if needed
                                });
                            }
                        } else {
                            entities.push(ProtoEntity {
                                name: name.to_string(),
                                proto_file: proto_file.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                                fields: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        Ok(entities)
    }

    /// Find all proto files in a directory recursively
    fn find_proto_files(dir: &PathBuf) -> Result<Vec<PathBuf>> {
        let mut proto_files = Vec::new();

        if dir.is_dir() {
            for entry in fs::read_dir(dir)
                .map_err(|e| Error::Parse(format!("Failed to read directory {}: {}", dir.display(), e)))?
            {
                let entry = entry.map_err(|e| Error::Parse(format!("Failed to read entry: {}", e)))?;
                let path = entry.path();

                if path.is_dir() {
                    proto_files.extend(Self::find_proto_files(&path)?);
                } else if path.extension().and_then(|s| s.to_str()) == Some("proto") {
                    proto_files.push(path);
                }
            }
        }

        proto_files.sort();
        proto_files.dedup();
        Ok(proto_files)
    }
}

/// Convert to snake_case
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

                // Add underscore before this uppercase if:
                // 1. Previous char is lowercase (e.g., "userN" -> "user_n")
                // 2. Previous char is digit (e.g., "OAuth2T" -> "oauth2_t")
                // 3. This is the start of a new word after acronym (e.g., "MFADevice" -> "mfa_device")
                //    detected when prev is uppercase AND next is lowercase
                if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_is_lower) {
                    result.push('_');
                }
            }
            result.push(c.to_ascii_lowercase());
        } else if c == '-' {
            result.push('_');
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert to PascalCase
pub fn to_pascal_case(s: &str) -> String {
    s.split(['_', '-', ' '])
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    if first.is_lowercase() {
                        first.to_uppercase().collect::<String>() + chars.as_str()
                    } else {
                        word.to_string()
                    }
                }
            }
        })
        .collect()
}

/// Convert to camelCase
pub fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

/// Convert to kebab-case
pub fn to_kebab_case(s: &str) -> String {
    to_snake_case(s).replace('_', "-")
}

/// Pluralize a word (simple English rules)
pub fn pluralize(word: &str) -> String {
    if word.ends_with('y') && !word.ends_with("ey") && !word.ends_with("ay") && !word.ends_with("oy") && !word.ends_with("uy") {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s') || word.ends_with('x') || word.ends_with("ch") || word.ends_with("sh") {
        format!("{}es", word)
    } else {
        format!("{}s", word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_case() {
        assert_eq!(to_snake_case("UserName"), "user_name");
        assert_eq!(to_snake_case("user-name"), "user_name");
        assert_eq!(to_snake_case("user_name"), "user_name");
        assert_eq!(to_snake_case("User"), "user");
        assert_eq!(to_snake_case("MFADevice"), "mfa_device");
        assert_eq!(to_snake_case("LDAPDirectory"), "ldap_directory");
        assert_eq!(to_snake_case("SAMLProvider"), "saml_provider");
        assert_eq!(to_snake_case("OAuthProvider"), "oauth_provider");
        assert_eq!(to_snake_case("HTTPRequest"), "http_request");
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(to_pascal_case("user_name"), "UserName");
        assert_eq!(to_pascal_case("user-name"), "UserName");
        assert_eq!(to_pascal_case("user name"), "UserName");
        assert_eq!(to_pascal_case("User"), "User");
    }

    #[test]
    fn test_camel_case() {
        assert_eq!(to_camel_case("user_name"), "userName");
        assert_eq!(to_camel_case("User"), "user");
        assert_eq!(to_camel_case("User_Name"), "userName");
    }
}
