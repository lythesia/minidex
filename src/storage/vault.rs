use ink::{primitives::AccountId, storage::Mapping};

use crate::{
    error::{Error, Result},
    traits::token_vault::TokenVault,
    types::Token,
};

#[ink::scale_derive(Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
#[derive(Debug, Clone, Default)]
pub(crate) struct Account {
    balance: u128,
    locked: u128,
}

#[ink::storage_item]
#[derive(Default)]
pub struct Vault {
    accounts: Mapping<(AccountId, Token), Account>,
}

impl core::fmt::Debug for Vault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vault").finish()
    }
}

impl Vault {
    #[inline]
    fn get_or_default(&self, acct_id: AccountId, token: Token) -> Account {
        self.accounts.get((acct_id, token)).unwrap_or_default()
    }

    pub(crate) fn get_balance(&self, acct_id: AccountId, token: Token) -> u128 {
        self.get_or_default(acct_id, token).balance
    }

    pub(crate) fn get_locked(&self, acct_id: AccountId, token: Token) -> u128 {
        self.get_or_default(acct_id, token).locked
    }
}

impl TokenVault for Vault {
    fn deposit(&mut self, acct_id: AccountId, token: Token, amt: u128) {
        let mut acct = self.get_or_default(acct_id, token);
        acct.balance = acct.balance.checked_add(amt).unwrap();
        self.accounts.insert((acct_id, token), &acct);
    }

