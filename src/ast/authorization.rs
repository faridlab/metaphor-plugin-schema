//! Authorization AST definitions
//!
//! Defines AST nodes for authorization schemas including permissions,
//! roles, policies, and attribute-based access control (ABAC).

use super::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

// =============================================================================
// AUTHORIZATION CONFIGURATION
// =============================================================================

/// Complete authorization configuration for a module
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthorizationConfig {
    /// Resource permissions (resource_name -> allowed_actions)
    pub permissions: IndexMap<String, Vec<String>>,
    /// Role definitions
    pub roles: Vec<RoleDefinition>,
    /// Policy definitions
    pub policies: Vec<PolicyDefinition>,
    /// Resource-level policy mappings
    pub resource_policies: IndexMap<String, ResourcePolicy>,
    /// ABAC attribute definitions
    pub attributes: Option<AbacAttributes>,
    /// ABAC policies
    pub abac_policies: Vec<AbacPolicy>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl AuthorizationConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a role exists
    pub fn has_role(&self, name: &str) -> bool {
        self.roles.iter().any(|r| r.name == name)
    }

    /// Find a role by name
    pub fn find_role(&self, name: &str) -> Option<&RoleDefinition> {
        self.roles.iter().find(|r| r.name == name)
    }

    /// Find a policy by name
    pub fn find_policy(&self, name: &str) -> Option<&PolicyDefinition> {
        self.policies.iter().find(|p| p.name == name)
    }

    /// Get all permissions for a resource
    pub fn resource_permissions(&self, resource: &str) -> Option<&Vec<String>> {
        self.permissions.get(resource)
    }

    /// Check if a permission exists
    pub fn has_permission(&self, resource: &str, action: &str) -> bool {
        self.permissions
            .get(resource)
            .map(|actions| actions.iter().any(|a| a == action))
            .unwrap_or(false)
    }
}

// =============================================================================
// ROLE DEFINITIONS
// =============================================================================

/// A role definition with permissions and hierarchy
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Role name (e.g., "admin", "user", "guest")
    pub name: String,
    /// Description of this role
    pub description: Option<String>,
    /// Assigned permissions (supports wildcards like "users.*")
    pub permissions: Vec<String>,
    /// Role level in hierarchy (higher = more privileged)
    pub level: Option<i32>,
    /// Parent role name (inherits permissions from)
    pub inherits: Option<String>,
    /// Own resource permissions (e.g., owner-based access)
    pub own_resources: IndexMap<String, String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl RoleDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Check if this role has a specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| {
            p == permission
                || p == "*"
                || (p.ends_with(".*") && permission.starts_with(&p[..p.len() - 2]))
        })
    }

    /// Check if this role is higher than another role (by level)
    pub fn is_higher_than(&self, other: &RoleDefinition) -> bool {
        match (self.level, other.level) {
            (Some(a), Some(b)) => a > b,
            _ => false,
        }
    }
}

// =============================================================================
// POLICY DEFINITIONS
// =============================================================================

/// A policy definition with complex access rules
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyDefinition {
    /// Policy name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Policy type (how rules are combined)
    pub policy_type: PolicyType,
    /// Policy rules
    pub rules: Vec<PolicyRule>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl PolicyDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            policy_type: PolicyType::Any,
            ..Default::default()
        }
    }

    /// Check if this is an "all" (AND) policy
    pub fn is_all(&self) -> bool {
        self.policy_type == PolicyType::All
    }

    /// Check if this is an "any" (OR) policy
    pub fn is_any(&self) -> bool {
        self.policy_type == PolicyType::Any
    }
}

/// Policy combination type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyType {
    /// Any rule must match (OR logic)
    #[default]
    Any,
    /// All rules must match (AND logic)
    All,
    /// No rule must match (NOT logic)
    None,
    /// Single permission requirement
    Permission,
}

impl PolicyType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "all" => Self::All,
            "any" => Self::Any,
            "none" => Self::None,
            "permission" => Self::Permission,
            _ => Self::Any,
        }
    }
}

/// A single policy rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyRule {
    /// Permission requirement
    Permission(String),
    /// Resource ownership check
    Owner {
        resource: String,
        field: String,
        actor_field: Option<String>,
    },
    /// Role requirement
    Role(String),
    /// Custom condition expression
    Condition {
        expression: String,
        message: Option<String>,
    },
    /// Negation of another rule
    Not(Box<PolicyRule>),
    /// Reference to another policy
    PolicyRef(String),
}

impl Default for PolicyRule {
    fn default() -> Self {
        Self::Permission(String::new())
    }
}

impl PolicyRule {
    /// Create a permission rule
    pub fn permission(perm: impl Into<String>) -> Self {
        Self::Permission(perm.into())
    }

    /// Create an owner rule
    pub fn owner(resource: impl Into<String>, field: impl Into<String>) -> Self {
        Self::Owner {
            resource: resource.into(),
            field: field.into(),
            actor_field: None,
        }
    }

