//! Dual-tier short/long break queue with 4:1 ratio, deterministic shuffling, and per-break overrides.

use serde::{Deserialize, Serialize};

use crate::config::{BreakSpec, BreakType, Config};
use crate::rng::SplitMix64;

/// Break specification resolved with effective tier or per-break timing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedBreak {
    pub break_type: BreakType,
    pub name: String,
    pub interval_secs: u64,
    pub duration_secs: u64,
}

/// Break currently staged at the front of `BreakQueue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedBreak {
    pub spec: ResolvedBreak,
    pub wait_secs: u64,
}

/// Dual-tier break queue managing short and long exercises.
#[derive(Debug, Clone)]
pub struct BreakQueue {
    short_breaks: Vec<ResolvedBreak>,
    long_breaks: Vec<ResolvedBreak>,
    short_index: usize,
    long_index: usize,
    long_remaining_secs: u64,
    random_order: bool,
    rng: SplitMix64,
    current: QueuedBreak,
    manual_short_override: bool,
}

impl BreakQueue {
    /// Builds a `BreakQueue` from `config` and an explicit PRNG seed.
    ///
    /// Returns `None` if neither tier contains any valid exercise.
    #[must_use]
    pub fn from_config(config: &Config, seed: u64) -> Option<Self> {
        let mut short_breaks = resolve_tier(
            BreakType::Short,
            &config.short_breaks,
            config.short_break_interval_secs,
            config.short_break_duration_secs,
        );
        let mut long_breaks = resolve_tier(
            BreakType::Long,
            &config.long_breaks,
            config.long_break_interval_secs,
            config.long_break_duration_secs,
        );

        if short_breaks.is_empty() && long_breaks.is_empty() {
            return None;
        }

        let mut rng = SplitMix64::new(seed);
        if config.random_order {
            rng.shuffle(&mut short_breaks);
            rng.shuffle(&mut long_breaks);
        }

        let long_remaining_secs = long_breaks.first().map_or(0, |b| b.interval_secs);
        let mut short_index = 0;
        let mut long_index = 0;
        let current = select_next_break(
            &mut short_breaks,
            &mut short_index,
            &mut long_breaks,
            &mut long_index,
            long_remaining_secs,
            config.random_order,
            &mut rng,
        )?;
        Some(Self {
            short_breaks,
            long_breaks,
            short_index,
            long_index,
            long_remaining_secs,
            random_order: config.random_order,
            rng,
            current,
            manual_short_override: false,
        })
    }

    /// Restores persisted queue indices and remaining long-break countdown safely.
    pub fn restore_progress(
        &mut self,
        short_index: usize,
        long_index: usize,
        long_remaining_secs: Option<u64>,
    ) {
        if !self.short_breaks.is_empty() {
            self.short_index = short_index % self.short_breaks.len();
        }
        if !self.long_breaks.is_empty() {
            self.long_index = long_index % self.long_breaks.len();
            if let Some(rem) = long_remaining_secs
                && rem > 0
            {
                self.long_remaining_secs = rem;
            }
        }
        self.manual_short_override = false;
        self.current = self.select_next();
    }

    /// Returns the currently staged break.
    #[must_use]
    pub const fn current(&self) -> &QueuedBreak {
        &self.current
    }

    /// Returns the current short-break index (next slot after the staged break).
    #[must_use]
    pub const fn short_index(&self) -> usize {
        self.short_index
    }

    /// Returns the current long-break index (next slot after the staged break).
    #[must_use]
    pub const fn long_index(&self) -> usize {
        self.long_index
    }

    /// Returns the short-break index to persist so restoring resumes at the staged break.
    #[must_use]
    pub fn persisted_short_index(&self) -> usize {
        persisted_index(
            self.short_index,
            self.short_breaks.len(),
            self.current.spec.break_type == BreakType::Short,
        )
    }

    /// Returns the long-break index to persist so restoring resumes at the staged break.
    #[must_use]
    pub fn persisted_long_index(&self) -> usize {
        persisted_index(
            self.long_index,
            self.long_breaks.len(),
            self.current.spec.break_type == BreakType::Long,
        )
    }

    /// Returns remaining seconds until the next long break (if long breaks exist).
    #[must_use]
    pub const fn long_remaining_secs(&self) -> Option<u64> {
        if self.long_breaks.is_empty() {
            None
        } else {
            Some(self.long_remaining_secs)
        }
    }

    /// Returns the duration of the next long break (if any long break is configured).
    #[must_use]
    pub fn next_long_duration_secs(&self) -> Option<u64> {
        if self.current.spec.break_type == BreakType::Long {
            return Some(self.current.spec.duration_secs);
        }
        self.long_breaks
            .get(self.long_index)
            .map(|b| b.duration_secs)
    }

