#![cfg_attr(not(feature = "std"), no_std, no_main)]

mod error;
mod storage;
mod traits;
mod types;

#[ink::contract]
pub mod minidex {

    use super::*;
    use erc20::Erc20Ref;
    use error::{Error, Result};
    use ink::env::call::FromAccountId;
    use storage::{BTreeOrderBook, Vault};
    use traits::{order_book::OrderBook, token_vault::TokenVault};
    use types::{EventFilled, Side, Token};

    #[allow(clippy::new_without_default)]
    #[ink(storage)]
    pub struct MiniDex {
        owner: AccountId,
        order_book: BTreeOrderBook,
        vault: Vault,
        base_token_contract: Erc20Ref,
        quote_token_contract: Erc20Ref,
    }

    #[ink(event)]
    pub struct NewOrder {
        #[ink(topic)]
        pub(crate) order_id: u64,
        pub(crate) price: u128,
        pub(crate) qty: u128,
    }

    #[ink(event)]
    pub struct OrderCancelled {
        #[ink(topic)]
        pub(crate) order_id: u64,
    }

    #[ink(event)]
    pub struct OrderFilled {
        #[ink(topic)]
        pub(crate) order_id: u64,
        pub(crate) side: Side,
        pub(crate) filled_price: u128,
        pub(crate) filled_qty: u128,
    }

    impl From<EventFilled> for OrderFilled {
        fn from(e: EventFilled) -> Self {
            Self {
                order_id: e.order_id,
                side: e.side,
                filled_price: e.filled_price,
                filled_qty: e.filled_qty,
            }
        }
    }

    #[ink(event)]
    pub struct Deposit {
        #[ink(topic)]
        pub(crate) account: AccountId,
        #[ink(topic)]
        pub(crate) token: Token,
        pub(crate) amount: u128,
    }

    #[ink(event)]
    pub struct Withdraw {
        #[ink(topic)]
        pub(crate) account: AccountId,
        #[ink(topic)]
        pub(crate) token: Token,
        pub(crate) amount: u128,
    }

    impl MiniDex {
        #[ink(constructor)]
        pub fn new(base_contract_addr: AccountId, quote_contract_addr: AccountId) -> Self {
            let owner = Self::env().caller();
            let base = Erc20Ref::from_account_id(base_contract_addr);
            let quote = Erc20Ref::from_account_id(quote_contract_addr);
            Self {
                owner,
                order_book: BTreeOrderBook::new(),
                vault: Default::default(),
                base_token_contract: base,
                quote_token_contract: quote,
            }
        }

        fn get_erc20(&mut self, token: Token) -> &mut Erc20Ref {
            match token {
                Token::Base => &mut self.base_token_contract,
                Token::Quote => &mut self.quote_token_contract,
            }
        }

        #[ink(message)]
        pub fn deposit(&mut self, token: Token, amount: u128) -> Result<()> {
            if amount == 0 {
                return Err(Error::InvalidQuantity(
                    "Deposit amount cannot be zero".into(),
                ));
            }

            let caller = self.env().caller();
            let contract = self.env().account_id();
            // check if user has approved enough tokens
            let allowance = self.get_erc20(token).allowance(caller, contract);
            if allowance < amount {
                return Err(Error::InsufficientAllowance(token));
            }
            // update vault balance
            self.vault.deposit(caller, token, amount);
            // transfer tokens from user to contract
            self.get_erc20(token)
                .transfer_from(caller, contract, amount)
                .map_err(|_| Error::InsufficientToken(token))?;

            self.env().emit_event(Deposit {
                account: caller,
                token,
                amount,
            });

            Ok(())
        }

        #[ink(message)]
        pub fn withdraw(&mut self, token: Token, amount: u128) -> Result<()> {
            if amount == 0 {
                return Err(Error::InvalidQuantity(
                    "Withdrawal amount cannot be zero".into(),
                ));
            }

            let caller = self.env().caller();
            // check and update vault balance
            self.vault.withdraw(caller, token, amount)?;
            // transfer tokens from contract to user
            self.get_erc20(token)
                .transfer(caller, amount)
                .map_err(|_| Error::InsufficientToken(token))?;

            self.env().emit_event(Withdraw {
                account: caller,
                token,
                amount,
            });

            Ok(())
        }

        #[ink(message)]
        pub fn balance_of(&self, token: Token) -> u128 {
            self.vault.get_balance(self.env().caller(), token)
        }

        #[ink(message)]
        pub fn locked_of(&self, token: Token) -> u128 {
            self.vault.get_locked(self.env().caller(), token)
        }

        #[ink(message)]
        pub fn place_limit_order(
            &mut self,
            pair: (Token, Token),
            side: Side,
            price: u128,
            qty: u128,
        ) -> Result<u64> {
            // sanity check
            if pair != (Token::Base, Token::Quote) {
                return Err(Error::InvalidOrder("Order dex pair not supported".into()));
            }
            if price == 0 {
                return Err(Error::InvalidPrice("Order price cannot be zero".into()));
            }
            if qty == 0 {
                return Err(Error::InvalidQuantity(
                    "Order quantity cannot be zero".into(),
                ));
            }

            let caller = self.env().caller();
            let now = self.env().block_timestamp();
            let mut order = self
                .order_book
                .make_new_order(caller, pair, side, price, qty, now);

            // emit
            let order_id = order.id;
            self.env().emit_event(NewOrder {
                order_id,
                price,
                qty,
            });

            // lock & try match
            let (base, quote) = pair;
            let (res, evts) = match side {
                Side::Buy => {
                    let required = price.checked_mul(qty).unwrap();
                    self.vault.lock(caller, quote, required)?;
                    order.locked = required;

                    // assert ok: transfer lock always success
                    self.order_book
                        .match_sell_orders(order, &mut self.vault)
                        .unwrap()
                }
                Side::Sell => {
                    self.vault.lock(caller, base, qty)?;
                    order.locked = qty;

                    // assert ok: transfer lock always success
                    self.order_book
                        .match_buy_orders(order, &mut self.vault)
                        .unwrap()
                }
            };

            for e in evts {
                self.env().emit_event(OrderFilled::from(e));
            }

            if let Some(order) = res {
                self.order_book.insert_new_order(order);
            }

            Ok(order_id)
        }

        #[ink(message)]
        pub fn cancel_order(&mut self, order_id: u64) -> Result<()> {
            let caller = self.env().caller();
            self.order_book
                .cancel_order(caller, order_id, &mut self.vault)?;

            self.env().emit_event(OrderCancelled { order_id });

            Ok(())
        }
    }
}

#[cfg(all(test, feature = "e2e-tests"))]
mod e2e_tests;
