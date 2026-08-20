//! YAML Parser for schema files
//!
//! Parses YAML-based schema files into AST structures.
//! This replaces the custom lexer/parser with serde_yaml deserialization.

mod converters;
mod helpers;
mod parsers;
mod resolver;
mod types;

pub use parsers::{
    is_hook_index_file, is_model_index_file, parse_hook_index_yaml_str, parse_hook_yaml,
    parse_hook_yaml_flexible, parse_hook_yaml_str, parse_model_index_yaml_str, parse_model_yaml,
    parse_model_yaml_flexible, parse_model_yaml_str, parse_workflow_yaml, parse_workflow_yaml_str,
    YamlHookParseResult, YamlModelParseResult,
};
pub use resolver::resolve_shared_types;
pub use types::*;

#[cfg(test)]
mod tests;