    /// Returns whether the queue has at least one break of `kind`.
    #[must_use]
    pub const fn has_break_type(&self, kind: BreakType) -> bool {
        match kind {
            BreakType::Short => !self.short_breaks.is_empty(),
            BreakType::Long => !self.long_breaks.is_empty(),
        }
    }

    /// Advances the queue after the current break completes or is skipped.
    ///
    /// Never call this on postpone.
    pub fn advance(&mut self) -> &QueuedBreak {
        if !self.long_breaks.is_empty() {
            match self.current.spec.break_type {
                BreakType::Short => {
                    if self.manual_short_override {
                        self.manual_short_override = false;
                    } else {
                        self.long_remaining_secs = self
                            .long_remaining_secs
                            .saturating_sub(self.current.wait_secs);
                    }
                }
                BreakType::Long => {
                    self.manual_short_override = false;
                    self.long_remaining_secs = self.long_breaks[self.long_index].interval_secs;
                }
            }
        }
        self.current = self.select_next();
        &self.current
    }

    /// Selects a break for an immediate manual `TakeBreak` request.
    pub fn select_manual(&mut self, kind: Option<BreakType>) -> Option<QueuedBreak> {
        match kind {
            None => Some(self.current.clone()),
            Some(BreakType::Short) => {
                if self.short_breaks.is_empty() {
                    return None;
                }
                if self.current.spec.break_type == BreakType::Short {
                    return Some(self.current.clone());
                }
                let spec = self.take_next_short();
                let wait_secs = spec.interval_secs;
                self.current = QueuedBreak { spec, wait_secs };
                self.manual_short_override = true;
                Some(self.current.clone())
            }
            Some(BreakType::Long) => {
                if self.long_breaks.is_empty() {
                    return None;
                }
                if self.current.spec.break_type == BreakType::Long {
                    return Some(self.current.clone());
                }
                let spec = self.take_next_long();
                let wait_secs = spec.interval_secs;
                self.current = QueuedBreak { spec, wait_secs };
                self.manual_short_override = false;
                Some(self.current.clone())
            }
        }
    }

    /// Resets the long-break countdown after a natural rest longer than `long_break_duration`.
    pub fn skip_long_break(&mut self) {
        self.manual_short_override = false;
        if self.long_breaks.is_empty() {
            self.current.wait_secs = self.current.spec.interval_secs;
            return;
        }

        self.long_remaining_secs = self.long_breaks[self.long_index].interval_secs;

        if self.current.spec.break_type == BreakType::Long && !self.short_breaks.is_empty() {
            let spec = self.take_next_short();
            let wait_secs = spec.interval_secs;
            self.current = QueuedBreak { spec, wait_secs };
        } else {
            self.current.wait_secs = self.current.spec.interval_secs;
        }
    }

    fn select_next(&mut self) -> QueuedBreak {
        select_next_break(
            &mut self.short_breaks,
            &mut self.short_index,
            &mut self.long_breaks,
            &mut self.long_index,
            self.long_remaining_secs,
            self.random_order,
            &mut self.rng,
        )
        .unwrap_or_else(|| self.current.clone())
    }

    fn take_next_short(&mut self) -> ResolvedBreak {
        take_next_from(
            &mut self.short_breaks,
            &mut self.short_index,
            self.random_order,
            &mut self.rng,
        )
    }

    fn take_next_long(&mut self) -> ResolvedBreak {
        take_next_from(
            &mut self.long_breaks,
            &mut self.long_index,
            self.random_order,
            &mut self.rng,
        )
    }
}

fn select_next_break(
    short_breaks: &mut [ResolvedBreak],
    short_index: &mut usize,
    long_breaks: &mut [ResolvedBreak],
    long_index: &mut usize,
    long_remaining_secs: u64,
    random_order: bool,
    rng: &mut SplitMix64,
) -> Option<QueuedBreak> {
    match (short_breaks.is_empty(), long_breaks.is_empty()) {
        (false, true) => {
            let spec = take_next_from(short_breaks, short_index, random_order, rng);
            let wait_secs = spec.interval_secs;
            Some(QueuedBreak { spec, wait_secs })
        }
        (true, false) => {
            let spec = take_next_from(long_breaks, long_index, random_order, rng);
            let wait_secs = spec.interval_secs;
            Some(QueuedBreak { spec, wait_secs })
        }
        (false, false) => {
            let next_short_interval = short_breaks[*short_index].interval_secs;
            if long_remaining_secs <= next_short_interval {
                let spec = take_next_from(long_breaks, long_index, random_order, rng);
                let wait_secs = long_remaining_secs.max(1);
                Some(QueuedBreak { spec, wait_secs })
            } else {
                let spec = take_next_from(short_breaks, short_index, random_order, rng);
                let wait_secs = spec.interval_secs;
                Some(QueuedBreak { spec, wait_secs })
            }
        }
        (true, true) => None,
    }
}

