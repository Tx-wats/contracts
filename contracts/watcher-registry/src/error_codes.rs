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
        assert_eq!(ContractError::AlreadyInitialized as u32, 1);
        assert_eq!(ContractError::Unauthorized as u32, 2);
        assert_eq!(ContractError::NotInitialized as u32, 3);
        assert_eq!(ContractError::LastAdmin as u32, 4);
        assert_eq!(ContractError::WatcherNotFound as u32, 5);
        assert_eq!(ContractError::NoPendingTransfer as u32, 6);
        assert_eq!(ContractError::MaxWatchersReached as u32, 7);
        assert_eq!(ContractError::MaxAdminsReached as u32, 8);
        assert_eq!(ContractError::ActionAlreadyPending as u32, 9);
        assert_eq!(ContractError::NoPendingAction as u32, 10);
    }
}
