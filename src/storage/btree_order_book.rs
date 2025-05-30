use ink::{
    prelude::{collections::BTreeMap, vec::Vec},
    primitives::AccountId,
    storage::Mapping,
};

use crate::{
    error::{Error, Result},
    traits::{order_book::OrderBook, token_vault::TokenVault},
    types::{EventFilled, Order, Side, Token},
};

type StorageBTreeMap = BTreeMap<(u128, u64, u64), u64>;

#[ink::storage_item]
#[derive(Default)]
pub struct BTreeOrderBook {
    // all orders
    orders: Mapping<u64, Order>,

    // sell orders: (price, timestamp, order_id) -> order_id
    sell_orders: StorageBTreeMap,

    // buy orders: (Reverse(price), timestamp, order_id) -> order_id
    buy_orders: StorageBTreeMap,

    // order id generator
    next_order_id: u64,

    // shortcut matching condition
    min_sell_price: u128,
    max_buy_price: u128,
}

impl BTreeOrderBook {
    pub fn new() -> Self {
        Self {
            min_sell_price: u128::MAX,
            max_buy_price: u128::MIN,
            ..Default::default()
        }
    }
}

impl core::fmt::Debug for BTreeOrderBook {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BTreeOrderBook").finish()
    }
}

impl OrderBook for BTreeOrderBook {
    fn make_new_order(
        &self,
        acct_id: AccountId,
        pair: (Token, Token),
        side: Side,
        price: u128,
        qty: u128,
        now: u64,
    ) -> Order {
        let order_id = self.next_order_id;
        Order {
            id: order_id,
            pair,
            owner: acct_id,
            side,
            price,
            qty,
            timestamp: now,
            locked: 0,
        }
    }

    fn insert_new_order(&mut self, order: Order) {
        self.orders.insert(order.id, &order);
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.next_order_id += 1;
        }

