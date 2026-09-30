#![cfg(test)]

//! Golden tests pinning every ContractError discriminant value.
//!
//! **IMPORTANT:** These discriminant values are part of the public ABI that off-chain
//! services and bindings depend on. Never change a discriminant without:
//! 1. Adding a CHANGELOG entry documenting the change and migration path
//! 2. Coordinating with all clients that depend on these error codes
//! 3. Understanding that changing a code is a breaking change for integrators

#[cfg(test)]
mod tests {
    use crate::ContractError;

    #[test]
    fn test_error_codes_are_stable() {
        // These assertions pin every error discriminant. If any discriminant changes,
        // the test will fail, forcing explicit documentation in the CHANGELOG.
        assert_eq!(ContractError::Unauthorized as u32, 1);
        assert_eq!(ContractError::AlertNotFound as u32, 2);
        assert_eq!(ContractError::AlreadyInitialized as u32, 3);
        assert_eq!(ContractError::NotInitialized as u32, 4);
        assert_eq!(ContractError::NotAWatcher as u32, 5);
        assert_eq!(ContractError::InvalidWebhookHash as u32, 6);
        assert_eq!(ContractError::LabelTooLong as u32, 7);
        assert_eq!(ContractError::TooManyRules as u32, 8);
        assert_eq!(ContractError::InvalidRuleDescriptor as u32, 9);
        assert_eq!(ContractError::OwnerAlertLimitExceeded as u32, 10);
        assert_eq!(ContractError::DuplicateAlertId as u32, 11);
        assert_eq!(ContractError::NoPendingWebhook as u32, 12);
        assert_eq!(ContractError::GlobalAlertLimitExceeded as u32, 13);
        assert_eq!(ContractError::ContractAlertLimitExceeded as u32, 14);
        assert_eq!(ContractError::DuplicateRule as u32, 15);
        assert_eq!(ContractError::NoPendingTransfer as u32, 16);
        assert_eq!(ContractError::InvalidWatcherRegistry as u32, 17);
        assert_eq!(ContractError::Paused as u32, 18);
        assert_eq!(ContractError::TransferExpired as u32, 19);
        assert_eq!(ContractError::InvalidTransferRecipient as u32, 20);
        assert_eq!(ContractError::AlertSuspended as u32, 21);
        assert_eq!(ContractError::EmptyLabel as u32, 22);
        assert_eq!(ContractError::NoopWebhookRotation as u32, 23);
    }
}
