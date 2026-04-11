//! Shared utility functions for metaphor-schema
//!
//! This module provides common utilities used across generators and parsers.

/// Convert a PascalCase or camelCase string to snake_case.
///
/// Handles acronyms properly:
/// - "MFADevice" -> "mfa_device"
/// - "UserID" -> "user_id"
/// - "HTTPRequest" -> "http_request"
///
/// # Examples
///
/// ```
/// use metaphor_schema::utils::to_snake_case;
///
/// assert_eq!(to_snake_case("MFADevice"), "mfa_device");
/// assert_eq!(to_snake_case("UserID"), "user_id");
/// assert_eq!(to_snake_case("HTTPRequest"), "http_request");
/// assert_eq!(to_snake_case("SimpleCase"), "simple_case");
/// ```
pub fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = name.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

                // Add underscore before this uppercase if:
                // 1. Previous char is lowercase (e.g., "userN" -> "user_n")
                // 2. Previous char is digit (e.g., "OAuth2T" -> "oauth2_t")
                // 3. This is the start of a new word after acronym (e.g., "HTTPRequest" -> "http_request")
                //    detected when prev is uppercase AND next is lowercase
                if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_is_lower) {
                    result.push('_');
                }
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert a snake_case string to PascalCase.
///
/// # Examples
///
/// ```
/// use metaphor_schema::utils::to_pascal_case;
///
/// assert_eq!(to_pascal_case("mfa_device"), "MfaDevice");
/// assert_eq!(to_pascal_case("user_id"), "UserId");
/// ```
pub fn to_pascal_case(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Pluralize a word (simple English rules).
///
/// # Examples
///
/// ```
/// use metaphor_schema::utils::pluralize;
///
/// assert_eq!(pluralize("user"), "users");
/// assert_eq!(pluralize("category"), "categories");
/// assert_eq!(pluralize("status"), "statuses");
/// ```
pub fn pluralize(word: &str) -> String {
    if word.ends_with('y') && !word.ends_with("ey") && !word.ends_with("ay") && !word.ends_with("oy") && !word.ends_with("uy") {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s') || word.ends_with('x') || word.ends_with("ch") || word.ends_with("sh") {
        format!("{}es", word)
    } else {
        format!("{}s", word)
    }
}

/// Escape Rust reserved keywords by prefixing with r#
///
/// # Examples
///
/// ```
/// use metaphor_schema::utils::escape_rust_keyword;
///
/// assert_eq!(escape_rust_keyword("use"), "r#use");
/// assert_eq!(escape_rust_keyword("type"), "r#type");
/// assert_eq!(escape_rust_keyword("normal"), "normal");
/// ```
pub fn escape_rust_keyword(name: &str) -> String {
    const RUST_KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
        "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become",
        "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized",
        "virtual", "yield",
    ];

    if RUST_KEYWORDS.contains(&name) {
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case_simple() {
        assert_eq!(to_snake_case("User"), "user");
        assert_eq!(to_snake_case("SimpleCase"), "simple_case");
        assert_eq!(to_snake_case("UserProfile"), "user_profile");
    }

    #[test]
    fn test_to_snake_case_acronyms() {
        assert_eq!(to_snake_case("MFADevice"), "mfa_device");
        assert_eq!(to_snake_case("UserID"), "user_id");
        assert_eq!(to_snake_case("HTTPRequest"), "http_request");
        assert_eq!(to_snake_case("APIKey"), "api_key");
        assert_eq!(to_snake_case("URLParser"), "url_parser");
        assert_eq!(to_snake_case("PasswordResetToken"), "password_reset_token");
    }

    #[test]
    fn test_to_snake_case_mixed() {
        assert_eq!(to_snake_case("GetUserByID"), "get_user_by_id");
        assert_eq!(to_snake_case("ParseHTTPResponse"), "parse_http_response");
        assert_eq!(to_snake_case("MFADeviceSettings"), "mfa_device_settings");
    }

    #[test]
    fn test_to_snake_case_already_lowercase() {
        assert_eq!(to_snake_case("user"), "user");
        assert_eq!(to_snake_case("already_snake"), "already_snake");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("user_profile"), "UserProfile");
        assert_eq!(to_pascal_case("mfa_device"), "MfaDevice");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize("user"), "users");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("status"), "statuses");
        assert_eq!(pluralize("box"), "boxes");
        assert_eq!(pluralize("match"), "matches");
        assert_eq!(pluralize("key"), "keys");
    }

    #[test]
    fn test_escape_rust_keyword() {
        assert_eq!(escape_rust_keyword("use"), "r#use");
        assert_eq!(escape_rust_keyword("type"), "r#type");
        assert_eq!(escape_rust_keyword("match"), "r#match");
        assert_eq!(escape_rust_keyword("async"), "r#async");
        assert_eq!(escape_rust_keyword("normal"), "normal");
        assert_eq!(escape_rust_keyword("create"), "create");
    }
}
