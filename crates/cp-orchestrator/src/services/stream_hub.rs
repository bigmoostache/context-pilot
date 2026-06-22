//! Per-agent **stream hub** — fans one agent's ephemeral stream to N bounded
//! subscribers, dropping and self-healing under back-pressure.
//!
//! The backend reads an agent's [`Frame`] stream off its UDS socket and
//! [`publish`](StreamHub::publish)es each frame to every [`Subscriber`] of that
//! agent (a subscriber maps, in a later phase, to one frontend WebSocket). Each
//! subscriber has its own **bounded** buffer; a slow consumer cannot stall the
//! agent or the other subscribers (design doc I7 producer-never-blocks,
//! extended to the fan-out).
//!
//! # Overflow → degraded → reconcile (R2-17)
//!
//! On a full buffer the **oldest** frame is evicted to admit the newest, so a
//! lagging subscriber always holds the most recent window (coalescing toward
//! current). Every eviction sets the subscriber's `degraded` flag and bumps a
//! dropped counter. `degraded` is the signal that the subscriber has missed
//! frames and must be **reconciled** from the authoritative oplog snapshot
//! (heads) — the stream plane is best-effort, the oplog is the safety net.
//! After the caller delivers that snapshot it calls
//! [`mark_reconciled`](StreamHub::mark_reconciled) to clear the flag.
//!
//! This module is the buffering + overflow *policy*; the socket read loop and
//! the WebSocket writer that drives it arrive with the transport phase. It is
//! single-owner (the orchestrator loop) and therefore lock-free.

use std::collections::{HashMap, VecDeque};

use cp_wire::types::stream::Frame;

/// One downstream consumer of an agent's stream (≈ one WebSocket client).
#[derive(Debug)]
pub struct Subscriber {
    /// Stable per-agent subscriber id.
    id: u64,

    /// Bounded FIFO of undelivered frames (oldest at the front).
    buffer: VecDeque<Frame>,

    /// Maximum frames the buffer may hold before evicting the oldest.
    capacity: usize,

    /// Set once a frame has been dropped; cleared only by reconciliation.
    degraded: bool,

    /// Total frames dropped since the last reconcile.
    dropped: u64,
}

impl Subscriber {
    /// Create an empty subscriber with the given buffer `capacity`.
    fn new(id: u64, capacity: usize) -> Self {
        Self { id, buffer: VecDeque::new(), capacity, degraded: false, dropped: 0 }
    }

    /// Admit one frame, evicting the oldest if the buffer is full.
    ///
    /// Returns `true` if the frame was admitted without loss, `false` if an
    /// eviction occurred (which also marks the subscriber degraded). A
    /// zero-capacity subscriber drops every frame.
    fn admit(&mut self, frame: Frame) -> bool {
        if self.capacity == 0 {
            self.degraded = true;
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        if self.buffer.len() >= self.capacity {
            let _evicted = self.buffer.pop_front();
            self.degraded = true;
            self.dropped = self.dropped.saturating_add(1);
            self.buffer.push_back(frame);
            return false;
        }
        self.buffer.push_back(frame);
        true
    }

    /// Remove and return all buffered frames in arrival order.
    pub fn drain(&mut self) -> Vec<Frame> {
        self.buffer.drain(..).collect()
    }

    /// This subscriber's id.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Whether the subscriber has lost frames and awaits reconciliation.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        self.degraded
    }

    /// Frames dropped since the last reconcile.
    #[must_use]
    pub const fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// Number of frames currently buffered.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// Clear the degraded state after the caller has delivered an authoritative
    /// snapshot resync to this subscriber.
    fn mark_reconciled(&mut self) {
        self.degraded = false;
        self.dropped = 0;
    }
}

/// Fan-out registry: per-agent subscribers, each independently bounded.
#[derive(Debug)]
pub struct StreamHub {
    /// Subscribers grouped by `agent_id`.
    agents: HashMap<String, Vec<Subscriber>>,

    /// Monotonic id source for new subscribers.
    next_id: u64,

    /// Per-subscriber buffer capacity applied at subscribe time.
    sub_capacity: usize,
}

