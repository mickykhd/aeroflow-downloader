use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};

/// Per-hour QoS mode — mirrors the DESIGN scheduler matrix colors:
/// cyan = Turbo uncapped, amber = throttled work-hours, violet = P2P priority, dark = sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SlotMode {
    #[default]
    Turbo,
    Throttled,
    P2pPriority,
    Sleep,
}

impl SlotMode {
    pub fn cap_bps(self) -> Option<u64> {
        match self {
            Self::Turbo => None,
            Self::Throttled => Some(15_000_000), // 15 MB/s work guardrail
            Self::P2pPriority => Some(50_000_000),
            Self::Sleep => Some(0),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Turbo => "Turbo Uncapped",
            Self::Throttled => "Throttled QoS",
            Self::P2pPriority => "P2P Priority",
            Self::Sleep => "Sleep / Paused",
        }
    }
}

/// 7 (Mon=0..Sun=6) × 24 hourly slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyMatrix {
    pub slots: [[SlotMode; 24]; 7],
}

impl Default for WeeklyMatrix {
    /// Default mirrors the mock: nights/weekends Turbo, 09–18 weekdays throttled,
    /// evenings P2P, Sun 12–18 sleep.
    fn default() -> Self {
        let mut slots = [[SlotMode::Turbo; 24]; 7];
        for day in 0..5 {
            for h in 9..18 {
                slots[day][h] = SlotMode::Throttled;
            }
            for h in 18..24 {
                slots[day][h] = SlotMode::P2pPriority;
            }
        }
        // Saturday: P2P morning, Turbo rest
        for h in 0..12 {
            slots[5][h] = SlotMode::P2pPriority;
        }
        // Sunday: P2P morning, sleep midday, Turbo evening
        for h in 0..12 {
            slots[6][h] = SlotMode::P2pPriority;
        }
        for h in 12..16 {
            slots[6][h] = SlotMode::Sleep;
        }
        Self { slots }
    }
}

impl WeeklyMatrix {
    pub fn mode_at(&self, weekday_mon0: usize, hour: usize) -> SlotMode {
        self.slots[weekday_mon0 % 7][hour % 24]
    }

    pub fn current(&self) -> (usize, usize, SlotMode) {
        let now = Local::now();
        let dow = now.weekday().num_days_from_monday() as usize;
        let h = now.hour() as usize;
        (dow, h, self.mode_at(dow, h))
    }

    /// Hours until the mode changes (for "Next transition in 03h 32m" UX).
    pub fn hours_until_change(&self) -> u32 {
        let (dow, h, cur) = self.current();
        for step in 1..=(7 * 24) {
            let nh = (h + step as usize) % 24;
            let nd = (dow + (h + step as usize) / 24) % 7;
            if self.slots[nd][nh] != cur {
                return step as u32;
            }
        }
        24
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matrix_work_hours_throttled() {
        let m = WeeklyMatrix::default();
        assert_eq!(m.mode_at(0, 2), SlotMode::Turbo); // Mon 02:00
        assert_eq!(m.mode_at(0, 14), SlotMode::Throttled); // Mon 14:00
        assert_eq!(m.mode_at(0, 20), SlotMode::P2pPriority);
        assert_eq!(m.mode_at(6, 13), SlotMode::Sleep); // Sun afternoon
    }
}