        match order.side {
            Side::Buy => {
                #[allow(clippy::arithmetic_side_effects)]
                let key = (u128::MAX - order.price, order.timestamp, order.id);
                self.buy_orders.insert(key, order.id);
                self.max_buy_price = self.max_buy_price.max(order.price);
            }
            Side::Sell => {
                #[allow(clippy::arithmetic_side_effects)]
                let key = (order.price, order.timestamp, order.id);
                self.sell_orders.insert(key, order.id);
                self.min_sell_price = self.min_sell_price.min(order.price);
            }
        }
    }

    fn match_sell_orders<V: TokenVault>(
        &mut self,
        mut buy_order: Order,
        vault: &mut V,
    ) -> Result<(Option<Order>, Vec<EventFilled>)> {
        let mut evts = Vec::new();
        if buy_order.price < self.min_sell_price {
            return Ok((Some(buy_order), evts));
        }

        let (base, quote) = buy_order.pair;
        while let Some(entry) = self.sell_orders.first_entry() {
            // 1. lowest sell order
            let (sell_price, ..) = entry.key();
            let order_id = *entry.get();
            let mut sell_order = self
                .orders
                .get(order_id)
                .ok_or(Error::OrderNotFound(order_id))?;

            // 2. if can match
            if sell_price > &buy_order.price {
                break;
            }
            // 2.1 finalize sell_order
            // assert sell_price <= buy_price
            let deal_price = sell_order.price;
            if sell_order.qty <= buy_order.qty {
                // quote transfer
                let quote_amt = deal_price.checked_mul(sell_order.qty).unwrap();
                // checked
                #[allow(clippy::arithmetic_side_effects)]
                {
                    buy_order.qty -= sell_order.qty;
                    buy_order.locked -= quote_amt;
                }
                vault.transfer_locked(buy_order.owner, sell_order.owner, quote, quote_amt)?;

                // base transfer
                vault.transfer_locked(sell_order.owner, buy_order.owner, base, sell_order.qty)?;

                // clear sell order
                entry.remove_entry();
                self.orders.remove(order_id);

                // emit
                evts.push(EventFilled::new(sell_order.id, deal_price, sell_order.qty));
                evts.push(EventFilled::new(buy_order.id, deal_price, sell_order.qty));
            }
            // 2.2 partial fill
            else {
                // quote transfer
                let quote_amt = deal_price.checked_mul(buy_order.qty).unwrap();
                // checked
                #[allow(clippy::arithmetic_side_effects)]
                {
                    buy_order.locked -= quote_amt;
                    sell_order.qty -= buy_order.qty;
                }
                vault.transfer_locked(buy_order.owner, sell_order.owner, quote, quote_amt)?;

                // base transfer
                vault.transfer_locked(sell_order.owner, buy_order.owner, base, buy_order.qty)?;

                // update sell order
                self.orders.insert(order_id, &sell_order);

                // emit
                evts.push(EventFilled::new(sell_order.id, deal_price, buy_order.qty));
                evts.push(EventFilled::new(buy_order.id, deal_price, buy_order.qty));

                buy_order.qty = 0;
                break;
            }
        }

        if buy_order.qty > 0 {
            Ok((Some(buy_order), evts))
        } else {
            // unlock remaining
            if buy_order.locked > 0 {
                vault.unlock(buy_order.owner, quote, buy_order.locked)?;
            }
            Ok((None, evts))
        }
    }

    fn match_buy_orders<V: TokenVault>(
        &mut self,
        mut sell_order: Order,
        vault: &mut V,
    ) -> Result<(Option<Order>, Vec<EventFilled>)> {
        let mut evts = Vec::new();
        if sell_order.price > self.max_buy_price {
            return Ok((Some(sell_order), evts));
        }

        let (base, quote) = sell_order.pair;
        while let Some(entry) = self.buy_orders.first_entry() {
            // 1. highest buy order
            let (buy_price, ..) = entry.key();
            #[allow(clippy::arithmetic_side_effects)]
            let buy_price = &(u128::MAX - buy_price);
            let order_id = *entry.get();
            let mut buy_order = self
                .orders
                .get(order_id)
                .ok_or(Error::OrderNotFound(order_id))?;

            // 2. if can match
            if buy_price < &sell_order.price {
                break;
            }
            // 2.1 finalize buy_order
            // assert sell_price <= buy_price
            let deal_price = sell_order.price;
            if buy_order.qty <= sell_order.qty {
                // quote transfer
                let quote_amt = deal_price.checked_mul(buy_order.qty).unwrap();
                // checked
                #[allow(clippy::arithmetic_side_effects)]
                {
                    sell_order.qty -= buy_order.qty;
                    buy_order.locked -= quote_amt;
                }
                vault.transfer_locked(buy_order.owner, sell_order.owner, quote, quote_amt)?;

                // base transfer
                vault.transfer_locked(sell_order.owner, buy_order.owner, base, buy_order.qty)?;

                // unlock remaining when complete
                if buy_order.locked > 0 {
                    vault.unlock(buy_order.owner, quote, buy_order.locked)?;
                }
                // clear buy order
                entry.remove_entry();
                self.orders.remove(order_id);

                // emit
                evts.push(EventFilled::new(buy_order.id, deal_price, buy_order.qty));
                evts.push(EventFilled::new(sell_order.id, deal_price, buy_order.qty));
            }
            // 2.2 partial fill
            else {
                // quote transfer
                let quote_amt = deal_price.checked_mul(sell_order.qty).unwrap();
                // checked
                #[allow(clippy::arithmetic_side_effects)]
                {
                    buy_order.locked -= quote_amt;
                    buy_order.qty -= sell_order.qty;
                }
                vault.transfer_locked(buy_order.owner, sell_order.owner, quote, quote_amt)?;

                // base transfer
                vault.transfer_locked(sell_order.owner, buy_order.owner, base, sell_order.qty)?;
                // update buy order
                self.orders.insert(order_id, &buy_order);

                // emit
                evts.push(EventFilled::new(buy_order.id, deal_price, sell_order.qty));
                evts.push(EventFilled::new(sell_order.id, deal_price, sell_order.qty));
                sell_order.qty = 0;
                break;
            }
        }

        if sell_order.qty > 0 {
            Ok((Some(sell_order), evts))
        } else {
            Ok((None, evts))
        }
    }

    fn cancel_order<V: TokenVault>(
        &mut self,
        acct_id: AccountId,
        order_id: u64,
        vault: &mut V,
    ) -> Result<()> {
        let order = self
            .orders
            .get(order_id)
            .ok_or(Error::OrderNotFound(order_id))?;
        if order.owner != acct_id {
            return Err(Error::Unauthorized("Only order owner can cancel".into()));
        }

        let (base, quote) = order.pair;
        match order.side {
            Side::Buy => {
                // unlock unfills
                // assert ok: unlock always success
                if order.locked > 0 {
                    vault.unlock(order.owner, quote, order.locked).unwrap();
                }
                // clear buy order
                #[allow(clippy::arithmetic_side_effects)]
                let key = (u128::MAX - order.price, order.timestamp, order.id);
                self.buy_orders.remove(&key);
                if order.price == self.max_buy_price {
                    self.max_buy_price = self
                        .buy_orders
                        .first_entry()
                        .map(|e| u128::MAX.checked_sub(e.key().0).unwrap())
                        .unwrap_or(0);
                }
            }
            Side::Sell => {
                // unlock unfills
                // assert ok: unlock always success
                vault.unlock(order.owner, base, order.qty).unwrap();
                // clear sell order
                #[allow(clippy::arithmetic_side_effects)]
                let key = (order.price, order.timestamp, order.id);
                self.sell_orders.remove(&key);
                if order.price == self.min_sell_price {
                    self.min_sell_price = self
                        .sell_orders
                        .first_entry()
                        .map(|e| e.key().0)
                        .unwrap_or(u128::MAX);
                }
            }
        }
        self.orders.remove(order_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::vault::Vault;
    use ink::env::test;

    fn setup() -> (BTreeOrderBook, Vault, AccountId, AccountId) {
        let book = BTreeOrderBook::new();
        let mut vault = Vault::default();
        let accounts = test::default_accounts::<ink::env::DefaultEnvironment>();
        let alice = accounts.alice;
        let bob = accounts.bob;

        // make ink engine happy
        test::set_callee::<ink::env::DefaultEnvironment>(accounts.charlie);

        // Setup initial balances
        vault.deposit(alice, Token::Base, 1000);
        vault.deposit(alice, Token::Quote, 1000);
        vault.deposit(bob, Token::Base, 1000);
        vault.deposit(bob, Token::Quote, 1000);

        (book, vault, alice, bob)
    }

    #[test]
    fn test_basic_matching() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 100, now);
        vault.lock(alice, Token::Quote, 1000).unwrap(); // Lock 1000 TokenB
        buy_order.locked = 1000;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 100 TokenA at price 10 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            100,
            now + 1,
        );
        vault.lock(bob, Token::Base, 100).unwrap(); // Lock 100 TokenA
        sell_order.locked = 100;
        book.insert_new_order(sell_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0);
        assert_eq!(vault.get_locked(alice, Token::Quote), 1000);
        assert_eq!(vault.get_balance(bob, Token::Base), 900);
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the orders
        let (remaining_sell, events) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_none()); // Sell order should be fully filled
        assert_eq!(events.len(), 2); // Two fill events

        // Verify the match by checking the events
        let (buy_event, sell_event) = (&events[0], &events[1]);
        assert_eq!(buy_event.order_id, buy_order.id);
        assert_eq!(sell_event.order_id, sell_order.id);
        assert_eq!(buy_event.filled_price, 10);
        assert_eq!(sell_event.filled_price, 10);
        assert_eq!(buy_event.filled_qty, 100);
        assert_eq!(sell_event.filled_qty, 100);

        // Check final balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1100); // Received 100 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0); // Spent 1000 TokenB
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);
        assert_eq!(vault.get_balance(bob, Token::Base), 900); // Spent 100 TokenA
        assert_eq!(vault.get_locked(bob, Token::Base), 0);
        assert_eq!(vault.get_balance(bob, Token::Quote), 2000); // Received 1000 TokenB
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_partial_fill() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 50 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 50, now);
        vault.lock(alice, Token::Quote, 500).unwrap(); // Lock 500 TokenB
        buy_order.locked = 500;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 100 TokenA at price 10 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            100,
            now + 1,
        );
        vault.lock(bob, Token::Base, 100).unwrap(); // Lock 100 TokenA
        sell_order.locked = 100;
        book.insert_new_order(sell_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 500);
        assert_eq!(vault.get_locked(alice, Token::Quote), 500);
        assert_eq!(vault.get_balance(bob, Token::Base), 900); // 1000 - 100 locked
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the orders
        let (remaining_sell, events) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_some()); // Sell order should be partially filled
        assert_eq!(events.len(), 2); // Two fill events

        let remaining_sell = remaining_sell.unwrap();
        assert_eq!(remaining_sell.qty, 50); // 50 TokenA remaining

        // Verify the partial fill by checking the events
        let (buy_event, sell_event) = (&events[0], &events[1]);
        assert_eq!(buy_event.order_id, buy_order.id);
        assert_eq!(sell_event.order_id, sell_order.id);
        assert_eq!(buy_event.filled_price, 10);
        assert_eq!(sell_event.filled_price, 10);
        assert_eq!(buy_event.filled_qty, 50);
        assert_eq!(sell_event.filled_qty, 50);

        // Check final balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1050); // Received 50 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 500); // Spent 500 TokenB
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);
        assert_eq!(vault.get_balance(bob, Token::Base), 900); // Still 900 because 100 was locked initially, 50 transferred, 50 still locked
        assert_eq!(vault.get_locked(bob, Token::Base), 50); // 50 TokenA still locked
        assert_eq!(vault.get_balance(bob, Token::Quote), 1500); // Received 500 TokenB
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_price_mismatch() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 8 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 8, 100, now);
        vault.lock(alice, Token::Quote, 800).unwrap(); // Lock 800 TokenB
        buy_order.locked = 800;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 100 TokenA at price 10 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            100,
            now + 1,
        );
        vault.lock(bob, Token::Base, 100).unwrap(); // Lock 100 TokenA
        sell_order.locked = 100;
        book.insert_new_order(sell_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 200);
        assert_eq!(vault.get_locked(alice, Token::Quote), 800);
        assert_eq!(vault.get_balance(bob, Token::Base), 900);
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the orders
        let (remaining_sell, events) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_some()); // Sell order should not be filled
        assert!(events.is_empty()); // No fill events

        // Verify the sell order is unchanged
        let remaining_sell = remaining_sell.unwrap();
        assert_eq!(remaining_sell.qty, 100);
        assert_eq!(remaining_sell.price, 10);

        // Check final balances - should be unchanged
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 200);
        assert_eq!(vault.get_locked(alice, Token::Quote), 800);
        assert_eq!(vault.get_balance(bob, Token::Base), 900);
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_buy_matches_multiple_sells() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 100, now);
        vault.lock(alice, Token::Quote, 1000).unwrap(); // Lock 1000 TokenB
        buy_order.locked = 1000;
        book.insert_new_order(buy_order.clone());

        // Bob places two sell orders: 60 TokenA and 40 TokenA at price 10 TokenB
        let mut sell_order1 = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            60,
            now + 1,
        );
        vault.lock(bob, Token::Base, 60).unwrap(); // Lock 60 TokenA
        sell_order1.locked = 60;
        book.insert_new_order(sell_order1.clone());

        let mut sell_order2 = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            40,
            now + 2,
        );
        vault.lock(bob, Token::Base, 40).unwrap(); // Lock 40 TokenA
        sell_order2.locked = 40;
        book.insert_new_order(sell_order2.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0);
        assert_eq!(vault.get_locked(alice, Token::Quote), 1000);
        assert_eq!(vault.get_balance(bob, Token::Base), 900);
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the first sell order
        let (remaining_sell1, events1) = book
            .match_buy_orders(sell_order1.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell1.is_none()); // First sell order should be fully filled
        assert_eq!(events1.len(), 2); // Two fill events

        // Match the second sell order
        let (remaining_sell2, events2) = book
            .match_buy_orders(sell_order2.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell2.is_none()); // Second sell order should be fully filled
        assert_eq!(events2.len(), 2); // Two fill events

        // Verify the matches by checking the events
        let (buy_event1, sell_event1) = (&events1[0], &events1[1]);
        assert_eq!(buy_event1.order_id, buy_order.id);
        assert_eq!(sell_event1.order_id, sell_order1.id);
        assert_eq!(buy_event1.filled_price, 10);
        assert_eq!(sell_event1.filled_price, 10);
        assert_eq!(buy_event1.filled_qty, 60);
        assert_eq!(sell_event1.filled_qty, 60);

        let (buy_event2, sell_event2) = (&events2[0], &events2[1]);
        assert_eq!(buy_event2.order_id, buy_order.id);
        assert_eq!(sell_event2.order_id, sell_order2.id);
        assert_eq!(buy_event2.filled_price, 10);
        assert_eq!(sell_event2.filled_price, 10);
        assert_eq!(buy_event2.filled_qty, 40);
        assert_eq!(sell_event2.filled_qty, 40);

        // Check final balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1100); // Received 100 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0); // Spent 1000 TokenB
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);
        assert_eq!(vault.get_balance(bob, Token::Base), 900); // Spent 100 TokenA
        assert_eq!(vault.get_locked(bob, Token::Base), 0);
        assert_eq!(vault.get_balance(bob, Token::Quote), 2000); // Received 1000 TokenB
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_buy_price_higher_than_sell() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 8 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 8, 100, now);
        vault.lock(alice, Token::Quote, 800).unwrap(); // Lock 800 TokenB
        buy_order.locked = 800;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 100 TokenA at price 6 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            6,
            100,
            now + 1,
        );
        vault.lock(bob, Token::Base, 100).unwrap(); // Lock 100 TokenA
        sell_order.locked = 100;
        book.insert_new_order(sell_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 200);
        assert_eq!(vault.get_locked(alice, Token::Quote), 800);
        assert_eq!(vault.get_balance(bob, Token::Base), 900);
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the orders
        let (remaining_sell, events) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_none()); // Sell order should be fully filled
        assert_eq!(events.len(), 2); // Two fill events

        // Verify the match by checking the events
        let (buy_event, sell_event) = (&events[0], &events[1]);
        assert_eq!(buy_event.order_id, buy_order.id);
        assert_eq!(sell_event.order_id, sell_order.id);
        assert_eq!(buy_event.filled_price, 6); // Should match at sell price
        assert_eq!(sell_event.filled_price, 6);
        assert_eq!(buy_event.filled_qty, 100);
        assert_eq!(sell_event.filled_qty, 100);

        // Check final balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1100); // Received 100 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 400); // Spent 600 TokenB (at sell price), 200 TokenB unlocked
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);
        assert_eq!(vault.get_balance(bob, Token::Base), 900); // Spent 100 TokenA
        assert_eq!(vault.get_locked(bob, Token::Base), 0);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1600); // Received 600 TokenB
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_buy_price_higher_than_sell_partial_fill() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 8 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 8, 100, now);
        vault.lock(alice, Token::Quote, 800).unwrap(); // Lock 800 TokenB
        buy_order.locked = 800;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 50 TokenA at price 6 TokenB
        let mut sell_order =
            book.make_new_order(bob, (Token::Base, Token::Quote), Side::Sell, 6, 50, now + 1);
        vault.lock(bob, Token::Base, 50).unwrap(); // Lock 50 TokenA
        sell_order.locked = 50;
        book.insert_new_order(sell_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 200);
        assert_eq!(vault.get_locked(alice, Token::Quote), 800);
        assert_eq!(vault.get_balance(bob, Token::Base), 950);
        assert_eq!(vault.get_locked(bob, Token::Base), 50);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the orders
        let (remaining_sell, events) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_none()); // Sell order should be fully filled
        assert_eq!(events.len(), 2); // Two fill events

        // Verify the match by checking the events
        let (buy_event, sell_event) = (&events[0], &events[1]);
        assert_eq!(buy_event.order_id, buy_order.id);
        assert_eq!(sell_event.order_id, sell_order.id);
        assert_eq!(buy_event.filled_price, 6); // Should match at sell price
        assert_eq!(sell_event.filled_price, 6);
        assert_eq!(buy_event.filled_qty, 50);
        assert_eq!(sell_event.filled_qty, 50);

        // Check final balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1050); // Received 50 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 200);
        assert_eq!(vault.get_locked(alice, Token::Quote), 500); // Spent 300 TokenB (at sell price), 500 TokenB trasnfer
        assert_eq!(vault.get_balance(bob, Token::Base), 950); // Spent 50 TokenA
        assert_eq!(vault.get_locked(bob, Token::Base), 0);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1300); // Received 300 TokenB
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_sell_matches_multiple_buys() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places two buy orders: 60 TokenA at price 10 TokenB and 40 TokenA at price 10 TokenB
        let mut buy_order1 =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 60, now);
        vault.lock(alice, Token::Quote, 600).unwrap(); // Lock 600 TokenB
        buy_order1.locked = 600;
        book.insert_new_order(buy_order1.clone());

        let mut buy_order2 = book.make_new_order(
            alice,
            (Token::Base, Token::Quote),
            Side::Buy,
            10,
            40,
            now + 1,
        );
        vault.lock(alice, Token::Quote, 400).unwrap(); // Lock 400 TokenB
        buy_order2.locked = 400;
        book.insert_new_order(buy_order2.clone());

        // Bob places a sell order: 100 TokenA at price 10 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            100,
            now + 2,
        );
        vault.lock(bob, Token::Base, 100).unwrap(); // Lock 100 TokenA
        sell_order.locked = 100;
        book.insert_new_order(sell_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0);
        assert_eq!(vault.get_locked(alice, Token::Quote), 1000);
        assert_eq!(vault.get_balance(bob, Token::Base), 900);
        assert_eq!(vault.get_locked(bob, Token::Base), 100);
        assert_eq!(vault.get_balance(bob, Token::Quote), 1000);
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);

        // Match the sell order against both buy orders
        let (remaining_sell, events) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_none()); // Sell order should be fully filled
        assert_eq!(events.len(), 4); // Four fill events (two for each match)

        // Verify the matches by checking the events
        let (buy_event1, sell_event1) = (&events[0], &events[1]);
        assert_eq!(buy_event1.order_id, buy_order1.id); // Second buy order matched first
        assert_eq!(sell_event1.order_id, sell_order.id);
        assert_eq!(buy_event1.filled_price, 10);
        assert_eq!(sell_event1.filled_price, 10);
        assert_eq!(buy_event1.filled_qty, 60);
        assert_eq!(sell_event1.filled_qty, 60);

        let (buy_event2, sell_event2) = (&events[2], &events[3]);
        assert_eq!(buy_event2.order_id, buy_order2.id); // First buy order matched second
        assert_eq!(sell_event2.order_id, sell_order.id);
        assert_eq!(buy_event2.filled_price, 10);
        assert_eq!(sell_event2.filled_price, 10);
        assert_eq!(buy_event2.filled_qty, 40);
        assert_eq!(sell_event2.filled_qty, 40);

        // Check final balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1100); // Received 100 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0); // Spent 1000 TokenB
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);
        assert_eq!(vault.get_balance(bob, Token::Base), 900); // Spent 100 TokenA
        assert_eq!(vault.get_locked(bob, Token::Base), 0);
        assert_eq!(vault.get_balance(bob, Token::Quote), 2000); // Received 1000 TokenB
        assert_eq!(vault.get_locked(bob, Token::Quote), 0);
    }

    #[test]
    fn test_cancel_unmatched_order() {
        let (mut book, mut vault, alice, _) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 100, now);
        vault.lock(alice, Token::Quote, 1000).unwrap(); // Lock 1000 TokenB
        buy_order.locked = 1000;
        book.insert_new_order(buy_order.clone());

        // Check initial balances
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0);
        assert_eq!(vault.get_locked(alice, Token::Quote), 1000);

        // Cancel the order
        book.cancel_order(alice, buy_order.id, &mut vault).unwrap();

        // Check final balances - all locked tokens should be unlocked
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 1000); // All TokenB unlocked
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);

        // Try to cancel again - should fail
        assert!(matches!(
            book.cancel_order(alice, buy_order.id, &mut vault),
            Err(Error::OrderNotFound(_))
        ));
    }

    #[test]
    fn test_cancel_partially_filled_order() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 100, now);
        vault.lock(alice, Token::Quote, 1000).unwrap(); // Lock 1000 TokenB
        buy_order.locked = 1000;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 50 TokenA at price 10 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            50,
            now + 1,
        );
        vault.lock(bob, Token::Base, 50).unwrap(); // Lock 50 TokenA
        sell_order.locked = 50;
        book.insert_new_order(sell_order.clone());

        // Match the orders
        let (remaining_sell, _) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_none()); // Sell order should be fully filled

        // Check balances after partial fill
        assert_eq!(vault.get_balance(alice, Token::Base), 1050); // Received 50 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0);
        assert_eq!(vault.get_locked(alice, Token::Quote), 500); // 500 TokenB still locked

        // Cancel the partially filled buy order
        book.cancel_order(alice, buy_order.id, &mut vault).unwrap();

        // Check final balances - remaining locked tokens should be unlocked
        assert_eq!(vault.get_balance(alice, Token::Base), 1050); // Still have 50 TokenA from partial fill
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 500); // All remaining TokenB unlocked
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);
    }

    #[test]
    fn test_cancel_fully_filled_order() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 100, now);
        vault.lock(alice, Token::Quote, 1000).unwrap(); // Lock 1000 TokenB
        buy_order.locked = 1000;
        book.insert_new_order(buy_order.clone());

        // Bob places a sell order: 100 TokenA at price 10 TokenB
        let mut sell_order = book.make_new_order(
            bob,
            (Token::Base, Token::Quote),
            Side::Sell,
            10,
            100,
            now + 1,
        );
        vault.lock(bob, Token::Base, 100).unwrap(); // Lock 100 TokenA
        sell_order.locked = 100;
        book.insert_new_order(sell_order.clone());

        // Match the orders
        let (remaining_sell, _) = book
            .match_buy_orders(sell_order.clone(), &mut vault)
            .unwrap();
        assert!(remaining_sell.is_none()); // Sell order should be fully filled

        // Check balances after full fill
        assert_eq!(vault.get_balance(alice, Token::Base), 1100); // Received 100 TokenA
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0); // Spent all TokenB
        assert_eq!(vault.get_locked(alice, Token::Quote), 0);

        // Try to cancel the fully filled order - should fail
        assert!(matches!(
            book.cancel_order(alice, buy_order.id, &mut vault),
            Err(Error::OrderNotFound(_))
        ));
    }

    #[test]
    fn test_cancel_unauthorized() {
        let (mut book, mut vault, alice, bob) = setup();
        let now = 1;

        // Alice places a buy order: 100 TokenA at price 10 TokenB
        let mut buy_order =
            book.make_new_order(alice, (Token::Base, Token::Quote), Side::Buy, 10, 100, now);
        vault.lock(alice, Token::Quote, 1000).unwrap(); // Lock 1000 TokenB
        buy_order.locked = 1000;
        book.insert_new_order(buy_order.clone());

        // Bob tries to cancel Alice's order - should fail
        assert!(matches!(
            book.cancel_order(bob, buy_order.id, &mut vault),
            Err(Error::Unauthorized(_))
        ));

        // Check balances - should be unchanged
        assert_eq!(vault.get_balance(alice, Token::Base), 1000);
        assert_eq!(vault.get_locked(alice, Token::Base), 0);
        assert_eq!(vault.get_balance(alice, Token::Quote), 0);
        assert_eq!(vault.get_locked(alice, Token::Quote), 1000);
    }
}
