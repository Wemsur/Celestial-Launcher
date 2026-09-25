//! The permission vocabulary a plugin may ask for.
//!
//! Every permission is a `kind` plus an optional `scope`, written as
//! `kind:scope` in a manifest (`network:example.com`, `slot:sidebar.bottom`).
//! Nothing is granted implicitly: see [`PermissionRisk`] for which kinds a user
//! has to approve by hand.

use serde::{Deserialize, Serialize};

/// How much damage a permission can do if a plugin misuses it.
///
/// Low-risk kinds are granted as soon as the plugin is installed, because the
/// worst case is an ugly launcher. High-risk kinds are only ever turned on by
/// the user saying yes to that exact permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRisk {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    /// Read and write this plugin's own data directory, and nothing else.
    Storage,
    /// Inject CSS. Unscoped because a stylesheet is global by nature — there is
    /// no such thing as a stylesheet that is "near" one element.
    Style,
    /// Put UI into one of the launcher's named slots.
    Slot,
    /// Register a page of its own with the launcher's router.
    Route,
    /// Reach a host over the network.
    Network,
    /// Touch the DOM inside one of the launcher's named regions.
    Region,
    /// Subscribe to a launcher event.
    Event,
    /// Call one of the launcher's exposed host APIs.
    HostApi,
    /// Download and run a native sidecar binary, and talk to it over
    /// localhost. The single biggest capability a plugin can hold — it runs
    /// native code outside the webview sandbox.
    Sidecar,
    /// Announce a Minecraft LAN game over UDP multicast on the local network.
    Lan,
}

impl PermissionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Style => "style",
            Self::Slot => "slot",
            Self::Route => "route",
            Self::Network => "network",
            Self::Region => "region",
            Self::Event => "event",
            Self::HostApi => "hostapi",
            Self::Sidecar => "sidecar",
            Self::Lan => "lan",
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        Some(match value {
            "storage" => Self::Storage,
            "style" => Self::Style,
            "slot" => Self::Slot,
            "route" => Self::Route,
            "network" => Self::Network,
            "region" => Self::Region,
            "event" => Self::Event,
            "hostapi" => Self::HostApi,
            "sidecar" => Self::Sidecar,
            "lan" => Self::Lan,
            _ => return None,
        })
    }

    /// Whether `kind:scope` requires a scope. The opposite kinds are written
    /// bare, so `storage:foo` is a manifest mistake worth reporting rather than
    /// quietly accepting a scope that will never be checked against anything.
    fn requires_scope(self) -> bool {
        !matches!(
            self,
            Self::Storage | Self::Style | Self::Route | Self::Sidecar | Self::Lan
        )
    }

    pub fn risk(self) -> PermissionRisk {
        match self {
            Self::Storage
            | Self::Style
            | Self::Slot
            | Self::Route
            | Self::Event => PermissionRisk::Low,
            Self::Network
            | Self::Region
            | Self::HostApi
            | Self::Sidecar
            | Self::Lan => PermissionRisk::High,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginPermission {
    pub kind: PermissionKind,
    pub scope: Option<String>,
}

impl PluginPermission {
    /// Parse `kind` or `kind:scope`.
    ///
    /// Returns a human-readable reason on failure: the caller is either a
    /// manifest author who needs to know what to fix, or the plugin page
    /// showing why a plugin will not load.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("permission must not be empty".to_string());
        }

        let (kind_key, scope) = match raw.split_once(':') {
            Some((kind, scope)) => (kind, Some(scope)),
            None => (raw, None),
        };

        let kind = PermissionKind::from_key(kind_key).ok_or_else(|| {
            format!("unknown permission kind '{kind_key}'")
        })?;

        let scope = match (kind.requires_scope(), scope) {
            (true, None) => {
                return Err(format!(
                    "'{}' needs a scope, e.g. '{}:example'",
                    kind.as_str(),
                    kind.as_str()
                ));
            }
            (false, Some(_)) => {
                return Err(format!(
                    "'{}' does not take a scope",
                    kind.as_str()
                ));
            }
            (true, Some(scope)) => Some(validate_scope(kind, scope)?),
            (false, None) => None,
        };

        Ok(Self { kind, scope })
    }

    pub fn risk(&self) -> PermissionRisk {
        self.kind.risk()
    }
}