const fn persisted_index(index: usize, len: usize, is_current_tier: bool) -> usize {
    if is_current_tier && len > 0 {
        match index.checked_sub(1) {
            Some(prev) => prev,
            None => len - 1,
        }
    } else {
        index
    }
}

fn take_next_from(
    items: &mut [ResolvedBreak],
    index: &mut usize,
    random_order: bool,
    rng: &mut SplitMix64,
) -> ResolvedBreak {
    let item = items[*index].clone();
    *index += 1;
    if *index >= items.len() {
        *index = 0;
        if random_order && items.len() > 1 {
            rng.shuffle(items);
        }
    }
    item
}

fn resolve_tier(
    break_type: BreakType,
    specs: &[BreakSpec],
    default_interval_secs: u64,
    default_duration_secs: u64,
) -> Vec<ResolvedBreak> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let trimmed = spec.name.trim();
        let interval_secs = spec.interval_secs.unwrap_or(default_interval_secs);
        let duration_secs = spec.duration_secs.unwrap_or(default_duration_secs);
        if trimmed.is_empty() || interval_secs == 0 || duration_secs == 0 {
            continue;
        }
        out.push(ResolvedBreak {
            break_type,
            name: trimmed.to_owned(),
            interval_secs,
            duration_secs,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::config::{DEFAULT_LONG_BREAK_NAMES, DEFAULT_SHORT_BREAK_NAMES};

    #[test]
    fn default_exercises_exact_counts_and_names() {
        let cfg = Config::default();
        assert_eq!(cfg.short_breaks.len(), 13);
        assert_eq!(cfg.long_breaks.len(), 6);
        for (spec, expected) in cfg.short_breaks.iter().zip(DEFAULT_SHORT_BREAK_NAMES) {
            assert_eq!(spec.name, expected);
        }
        for (spec, expected) in cfg.long_breaks.iter().zip(DEFAULT_LONG_BREAK_NAMES) {
            assert_eq!(spec.name, expected);
        }
    }

    #[test]
    fn four_to_one_short_long_ratio_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let cfg = Config {
            random_order: false,
            ..Config::default()
        };
        let Some(mut queue) = BreakQueue::from_config(&cfg, 42) else {
            return Err("expected non-empty queue".into());
        };

        let mut types = Vec::new();
        for _ in 0..10 {
            assert_eq!(queue.current().wait_secs, 900);
            types.push(queue.current().spec.break_type);
            queue.advance();
        }
        assert_eq!(
            types,
            vec![
                BreakType::Short,
                BreakType::Short,
                BreakType::Short,
                BreakType::Short,
                BreakType::Long,
                BreakType::Short,
                BreakType::Short,
                BreakType::Short,
                BreakType::Short,
                BreakType::Long,
            ]
        );
        Ok(())
    }

    #[test]
    fn sequential_order_preserves_config_order_and_wraps() -> Result<(), Box<dyn std::error::Error>>
    {
        let cfg = Config {
            random_order: false,
            short_breaks: vec![BreakSpec::new("S1"), BreakSpec::new("S2")],
            long_breaks: Vec::new(),
            ..Config::default()
        };
        let Some(mut queue) = BreakQueue::from_config(&cfg, 1) else {
            return Err("expected queue".into());
        };
        let mut names = Vec::new();
        for _ in 0..4 {
            names.push(queue.current().spec.name.clone());
            queue.advance();
        }
        assert_eq!(names, vec!["S1", "S2", "S1", "S2"]);
        Ok(())
    }

    #[test]
    fn random_order_deterministic_with_seed_and_covers_all_exercises()
    -> Result<(), Box<dyn std::error::Error>> {
        let cfg = Config {
            random_order: true,
            long_breaks: Vec::new(),
            ..Config::default()
        };
        let Some(mut q1) = BreakQueue::from_config(&cfg, 42) else {
            return Err("expected queue".into());
        };
        let Some(mut q2) = BreakQueue::from_config(&cfg, 42) else {
            return Err("expected queue".into());
        };

        let mut cycle1 = Vec::new();
        for _ in 0..13 {
            assert_eq!(q1.current().spec.name, q2.current().spec.name);
            cycle1.push(q1.current().spec.name.clone());
            q1.advance();
            q2.advance();
        }
        let unique: HashSet<&str> = cycle1.iter().map(String::as_str).collect();
        assert_eq!(unique.len(), 13);
        Ok(())
    }

    #[test]
    fn per_break_interval_and_duration_overrides() -> Result<(), Box<dyn std::error::Error>> {
        let cfg = Config {
            random_order: false,
            short_break_interval_secs: 900,
            short_break_duration_secs: 15,
            short_breaks: vec![
                BreakSpec {
                    name: "Custom".to_owned(),
                    interval_secs: Some(300),
                    duration_secs: Some(25),
                },
                BreakSpec::new("Default"),
            ],
            long_breaks: Vec::new(),
            ..Config::default()
        };
        let Some(mut queue) = BreakQueue::from_config(&cfg, 7) else {
            return Err("expected queue".into());
        };
        assert_eq!(queue.current().spec.name, "Custom");
        assert_eq!(queue.current().spec.interval_secs, 300);
        assert_eq!(queue.current().spec.duration_secs, 25);
        assert_eq!(queue.current().wait_secs, 300);

        queue.advance();
        assert_eq!(queue.current().spec.name, "Default");
        assert_eq!(queue.current().spec.interval_secs, 900);
        assert_eq!(queue.current().spec.duration_secs, 15);
        Ok(())
    }

    #[test]
    fn short_interval_gte_long_interval_selects_long_without_underflow()
    -> Result<(), Box<dyn std::error::Error>> {
        for (short_i, long_i) in [(900, 900), (1200, 900)] {
            let cfg = Config {
                random_order: false,
                short_break_interval_secs: short_i,
                long_break_interval_secs: long_i,
                ..Config::default()
            };
            let Some(mut queue) = BreakQueue::from_config(&cfg, 9) else {
                return Err("expected queue".into());
            };
            for _ in 0..5 {
                assert_eq!(queue.current().spec.break_type, BreakType::Long);
                assert_eq!(queue.current().wait_secs, long_i);
                queue.advance();
            }
        }
        Ok(())
    }

    #[test]
    fn empty_or_invalid_queues_return_none() {
        let empty = Config {
            short_breaks: Vec::new(),
            long_breaks: Vec::new(),
            ..Config::default()
        };
        assert!(BreakQueue::from_config(&empty, 1).is_none());

        let zero_dur = Config {
            short_breaks: vec![BreakSpec {
                name: "Bad".to_owned(),
                interval_secs: Some(10),
                duration_secs: Some(0),
            }],
            long_breaks: Vec::new(),
            ..Config::default()
        };
        assert!(BreakQueue::from_config(&zero_dur, 1).is_none());
    }

    #[test]
    fn single_tier_queues_handle_skip_long_break_safely() -> Result<(), Box<dyn std::error::Error>>
    {
        let short_only = Config {
            long_breaks: Vec::new(),
            ..Config::default()
        };
        let Some(mut sq) = BreakQueue::from_config(&short_only, 1) else {
            return Err("expected short-only queue".into());
        };
        sq.skip_long_break();
        assert_eq!(sq.current().spec.break_type, BreakType::Short);

        let long_only = Config {
            short_breaks: Vec::new(),
            ..Config::default()
        };
        let Some(mut lq) = BreakQueue::from_config(&long_only, 1) else {
            return Err("expected long-only queue".into());
        };
        lq.skip_long_break();
        assert_eq!(lq.current().spec.break_type, BreakType::Long);
        assert_eq!(lq.current().wait_secs, 4500);
        Ok(())
    }

    #[test]
    fn skip_long_break_resets_pending_long_and_mid_cycle() -> Result<(), Box<dyn std::error::Error>>
    {
        let cfg = Config {
            random_order: false,
            ..Config::default()
        };
        let Some(mut queue) = BreakQueue::from_config(&cfg, 1) else {
            return Err("expected queue".into());
        };
        // Advance 4 short breaks so Long is staged next.
        for _ in 0..4 {
            assert_eq!(queue.current().spec.break_type, BreakType::Short);
            queue.advance();
        }
        assert_eq!(queue.current().spec.break_type, BreakType::Long);

        queue.skip_long_break();
        assert_eq!(queue.current().spec.break_type, BreakType::Short);
        assert_eq!(queue.long_remaining_secs(), Some(4500));

        // Advance 2 short breaks, then skip_long_break mid-cycle.
        queue.advance();
        queue.advance();
        assert_eq!(queue.long_remaining_secs(), Some(2700));
        queue.skip_long_break();
        assert_eq!(queue.long_remaining_secs(), Some(4500));

        for _ in 0..4 {
            assert_eq!(queue.current().spec.break_type, BreakType::Short);
            queue.advance();
        }
        assert_eq!(queue.current().spec.break_type, BreakType::Long);
        Ok(())
    }
}