    /// Create a role rule
    pub fn role(role: impl Into<String>) -> Self {
        Self::Role(role.into())
    }

    /// Create a condition rule
    pub fn condition(expr: impl Into<String>) -> Self {
        Self::Condition {
            expression: expr.into(),
            message: None,
        }
    }
}

// =============================================================================
// RESOURCE POLICIES
// =============================================================================

/// Policy mappings for a specific resource
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourcePolicy {
    /// Read access rules
    pub read: Vec<ResourcePolicyRule>,
    /// Create access rules
    pub create: Vec<ResourcePolicyRule>,
    /// Update access rules
    pub update: Vec<ResourcePolicyRule>,
    /// Delete access rules
    pub delete: Vec<ResourcePolicyRule>,
    /// Custom action rules (action_name -> rules)
    pub custom: IndexMap<String, Vec<ResourcePolicyRule>>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl ResourcePolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get rules for an action
    pub fn rules_for_action(&self, action: &str) -> Option<&Vec<ResourcePolicyRule>> {
        match action {
            "read" => Some(&self.read),
            "create" => Some(&self.create),
            "update" => Some(&self.update),
            "delete" => Some(&self.delete),
            _ => self.custom.get(action),
        }
    }
}

/// A rule in a resource policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourcePolicyRule {
    /// Reference to a policy
    Policy(String),
    /// Permission requirement
    Permission(String),
    /// Owner check (field name to check for ownership)
    Owner(String),
    /// Custom condition
    Condition {
        expression: String,
        message: Option<String>,
    },
    /// All rules must match (AND)
    All(Vec<ResourcePolicyRule>),
    /// Any rule must match (OR)
    Any(Vec<ResourcePolicyRule>),
}

impl Default for ResourcePolicyRule {
    fn default() -> Self {
        Self::Permission(String::new())
    }
}

// =============================================================================
// ATTRIBUTE-BASED ACCESS CONTROL (ABAC)
// =============================================================================

/// ABAC attribute definitions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbacAttributes {
    /// Subject (user/actor) attributes
    pub subject: Vec<String>,
    /// Resource attributes
    pub resource: Vec<String>,
    /// Environment attributes
    pub environment: Vec<String>,
}

impl AbacAttributes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if an attribute is defined for subjects
    pub fn has_subject_attr(&self, attr: &str) -> bool {
        self.subject.iter().any(|a| a == attr)
    }

    /// Check if an attribute is defined for resources
    pub fn has_resource_attr(&self, attr: &str) -> bool {
        self.resource.iter().any(|a| a == attr)
    }

    /// Check if an attribute is defined for environment
    pub fn has_environment_attr(&self, attr: &str) -> bool {
        self.environment.iter().any(|a| a == attr)
    }
}

/// An ABAC policy rule
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbacPolicy {
    /// Policy name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Condition expression
    pub condition: String,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl AbacPolicy {
    pub fn new(name: impl Into<String>, condition: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            condition: condition.into(),
            ..Default::default()
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_has_permission() {
        let role = RoleDefinition {
            name: "admin".to_string(),
            permissions: vec!["users.*".to_string(), "roles.read".to_string()],
            ..Default::default()
        };

        assert!(role.has_permission("users.read"));
        assert!(role.has_permission("users.create"));
        assert!(role.has_permission("users.delete"));
        assert!(role.has_permission("roles.read"));
        assert!(!role.has_permission("roles.create"));
    }

    #[test]
    fn test_role_wildcard_permission() {
        let superadmin = RoleDefinition {
            name: "super_admin".to_string(),
            permissions: vec!["*".to_string()],
            ..Default::default()
        };

        assert!(superadmin.has_permission("users.read"));
        assert!(superadmin.has_permission("anything.at.all"));
    }

    #[test]
    fn test_role_hierarchy() {
        let admin = RoleDefinition {
            name: "admin".to_string(),
            level: Some(80),
            ..Default::default()
        };

        let user = RoleDefinition {
            name: "user".to_string(),
            level: Some(10),
            ..Default::default()
        };

        assert!(admin.is_higher_than(&user));
        assert!(!user.is_higher_than(&admin));
    }

    #[test]
    fn test_policy_types() {
        assert_eq!(PolicyType::from_str("all"), PolicyType::All);
        assert_eq!(PolicyType::from_str("any"), PolicyType::Any);
        assert_eq!(PolicyType::from_str("none"), PolicyType::None);
        assert_eq!(PolicyType::from_str("unknown"), PolicyType::Any);
    }

    #[test]
    fn test_authorization_config() {
        let mut config = AuthorizationConfig::new();
        config.permissions.insert("users".to_string(), vec!["read".to_string(), "create".to_string()]);
        config.roles.push(RoleDefinition::new("admin"));

        assert!(config.has_role("admin"));
        assert!(!config.has_role("guest"));
        assert!(config.has_permission("users", "read"));
        assert!(!config.has_permission("users", "delete"));
    }
}
