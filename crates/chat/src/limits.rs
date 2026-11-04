// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Limits per address and per chatter, kept only in memory and only for minutes (ADR-0120;
//! review 0025, C1). Nothing here is written anywhere, so retention is unaffected: an address
//! is forgotten once its window has passed.
//!
//! An IPv6 address counts by its /64, since one machine commonly holds many addresses in it;
//! an IPv4 address counts alone. Callers behind one carrier-grade NAT share an address, so the
//! defaults are generous: they stop one host flooding the chat, not a household.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Mutex;

/// The key an address counts under: an IPv4 address, or an IPv6 address's /64.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressKey {
    V4(u32),
    V6Prefix(u64),
}

impl From<IpAddr> for AddressKey {
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => AddressKey::V4(u32::from(v4)),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => AddressKey::V4(u32::from(v4)),
                None => AddressKey::V6Prefix((u128::from(v6) >> 64) as u64),
            },
        }
    }
}

/// At most `max` events per key in each window of `window_ms`.
#[derive(Debug)]
pub struct Window<K> {
    max: u32,
    window_ms: i64,
    seen: Mutex<HashMap<K, (i64, u32)>>,
}

impl<K: Eq + Hash + Copy> Window<K> {
    pub fn new(max: u32, window_ms: i64) -> Self {
        Window {
            max,
            window_ms,
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Count one event for `key` at `now_ms`; false (and not counted) when over the limit.
    /// A limit of 0 turns the check off.
    pub fn take(&self, key: K, now_ms: i64) -> bool {
        if self.max == 0 {
            return true;
        }
        let mut seen = self.seen.lock().expect("window lock");
        if seen.len() > 4096 {
            seen.retain(|_, (start, _)| now_ms - *start < self.window_ms);
        }
        let e = seen.entry(key).or_insert((now_ms, 0));
        if now_ms - e.0 >= self.window_ms {
            *e = (now_ms, 0);
        }
        if e.1 >= self.max {
            return false;
        }
        e.1 += 1;
        true
    }

    /// Keys remembered now (for tests and the forgetting check).
    pub fn len(&self) -> usize {
        self.seen.lock().expect("window lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Forget every window that has passed.
    pub fn forget_old(&self, now_ms: i64) {
        self.seen
            .lock()
            .expect("window lock")
            .retain(|_, (start, _)| now_ms - *start < self.window_ms);
    }
}

/// Things held open at once per address (sockets): at most `max` each.
#[derive(Debug)]
pub struct Open {
    max: usize,
    open: Mutex<HashMap<AddressKey, usize>>,
}

impl Open {
    pub fn new(max: usize) -> Self {
        Open {
            max,
            open: Mutex::new(HashMap::new()),
        }
    }

    /// Open one for `key`; false when it already has `max` (0 turns the check off).
    pub fn open(&self, key: AddressKey) -> bool {
        let mut open = self.open.lock().expect("open lock");
        let n = open.entry(key).or_insert(0);
        if self.max != 0 && *n >= self.max {
            if *n == 0 {
                open.remove(&key);
            }
            return false;
        }
        *n += 1;
        true
    }

    /// Close one; an address with none open is forgotten.
    pub fn close(&self, key: AddressKey) {
        let mut open = self.open.lock().expect("open lock");
        if let Some(n) = open.get_mut(&key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                open.remove(&key);
            }
        }
    }

    pub fn addresses(&self) -> usize {
        self.open.lock().expect("open lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> AddressKey {
        s.parse::<IpAddr>().unwrap().into()
    }

    #[test]
    fn an_ipv6_address_counts_by_its_64_and_ipv4_alone() {
        assert_eq!(ip("2001:db8:1:2::1"), ip("2001:db8:1:2:ffff::9"));
        assert_ne!(ip("2001:db8:1:2::1"), ip("2001:db8:1:3::1"));
        assert_ne!(ip("192.0.2.1"), ip("192.0.2.2"));
        assert_eq!(ip("::ffff:192.0.2.1"), ip("192.0.2.1"));
        // An IPv4 address never collides with a /64.
        assert_ne!(ip("0.0.0.1"), ip("::1"));
    }

    #[test]
    fn a_window_allows_its_count_then_refuses_until_it_passes() {
        let w = Window::new(3, 60_000);
        let a = ip("192.0.2.1");
        assert!((0..3).all(|_| w.take(a, 1_000)));
        assert!(!w.take(a, 2_000));
        assert!(w.take(ip("192.0.2.2"), 2_000), "another address is its own");
        assert!(w.take(a, 61_000), "a new window");
        w.forget_old(200_000);
        assert!(
            w.is_empty(),
            "addresses are forgotten once their window has passed"
        );
    }

    #[test]
    fn a_limit_of_zero_is_off() {
        let w = Window::new(0, 60_000);
        assert!((0..100).all(|_| w.take(ip("192.0.2.1"), 0)));
        let o = Open::new(0);
        assert!((0..100).all(|_| o.open(ip("192.0.2.1"))));
    }

    #[test]
    fn open_things_are_counted_and_released() {
        let o = Open::new(2);
        let a = ip("192.0.2.1");
        assert!(o.open(a) && o.open(a));
        assert!(!o.open(a));
        o.close(a);
        assert!(o.open(a));
        o.close(a);
        o.close(a);
        assert_eq!(
            o.addresses(),
            0,
            "an address with nothing open is forgotten"
        );
    }
}
