#![cfg_attr(not(feature = "std"), no_std, no_main)]

mod error;
mod storage;
mod traits;
mod types;

#[ink::contract]
pub mod minidex {

    use super::*;
    use error::{Error, Result};
    use storage::{BTreeOrderBook, Vault};
    use traits::{order_book::OrderBook, token_vault::TokenVault};
    use types::{EventFilled, Side, Token};

    #[allow(clippy::new_without_default)]
    #[ink(storage)]
    pub struct MiniDex {
        owner: AccountId,
        order_book: BTreeOrderBook,
        vault: Vault,
        // base_token_contract: Erc20Ref,
        // quote_token_contract: Erc20Ref,
    }

    #[ink(event)]
    pub struct NewOrder {
        #[ink(topic)]
        order_id: u64,
        price: u128,
        qty: u128,
    }

    #[ink(event)]
    pub struct OrderCancelled {
        #[ink(topic)]
        order_id: u64,
    }

    #[ink(event)]
    pub struct OrderFilled {
        #[ink(topic)]
        order_id: u64,
        filled_price: u128,
        filled_qty: u128,
    }

    impl From<EventFilled> for OrderFilled {
        fn from(e: EventFilled) -> Self {
            Self {
                order_id: e.order_id,
                filled_price: e.filled_price,
                filled_qty: e.filled_qty,
            }
        }
    }

    #[ink(event)]
    pub struct Deposit {
        #[ink(topic)]
        account: AccountId,
        #[ink(topic)]
        token: Token,
        amount: u128,
    }

    #[ink(event)]
    pub struct Withdraw {
        #[ink(topic)]
        account: AccountId,
        #[ink(topic)]
        token: Token,
        amount: u128,
    }

    impl MiniDex {
        #[ink(constructor)]
        pub fn new() -> Self {
            let owner = Self::env().caller();
            Self {
                owner,
                order_book: BTreeOrderBook::new(),
                vault: Default::default(),
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
            self.vault.deposit(caller, token, amount);

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
            self.vault.withdraw(caller, token, amount)?;

            self.env().emit_event(Withdraw {
                account: caller,
                token,
                amount,
            });

            Ok(())
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
            if pair != (Token::TokenA, Token::TokenB) {
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
