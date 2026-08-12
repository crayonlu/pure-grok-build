use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitlementState {
    Unknown,
    Unrestricted,
    Restricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EntitlementPolicy {
    pub state: EntitlementState,
    pub show_billing: bool,
    pub show_subscription_gate: bool,
    pub watch_subscription: bool,
}

impl Default for EntitlementPolicy {
    fn default() -> Self {
        Self::provider_neutral()
    }
}

impl EntitlementPolicy {
    pub const fn provider_neutral() -> Self {
        Self {
            state: EntitlementState::Unrestricted,
            show_billing: false,
            show_subscription_gate: false,
            watch_subscription: false,
        }
    }

    pub const fn first_party() -> Self {
        Self {
            state: EntitlementState::Unknown,
            show_billing: true,
            show_subscription_gate: true,
            watch_subscription: true,
        }
    }

    pub const fn should_show_gate(self) -> bool {
        self.show_subscription_gate && matches!(self.state, EntitlementState::Restricted)
    }

    pub const fn should_check_subscription(self) -> bool {
        self.watch_subscription
            && matches!(
                self.state,
                EntitlementState::Unknown | EntitlementState::Restricted
            )
    }

    pub const fn should_show_billing(self) -> bool {
        self.show_billing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_neutral_entitlement_hides_billing_and_paywall() {
        let policy = EntitlementPolicy::provider_neutral();

        assert_eq!(policy.state, EntitlementState::Unrestricted);
        assert!(!policy.show_billing);
        assert!(!policy.should_show_gate());
        assert!(!policy.should_check_subscription());
    }

    #[test]
    fn first_party_entitlement_can_watch_restricted_access() {
        let policy = EntitlementPolicy::first_party();

        assert!(policy.show_billing);
        assert!(policy.should_check_subscription());
        assert!(!policy.should_show_gate());
    }
}