impl std::fmt::Display for PluginPermission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.scope {
            Some(scope) => {
                write!(formatter, "{}:{}", self.kind.as_str(), scope)
            }
            None => formatter.write_str(self.kind.as_str()),
        }
    }
}

fn validate_scope(kind: PermissionKind, scope: &str) -> Result<String, String> {
    if scope.is_empty() {
        return Err(format!("'{}' permission has an empty scope", kind.as_str()));
    }
    if scope.len() > 128 {
        return Err(format!(
            "'{}' permission scope is too long (max 128 characters)",
            kind.as_str()
        ));
    }
    // Whitespace and control characters would let a scope be written one way in
    // the manifest and read another way by whatever matches against it.
    if scope.chars().any(|character| {
        character.is_whitespace() || character.is_control()
    }) {
        return Err(format!(
            "'{}' permission scope must not contain whitespace",
            kind.as_str()
        ));
    }

    if kind == PermissionKind::Network {
        // A host, optionally wildcarded, optionally with a port.
        let host = scope.strip_prefix("*.").unwrap_or(scope);
        let host = host.split(':').next().unwrap_or_default();
        if host.is_empty()
            || !host.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '-' | '_')
            })
        {
            return Err(format!("'{scope}' is not a usable host name"));
        }
    }

    Ok(scope.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_kinds_parse() {
        for (raw, kind) in [
            ("storage", PermissionKind::Storage),
            ("style", PermissionKind::Style),
            ("route", PermissionKind::Route),
            ("sidecar", PermissionKind::Sidecar),
            ("lan", PermissionKind::Lan),
        ] {
            assert_eq!(
                PluginPermission::parse(raw),
                Ok(PluginPermission { kind, scope: None })
            );
        }
    }

    #[test]
    fn scoped_kinds_round_trip() {
        for raw in [
            "slot:sidebar.bottom",
            "region:topbar",
            "network:example.com",
            "network:*.example.com",
            "network:127.0.0.1:8080",
            "event:instance_launched",
            "hostapi:instance.list",
        ] {
            let parsed = PluginPermission::parse(raw).expect(raw);
            assert_eq!(parsed.to_string(), raw);
        }
    }

    #[test]
    fn scoped_kind_without_a_scope_is_rejected() {
        assert!(PluginPermission::parse("slot").is_err());
        assert!(PluginPermission::parse("network").is_err());
    }

    #[test]
    fn bare_kind_with_a_scope_is_rejected() {
        assert!(PluginPermission::parse("storage:x").is_err());
        assert!(PluginPermission::parse("style:global").is_err());
    }

    #[test]
    fn unknown_kinds_are_rejected() {
        assert!(PluginPermission::parse("filesystem").is_err());
        assert!(PluginPermission::parse("").is_err());
    }

    #[test]
    fn bad_hosts_are_rejected() {
        assert!(PluginPermission::parse("network:").is_err());
        assert!(PluginPermission::parse("network:exa mple.com").is_err());
        assert!(PluginPermission::parse("network:*.example.com/").is_err());
    }

    #[test]
    fn only_the_documented_kinds_are_high_risk() {
        assert_eq!(
            PluginPermission::parse("style").unwrap().risk(),
            PermissionRisk::Low
        );
        assert_eq!(
            PluginPermission::parse("region:topbar").unwrap().risk(),
            PermissionRisk::High
        );
        assert_eq!(
            PluginPermission::parse("network:example.com").unwrap().risk(),
            PermissionRisk::High
        );
        assert_eq!(
            PluginPermission::parse("sidecar").unwrap().risk(),
            PermissionRisk::High
        );
        assert_eq!(
            PluginPermission::parse("lan").unwrap().risk(),
            PermissionRisk::High
        );
    }

    #[test]
    fn bare_only_kinds_reject_a_scope() {
        assert!(PluginPermission::parse("sidecar:terracotta").is_err());
        assert!(PluginPermission::parse("lan:4445").is_err());
    }
}
