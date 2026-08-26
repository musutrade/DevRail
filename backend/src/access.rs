//! Authenticated actor identity and row-level data-scope contract.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataScope {
    All,
    Organization,
    DepartmentAndChildren,
    Department,
    SelfOnly,
}

impl DataScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Organization => "organization",
            Self::DepartmentAndChildren => "department_and_children",
            Self::Department => "department",
            Self::SelfOnly => "self",
        }
    }

    pub fn from_database(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "organization" => Some(Self::Organization),
            "department_and_children" => Some(Self::DepartmentAndChildren),
            "department" => Some(Self::Department),
            "self" => Some(Self::SelfOnly),
            _ => None,
        }
    }

    pub const fn can_grant(self, requested: Self) -> bool {
        match self {
            Self::All => true,
            Self::Organization => !matches!(requested, Self::All),
            Self::DepartmentAndChildren => matches!(
                requested,
                Self::DepartmentAndChildren | Self::Department | Self::SelfOnly
            ),
            Self::Department => matches!(requested, Self::Department | Self::SelfOnly),
            Self::SelfOnly => matches!(requested, Self::SelfOnly),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActorContext {
    pub actor_type: ActorType,
    pub user_id: i64,
    pub session_id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub data_scope: DataScope,
    pub permission_codes: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorType {
    User,
    System,
}

impl ActorType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
        }
    }
}

impl ActorContext {
    pub fn has_permission(&self, code: &str) -> bool {
        self.permission_codes.contains(code)
    }

    pub const fn can_create_peer(&self) -> bool {
        !matches!(self.data_scope, DataScope::SelfOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_scope_database_codes_are_explicit() {
        for scope in [
            DataScope::All,
            DataScope::Organization,
            DataScope::DepartmentAndChildren,
            DataScope::Department,
            DataScope::SelfOnly,
        ] {
            assert_eq!(DataScope::from_database(scope.as_str()), Some(scope));
        }
        assert_eq!(DataScope::from_database("unknown"), None);
    }

    #[test]
    fn data_scope_grants_cannot_exceed_actor_scope() {
        assert!(DataScope::Organization.can_grant(DataScope::Department));
        assert!(!DataScope::Organization.can_grant(DataScope::All));
        assert!(!DataScope::SelfOnly.can_grant(DataScope::Department));
    }
}