    fn withdraw(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()> {
        let mut acct = self.get_or_default(acct_id, token);
        acct.balance = acct
            .balance
            .checked_sub(amt)
            .ok_or(Error::InsufficientBalance(token))?;
        self.accounts.insert((acct_id, token), &acct);
        Ok(())
    }

    fn lock(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()> {
        let mut acct = self.get_or_default(acct_id, token);
        acct.balance = acct
            .balance
            .checked_sub(amt)
            .ok_or(Error::InsufficientBalance(token))?;
        acct.locked = acct.locked.checked_add(amt).unwrap();
        self.accounts.insert((acct_id, token), &acct);
        Ok(())
    }

    fn unlock(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()> {
        let mut acct = self.get_or_default(acct_id, token);
        acct.locked = acct
            .locked
            .checked_sub(amt)
            .ok_or(Error::InsufficientLockedBalance(token))?;
        acct.balance = acct.balance.checked_add(amt).unwrap();
        self.accounts.insert((acct_id, token), &acct);
        Ok(())
    }

    fn transfer_locked(
        &mut self,
        from: AccountId,
        to: AccountId,
        token: Token,
        amt: u128,
    ) -> Result<()> {
        if from == to {
            return Err(Error::InvalidTransfer(
                "Cannot transfer locked to self".into(),
            ));
        }
        let mut from_acct = self.get_or_default(from, token);
        from_acct.locked = from_acct
            .locked
            .checked_sub(amt)
            .ok_or(Error::InsufficientLockedBalance(token))?;
        self.accounts.insert((from, token), &from_acct);

        let mut to_acct = self.get_or_default(to, token);
        to_acct.balance = to_acct.balance.checked_add(amt).unwrap();
        self.accounts.insert((to, token), &to_acct);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ink::env::test;

    fn setup() -> (AccountId, AccountId) {
        let accounts = test::default_accounts::<ink::env::DefaultEnvironment>();
        // make ink engine happy
        test::set_callee::<ink::env::DefaultEnvironment>(accounts.charlie);
        (accounts.alice, accounts.bob)
    }

    #[test]
    fn test_deposit() {
        let (alice, _) = setup();
        let mut vault = Vault::default();
        let token = Token::Base;

        // Test initial deposit
        vault.deposit(alice, token, 100);
        let account = vault.get_or_default(alice, token);
        assert_eq!(account.balance, 100);
        assert_eq!(account.locked, 0);

        // Test additional deposit
        vault.deposit(alice, token, 50);
        let account = vault.get_or_default(alice, token);
        assert_eq!(account.balance, 150);
    }

    #[test]
    fn test_withdraw() {
        let (alice, _) = setup();
        let mut vault = Vault::default();
        let token = Token::Base;

        // Setup initial balance
        vault.deposit(alice, token, 100);

        // Test successful withdrawal
        assert!(vault.withdraw(alice, token, 50).is_ok());
        let account = vault.get_or_default(alice, token);
        assert_eq!(account.balance, 50);

        // Test withdrawal with insufficient balance
        assert!(matches!(
            vault.withdraw(alice, token, 100),
            Err(Error::InsufficientBalance(_))
        ));
    }

    #[test]
    fn test_lock() {
        let (alice, _) = setup();
        let mut vault = Vault::default();
        let token = Token::Base;

        // Setup initial balance
        vault.deposit(alice, token, 100);

        // Test successful lock
        assert!(vault.lock(alice, token, 50).is_ok());
        let account = vault.get_or_default(alice, token);
        assert_eq!(account.balance, 50);
        assert_eq!(account.locked, 50);

        // Test lock with insufficient balance
        assert!(matches!(
            vault.lock(alice, token, 100),
            Err(Error::InsufficientBalance(_))
        ));
    }

    #[test]
    fn test_unlock() {
        let (alice, _) = setup();
        let mut vault = Vault::default();
        let token = Token::Base;

        // Setup initial balance and locked amount
        vault.deposit(alice, token, 100);
        vault.lock(alice, token, 50).unwrap();

        // Test successful unlock
        assert!(vault.unlock(alice, token, 30).is_ok());
        let account = vault.get_or_default(alice, token);
        assert_eq!(account.balance, 80);
        assert_eq!(account.locked, 20);

        // Test unlock with insufficient locked balance
        assert!(matches!(
            vault.unlock(alice, token, 100),
            Err(Error::InsufficientLockedBalance(_))
        ));
    }

    #[test]
    fn test_transfer_locked() {
        let (alice, bob) = setup();
        let mut vault = Vault::default();
        let token = Token::Base;

        // Setup initial balance and locked amount
        vault.deposit(alice, token, 100);
        vault.lock(alice, token, 50).unwrap();

        // Test successful transfer
        assert!(vault.transfer_locked(alice, bob, token, 30).is_ok());

        let alice_account = vault.get_or_default(alice, token);
        assert_eq!(alice_account.balance, 50);
        assert_eq!(alice_account.locked, 20);

        let bob_account = vault.get_or_default(bob, token);
        assert_eq!(bob_account.balance, 30);
        assert_eq!(bob_account.locked, 0);

        // Test transfer with insufficient locked balance
        assert!(matches!(
            vault.transfer_locked(alice, bob, token, 100),
            Err(Error::InsufficientLockedBalance(_))
        ));

        // Test transfer to self - should fail
        assert!(matches!(
            vault.transfer_locked(alice, alice, token, 10),
            Err(Error::InvalidTransfer(_))
        ));
    }

    #[test]
    fn test_multiple_tokens() {
        let (alice, _) = setup();
        let mut vault = Vault::default();
        let token1 = Token::Base;
        let token2 = Token::Quote;

        // Test operations with different tokens
        vault.deposit(alice, token1, 100);
        vault.deposit(alice, token2, 200);

        let account1 = vault.get_or_default(alice, token1);
        assert_eq!(account1.balance, 100);

        let account2 = vault.get_or_default(alice, token2);
        assert_eq!(account2.balance, 200);

        // Test operations on different tokens are independent
        vault.lock(alice, token1, 50).unwrap();
        let account1 = vault.get_or_default(alice, token1);
        assert_eq!(account1.balance, 50);
        assert_eq!(account1.locked, 50);

        let account2 = vault.get_or_default(alice, token2);
        assert_eq!(account2.balance, 200);
        assert_eq!(account2.locked, 0);
    }
}
