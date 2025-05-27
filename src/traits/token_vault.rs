use ink::primitives::AccountId;

use crate::{error::Result, types::Token};

pub trait TokenVault {
    /// deposit token to account
    fn deposit(&mut self, acct_id: AccountId, token: Token, amt: u128);

    /// withdraw token from account
    fn withdraw(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()>;

    /// lock account balance for order
    fn lock(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()>;

    /// unlock balance
    fn unlock(&mut self, acct_id: AccountId, token: Token, amt: u128) -> Result<()>;

    /// transfer locked balance to fill order
    fn transfer_locked(
        &mut self,
        from: AccountId,
        to: AccountId,
        token: Token,
        amt: u128,
    ) -> Result<()>;
}
