use ink::primitives::AccountId;

use crate::{error::Result, types::Token};

/// A trait for managing token balances and locked amounts in a DEX vault.
///
/// This trait provides the core functionality for handling token deposits, withdrawals,
/// and order-related balance operations in a decentralized exchange. It manages both
/// available balances and locked amounts for pending orders.
pub trait TokenVault {
    /// Deposits tokens into an account's balance.
    ///
    /// # Arguments
    /// * `acct_id` - The account ID to deposit tokens to
    /// * `token` - The type of token to deposit (Base or Quote)
    /// * `amt` - The amount of tokens to deposit
    fn deposit(&mut self, acct_id: AccountId, token: Token, amt: u128);

    /// Withdraws tokens from an account's balance.
    ///
    /// # Arguments
    /// * `acct_id` - The account ID to withdraw tokens from
    /// * `token` - The type of token to withdraw (Base or Quote)
    /// * `amt` - The amount of tokens to withdraw
    ///
    /// # Returns
    /// * `Result<()>` - Ok if withdrawal successful, Error if insufficient balance
    fn withdraw(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()>;

    /// Locks tokens from an account's balance for a pending order.
    ///
    /// # Arguments
    /// * `acct_id` - The account ID to lock tokens from
    /// * `token` - The type of token to lock (Base or Quote)
    /// * `amt` - The amount of tokens to lock
    ///
    /// # Returns
    /// * `Result<()>` - Ok if lock successful, Error if insufficient balance
    fn lock(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()>;

    /// Unlocks tokens from an account's locked balance.
    ///
    /// # Arguments
    /// * `acct_id` - The account ID to unlock tokens for
    /// * `token` - The type of token to unlock (Base or Quote)
    /// * `amt` - The amount of tokens to unlock
    ///
    /// # Returns
    /// * `Result<()>` - Ok if unlock successful, Error if insufficient locked balance
    fn unlock(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()>;

    /// Transfers locked tokens between accounts to fill an order.
    ///
    /// # Arguments
    /// * `from` - The account ID to transfer tokens from
    /// * `to` - The account ID to transfer tokens to
    /// * `token` - The type of token to transfer (Base or Quote)
    /// * `amt` - The amount of tokens to transfer
    ///
    /// # Returns
    /// * `Result<()>` - Ok if transfer successful, Error if transfer fails
    fn transfer_locked(
        &mut self,
        from: AccountId,
        to: AccountId,
        token: Token,
        amt: u128,
    ) -> Result<()>;
}