impl StreamHub {
    /// Create a hub whose subscribers each buffer up to `sub_capacity` frames.
    #[must_use]
    pub fn new(sub_capacity: usize) -> Self {
        Self { agents: HashMap::new(), next_id: 0, sub_capacity }
    }

    /// Register a new subscriber for `agent_id`, returning its id.
    pub fn subscribe(&mut self, agent_id: &str) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.agents.entry(agent_id.to_owned()).or_default().push(Subscriber::new(id, self.sub_capacity));
        id
    }

    /// Remove a subscriber. Returns `true` if it was present.
    pub fn unsubscribe(&mut self, agent_id: &str, sub_id: u64) -> bool {
        let Some(subs) = self.agents.get_mut(agent_id) else {
            return false;
        };
        let before = subs.len();
        subs.retain(|s| s.id != sub_id);
        let removed = subs.len() != before;
        if subs.is_empty() {
            let _gone = self.agents.remove(agent_id);
        }
        removed
    }

    /// Fan a frame out to every subscriber of `agent_id`.
    ///
    /// Returns the number of subscribers that admitted it without loss. A frame
    /// for an agent with no subscribers is silently discarded (a late
    /// subscriber sees live frames, not a backlog).
    pub fn publish(&mut self, agent_id: &str, frame: &Frame) -> usize {
        let Some(subs) = self.agents.get_mut(agent_id) else {
            return 0;
        };
        let mut clean: usize = 0;
        for sub in subs.iter_mut() {
            if sub.admit(frame.clone()) {
                clean = clean.saturating_add(1);
            }
        }
        clean
    }

    /// Drain a subscriber's buffered frames.
    ///
    /// Returns `None` if no such subscriber exists.
    pub fn drain(&mut self, agent_id: &str, sub_id: u64) -> Option<Vec<Frame>> {
        self.subscriber_mut(agent_id, sub_id).map(Subscriber::drain)
    }

    /// Inspect a subscriber (degraded flag, dropped count, buffered len).
    #[must_use]
    pub fn subscriber(&self, agent_id: &str, sub_id: u64) -> Option<&Subscriber> {
        self.agents.get(agent_id)?.iter().find(|s| s.id == sub_id)
    }

    /// Clear a subscriber's degraded state after delivering a snapshot resync.
    /// Returns `true` if the subscriber was found.
    pub fn mark_reconciled(&mut self, agent_id: &str, sub_id: u64) -> bool {
        match self.subscriber_mut(agent_id, sub_id) {
            Some(sub) => {
                sub.mark_reconciled();
                true
            }
            None => false,
        }
    }

    /// Number of subscribers attached to `agent_id`.
    #[must_use]
    pub fn subscriber_count(&self, agent_id: &str) -> usize {
        self.agents.get(agent_id).map_or(0, Vec::len)
    }

    /// Aggregate stream health for `agent_id` (design doc §19 observability):
    /// `(subscriber_count, total_dropped_frames, any_degraded)`.
    ///
    /// Sums the per-subscriber dropped counters and ORs their degraded flags so
    /// a metrics surface can report whether *any* consumer of this agent's
    /// stream has lost frames (the signal that a reconcile is owed) without
    /// walking subscribers itself. Returns `(0, 0, false)` for an agent with no
    /// subscribers.
    #[must_use]
    pub fn agent_stream_health(&self, agent_id: &str) -> (usize, u64, bool) {
        match self.agents.get(agent_id) {
            None => (0, 0, false),
            Some(subs) => subs.iter().fold((0, 0, false), |(n, dropped, degraded), s| {
                (n + 1, dropped.saturating_add(s.dropped_count()), degraded || s.is_degraded())
            }),
        }
    }

    /// Mutable subscriber lookup (private helper).
    fn subscriber_mut(&mut self, agent_id: &str, sub_id: u64) -> Option<&mut Subscriber> {
        self.agents.get_mut(agent_id)?.iter_mut().find(|s| s.id == sub_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::stream::Kind;

    /// Build a token frame carrying `text` at sequence `seq`.
    fn token(seq: u64, text: &str) -> Frame {
        Frame {
            schema_version: 1,
            agent_id: "a1".into(),
            worker_id: "w0".into(),
            thread_id: "T1".into(),
            message_id: "m1".into(),
            seq,
            kind: Kind::Token { text: text.to_owned() },
        }
    }

    #[test]
    fn publish_fans_to_all_subscribers() {
        let mut hub = StreamHub::new(8);
        let s1 = hub.subscribe("a1");
        let s2 = hub.subscribe("a1");

        let clean = hub.publish("a1", &token(0, "hi"));
        assert_eq!(clean, 2, "both subscribers admit the frame");

        assert_eq!(hub.drain("a1", s1).expect("s1").len(), 1);
        assert_eq!(hub.drain("a1", s2).expect("s2").len(), 1);
    }

    #[test]
    fn drain_returns_frames_in_arrival_order() {
        let mut hub = StreamHub::new(8);
        let s = hub.subscribe("a1");
        for i in 0..3u64 {
            let _c = hub.publish("a1", &token(i, "x"));
        }
        let frames = hub.drain("a1", s).expect("drain");
        let seqs: Vec<u64> = frames.iter().map(|f| f.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    #[test]
    fn overflow_evicts_oldest_and_marks_degraded() {
        let mut hub = StreamHub::new(2);
        let s = hub.subscribe("a1");

        let _c0 = hub.publish("a1", &token(0, "a"));
        let _c1 = hub.publish("a1", &token(1, "b"));
        let clean = hub.publish("a1", &token(2, "c")); // overflow

        assert_eq!(clean, 0, "the overflowing publish admitted no clean delivery");
        let sub = hub.subscriber("a1", s).expect("sub");
        assert!(sub.is_degraded(), "overflow marks degraded");
        assert_eq!(sub.dropped_count(), 1);

        // Buffer holds the *newest* window: seq 1 and 2 (seq 0 evicted).
        let frames = hub.drain("a1", s).expect("drain");
        let seqs: Vec<u64> = frames.iter().map(|f| f.seq).collect();
        assert_eq!(seqs, vec![1, 2], "oldest frame was coalesced away");
    }

    #[test]
    fn degraded_is_per_subscriber() {
        let mut hub = StreamHub::new(1);
        let slow = hub.subscribe("a1");
        let fast = hub.subscribe("a1");

        let _c0 = hub.publish("a1", &token(0, "a"));
        // Fast subscriber drains, slow one does not.
        let _drained = hub.drain("a1", fast).expect("fast drain");
        let _c1 = hub.publish("a1", &token(1, "b")); // slow overflows, fast is clear

        assert!(hub.subscriber("a1", slow).expect("slow").is_degraded());
        assert!(!hub.subscriber("a1", fast).expect("fast").is_degraded());
    }

    #[test]
    fn mark_reconciled_clears_degraded() {
        let mut hub = StreamHub::new(1);
        let s = hub.subscribe("a1");
        let _c0 = hub.publish("a1", &token(0, "a"));
        let _c1 = hub.publish("a1", &token(1, "b")); // overflow → degraded

        assert!(hub.subscriber("a1", s).expect("sub").is_degraded());
        assert!(hub.mark_reconciled("a1", s));
        let sub = hub.subscriber("a1", s).expect("sub");
        assert!(!sub.is_degraded(), "reconcile clears degraded");
        assert_eq!(sub.dropped_count(), 0, "reconcile resets the dropped counter");
    }

    #[test]
    fn publish_to_agent_without_subscribers_is_noop() {
        let mut hub = StreamHub::new(8);
        assert_eq!(hub.publish("ghost", &token(0, "x")), 0);
    }

    #[test]
    fn unsubscribe_removes_subscriber() {
        let mut hub = StreamHub::new(8);
        let s = hub.subscribe("a1");
        assert_eq!(hub.subscriber_count("a1"), 1);

        assert!(hub.unsubscribe("a1", s));
        assert_eq!(hub.subscriber_count("a1"), 0);
        assert!(!hub.unsubscribe("a1", s), "second unsubscribe is a no-op");
    }

    #[test]
    fn zero_capacity_subscriber_drops_every_frame() {
        let mut hub = StreamHub::new(0);
        let s = hub.subscribe("a1");
        let clean = hub.publish("a1", &token(0, "x"));
        assert_eq!(clean, 0);
        let sub = hub.subscriber("a1", s).expect("sub");
        assert!(sub.is_degraded());
        assert_eq!(sub.buffered(), 0);
    }
}
