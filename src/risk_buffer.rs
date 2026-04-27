//! Persistent 4-entry highest-notional cache, ported verbatim from the legacy
//! `mod risk_buffer`. Pure data + bytemuck — no solana-program / anchor deps.

use crate::constants::RISK_BUF_CAP;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RiskEntry {
    pub idx: u16,
    pub _pad: [u8; 14],
    pub notional: u128,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RiskBuffer {
    pub scan_cursor: u16,
    pub count: u8,
    pub _pad: [u8; 13],
    pub min_notional: u128,
    pub entries: [RiskEntry; RISK_BUF_CAP],
}

impl RiskBuffer {
    pub fn recompute_min(&mut self) {
        self.min_notional = match self.count {
            0 => 0,
            1 => self.entries[0].notional,
            2 => core::cmp::min(self.entries[0].notional, self.entries[1].notional),
            3 => core::cmp::min(
                self.entries[0].notional,
                core::cmp::min(self.entries[1].notional, self.entries[2].notional),
            ),
            _ => core::cmp::min(
                core::cmp::min(self.entries[0].notional, self.entries[1].notional),
                core::cmp::min(self.entries[2].notional, self.entries[3].notional),
            ),
        };
    }

    pub fn find(&self, idx: u16) -> Option<usize> {
        if self.count > 0 && self.entries[0].idx == idx {
            return Some(0);
        }
        if self.count > 1 && self.entries[1].idx == idx {
            return Some(1);
        }
        if self.count > 2 && self.entries[2].idx == idx {
            return Some(2);
        }
        if self.count > 3 && self.entries[3].idx == idx {
            return Some(3);
        }
        None
    }

    fn min_slot(&self) -> usize {
        let mut m = 0;
        if self.count > 1 && self.entries[1].notional < self.entries[m].notional {
            m = 1;
        }
        if self.count > 2 && self.entries[2].notional < self.entries[m].notional {
            m = 2;
        }
        if self.count > 3 && self.entries[3].notional < self.entries[m].notional {
            m = 3;
        }
        m
    }

    pub fn upsert(&mut self, idx: u16, notional: u128) -> bool {
        if let Some(slot) = self.find(idx) {
            if self.entries[slot].notional == notional {
                return false;
            }
            self.entries[slot].notional = notional;
            self.recompute_min();
            return true;
        }
        if (self.count as usize) < RISK_BUF_CAP {
            let s = self.count as usize;
            self.entries[s].idx = idx;
            self.entries[s].notional = notional;
            self.entries[s]._pad = [0; 14];
            self.count += 1;
            self.recompute_min();
            return true;
        }
        if notional <= self.min_notional {
            return false;
        }
        let victim = self.min_slot();
        self.entries[victim].idx = idx;
        self.entries[victim].notional = notional;
        self.entries[victim]._pad = [0; 14];
        self.recompute_min();
        true
    }

    pub fn remove(&mut self, idx: u16) -> bool {
        let slot = match self.find(idx) {
            Some(s) => s,
            None => return false,
        };
        let last = self.count as usize - 1;
        if slot != last {
            self.entries[slot] = self.entries[last];
        }
        self.entries[last] = RiskEntry::zeroed();
        self.count -= 1;
        self.recompute_min();
        true
    }
}
