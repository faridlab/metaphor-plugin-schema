//! YAML Parser for schema files
//!
//! Parses YAML-based schema files into AST structures.
//! This replaces the custom lexer/parser with serde_yaml deserialization.

mod types;
mod parsers;
mod resolver;
mod converters;
mod helpers;

pub use types::*;
pub use parsers::{
    parse_model_yaml, parse_model_yaml_str,
    parse_hook_yaml, parse_hook_yaml_str,
    parse_workflow_yaml, parse_workflow_yaml_str,
    parse_hook_yaml_flexible, parse_hook_index_yaml_str,
    parse_model_yaml_flexible, parse_model_index_yaml_str,
    is_hook_index_file, is_model_index_file,
    YamlHookParseResult, YamlModelParseResult,
};
pub use resolver::resolve_shared_types;

#[cfg(test)]
mod tests;
