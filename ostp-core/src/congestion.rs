//! Congestion control for the OSTP protocol.
//!
//! Implements a simplified BBR-inspired algorithm that estimates bottleneck
//! bandwidth and minimum RTT to determine the optimal sending rate.
//! This replaces the fixed `retransmit_budget = 8` with an adaptive
//! congestion window that responds to network conditions.
//!
//! RTO calculation follows RFC 6298:
//!   SRTT = (1 - α) * SRTT + α * RTT       (α = 1/8)
//!   RTTVAR = (1 - β) * RTTVAR + β * |SRTT - RTT|  (β = 1/4)
//!   RTO = SRTT + 4 * RTTVAR
//!   clamped to [RTO_MIN, RTO_MAX]

use std::time::{Duration, Instant};

/// Congestion control state for a single OSTP session.
pub struct CongestionController {
    /// Current congestion window in bytes (how much can be in-flight)
    cwnd: u64,
    /// Slow-start threshold in bytes
    ssthresh: u64,
    /// Current phase
    phase: Phase,
    /// Minimum RTT observed (for BBR-style bandwidth estimation)
    min_rtt: Duration,
    /// Smoothed RTT (RFC 6298 SRTT)
    srtt: Duration,
    /// RTT variance (RFC 6298 RTTVAR)
    rttvar: Duration,
    /// Whether we have received a first RTT sample
    rtt_initialized: bool,
    /// Bytes currently in flight (unacknowledged)
    bytes_in_flight: u64,
    /// Total bytes acknowledged (for bandwidth estimation)
    total_acked: u64,
    /// Last time we received an ACK
    last_ack_time: Instant,
    /// Number of loss events in the current window
    loss_count: u32,
    /// Pacing rate: bytes per second
    pacing_rate: u64,
    /// MTU estimate (used for cwnd → packet count conversion)
    mtu: u64,
    /// Min RTT expiry: re-probe after 10 seconds
    min_rtt_stamp: Instant,
    /// Loss events counted toward SLOW_START_LOSS_TOLERANCE within the
    /// current SLOW_START_LOSS_WINDOW (see on_loss's SlowStart arm).
    slow_start_losses: u32,
    /// Start of the current loss-tolerance window.
    slow_start_loss_window_start: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Exponential growth until loss or ssthresh
    SlowStart,
    /// Probe bandwidth: additive increase
    ProbeBandwidth,
}

/// Initial congestion window: 32 packets × MTU (IW10 is too conservative for modern links)
const INITIAL_CWND_PACKETS: u64 = 32;
/// Minimum cwnd: 2 packets
const MIN_CWND_PACKETS: u64 = 2;
/// Min RTT expiry window (after which we re-probe)
const MIN_RTT_EXPIRY: Duration = Duration::from_secs(10);
/// Minimum RTO (RFC 6298: 1s in TCP; we use 50ms since we own the protocol)
const RTO_MIN: Duration = Duration::from_millis(50);
/// Maximum RTO
const RTO_MAX: Duration = Duration::from_secs(16);
/// Initial RTT estimate — 30 ms is reasonable for a well-connected VPN server.
/// Will be replaced by first real measurement within milliseconds.
const INITIAL_RTT: Duration = Duration::from_millis(30);

/// Isolated packet loss during slow start (a single dropped frame from
/// wireless noise, a brief LTE handover blip, etc.) is normal on real
/// mobile/Wi-Fi links and does NOT mean the link is congested. The previous
/// behavior exited slow start and halved cwnd on the very FIRST loss, which
/// on any link with a non-zero background loss rate permanently downgrades
/// the session from exponential growth to linear (+1 MTU/RTT) ProbeBandwidth
/// growth within the first few RTTs - turning what should be a sub-second
/// ramp-up into tens of seconds to minutes before throughput opens up
/// (observed as: a trickle of KB/s, then a sudden jump once cwnd finally
/// claws back up). Only treat loss as a real congestion signal - and pay
/// the full slow-start-exit + halving cost - once this many losses land
/// within SLOW_START_LOSS_WINDOW.
const SLOW_START_LOSS_TOLERANCE: u32 = 3;
/// Window within which SLOW_START_LOSS_TOLERANCE losses must land to count
/// as sustained (rather than isolated) loss. Roughly a few RTTs on a
/// well-connected link, generous on a slow one.
const SLOW_START_LOSS_WINDOW: Duration = Duration::from_millis(500);

impl CongestionController {
    pub fn new(mtu: u64) -> Self {
        let now = Instant::now();
        let initial_cwnd = INITIAL_CWND_PACKETS * mtu;
        // Initial pacing: deliver cwnd in ~2 RTTs to fill the pipe quickly
        let initial_pacing = initial_cwnd * 1_000_000 / INITIAL_RTT.as_micros().max(1) as u64;
        Self {
            cwnd: initial_cwnd,
            ssthresh: u64::MAX,
            phase: Phase::SlowStart,
            min_rtt: INITIAL_RTT,
            srtt: INITIAL_RTT,
            rttvar: INITIAL_RTT / 2,
            rtt_initialized: false,
            bytes_in_flight: 0,
            total_acked: 0,
            last_ack_time: now,
            loss_count: 0,
            pacing_rate: initial_pacing,
            mtu,
            min_rtt_stamp: now,
            slow_start_losses: 0,
            slow_start_loss_window_start: now,
        }
    }

    /// Returns the current congestion window in bytes.
    pub fn cwnd(&self) -> u64 {
        self.cwnd
    }

    /// Returns the current congestion window in packets.
    pub fn cwnd_packets(&self) -> usize {
        (self.cwnd / self.mtu).max(MIN_CWND_PACKETS) as usize
    }

    /// Returns the current pacing rate in bytes/sec.
    pub fn pacing_rate(&self) -> u64 {
        self.pacing_rate
    }

    /// Returns the smoothed RTT estimate (SRTT).
    pub fn smoothed_rtt(&self) -> Duration {
        self.srtt
    }

    /// Returns the adaptive RTO computed per RFC 6298:
    ///   RTO = SRTT + 4 * RTTVAR, clamped to [RTO_MIN, RTO_MAX].
    ///
    /// This replaces the static `rto_ms` field in ProtocolMachine so that
    /// retransmit timers automatically track changing network conditions.
    pub fn rto(&self) -> Duration {
        let rttvar4 = self.rttvar.saturating_mul(4);
        let rto = self.srtt.saturating_add(rttvar4);
        rto.clamp(RTO_MIN, RTO_MAX)
    }

    /// Returns how many bytes can still be sent.
    pub fn available_cwnd(&self) -> u64 {
        self.cwnd.saturating_sub(self.bytes_in_flight)
    }

    /// Returns the recommended retransmit budget per tick.
    pub fn retransmit_budget(&self) -> usize {
        // Allow retransmitting up to 1/4 of the cwnd in packets per tick
        let budget = (self.cwnd_packets() / 4).max(2);
        budget.min(64) // cap at 64 to prevent burst
    }

    /// Check whether we can send more data.
    pub fn can_send(&self) -> bool {
        self.bytes_in_flight < self.cwnd
    }

    /// Record that we sent `bytes` of data.
    pub fn on_send(&mut self, bytes: u64) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_add(bytes);
    }

    /// Record that `bytes` were acknowledged but WITHOUT a usable RTT sample
    /// (e.g. every acked frame was retransmitted, so Karn's algorithm forbids
    /// measuring RTT from it). The window still advances; only the RTT estimator
    /// is left untouched.
    pub fn on_ack_no_rtt(&mut self, bytes: u64) {
        let now = Instant::now();
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes);
        self.total_acked = self.total_acked.saturating_add(bytes);
        self.grow_window(bytes);
        self.update_pacing_rate();
        self.last_ack_time = now;
    }

    /// Record that `bytes` were acknowledged with the given RTT sample.
    pub fn on_ack(&mut self, bytes: u64, rtt: Duration) {
        let now = Instant::now();
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes);
        self.total_acked = self.total_acked.saturating_add(bytes);

        // Update RTT measurements
        self.update_rtt(rtt, now);

        self.grow_window(bytes);
        self.update_pacing_rate();
        self.last_ack_time = now;
    }

    /// Congestion-window growth shared by both ACK paths (slow start / probe).
    fn grow_window(&mut self, bytes: u64) {
        // State machine
        match self.phase {
            Phase::SlowStart => {
                // Exponential growth: increase cwnd by acked bytes (doubles per RTT)
                self.cwnd = self.cwnd.saturating_add(bytes);
                if self.cwnd >= self.ssthresh {
                    self.phase = Phase::ProbeBandwidth;
                    tracing::debug!(cwnd = self.cwnd, "congestion: exiting slow start");
                }
            }
            Phase::ProbeBandwidth => {
                // TCP Reno Additive Increase: increase cwnd by ~1 MTU per RTT
                self.cwnd = self.cwnd.saturating_add(bytes * self.mtu / self.cwnd.max(1));
            }
        }
    }

    /// Record a loss event.
    pub fn on_loss(&mut self, bytes_lost: u64) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes_lost);
        self.loss_count += 1;

        match self.phase {
            Phase::SlowStart => {
                let now = Instant::now();
                if now.duration_since(self.slow_start_loss_window_start) > SLOW_START_LOSS_WINDOW {
                    // Previous window's losses have aged out - this loss starts a fresh count.
                    self.slow_start_losses = 0;
                    self.slow_start_loss_window_start = now;
                }
                self.slow_start_losses += 1;

                if self.slow_start_losses >= SLOW_START_LOSS_TOLERANCE {
                    // Sustained loss within the window: treat as real congestion.
                    // Exit slow start, set ssthresh to half of cwnd.
                    self.ssthresh = self.cwnd / 2;
                    self.cwnd = self.ssthresh.max(MIN_CWND_PACKETS * self.mtu);
                    self.phase = Phase::ProbeBandwidth;
                    tracing::debug!(cwnd = self.cwnd, ssthresh = self.ssthresh, "congestion: sustained loss during slow start, exiting");
                } else {
                    // Isolated loss: likely non-congestive noise. Take a mild,
                    // temporary haircut but keep exponential growth going -
                    // don't throw away slow start over a single dropped frame.
                    self.cwnd = (self.cwnd * 8 / 10).max(MIN_CWND_PACKETS * self.mtu);
                    tracing::debug!(cwnd = self.cwnd, count = self.slow_start_losses, "congestion: isolated loss during slow start, staying in slow start");
                }
            }
            Phase::ProbeBandwidth => {
                // Multiplicative decrease: cwnd *= 0.7 (BBR-style, less aggressive than Cubic's 0.5)
                self.cwnd = (self.cwnd * 7 / 10).max(MIN_CWND_PACKETS * self.mtu);
                tracing::debug!(cwnd = self.cwnd, "congestion: loss, cwnd reduced");
            }
        }

        self.update_pacing_rate();
    }

    // ── Private ──────────────────────────────────────────────────────────────

    fn update_rtt(&mut self, rtt: Duration, now: Instant) {
        // Update windowed minimum RTT (for pacing)
        if rtt < self.min_rtt || now.duration_since(self.min_rtt_stamp) >= MIN_RTT_EXPIRY {
            self.min_rtt = rtt;
            self.min_rtt_stamp = now;
        }

        // Update SRTT and RTTVAR per RFC 6298
        if !self.rtt_initialized {
            // First measurement: initialize directly
            self.srtt = rtt;
            self.rttvar = rtt / 2;
            self.rtt_initialized = true;
        } else {
            // RTTVAR = (3/4) * RTTVAR + (1/4) * |SRTT - R|
            let diff = if rtt > self.srtt {
                rtt - self.srtt
            } else {
                self.srtt - rtt
            };
            // Integer-safe: RTTVAR = RTTVAR - RTTVAR/4 + diff/4
            self.rttvar = self.rttvar
                .saturating_sub(self.rttvar / 4)
                .saturating_add(diff / 4);

            // SRTT = (7/8) * SRTT + (1/8) * R
            self.srtt = self.srtt
                .saturating_sub(self.srtt / 8)
                .saturating_add(rtt / 8);
        }

        tracing::trace!(
            srtt_ms = self.srtt.as_millis(),
            rttvar_ms = self.rttvar.as_millis(),
            rto_ms = self.rto().as_millis(),
            "congestion: RTT updated"
        );
    }

    fn update_pacing_rate(&mut self) {
        // Pacing rate = cwnd / min_rtt (delivery rate target)
        let rtt_us = self.min_rtt.as_micros().max(1) as u64;
        self.pacing_rate = self.cwnd * 1_000_000 / rtt_us;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let cc = CongestionController::new(1200);
        assert_eq!(cc.cwnd(), 32 * 1200); // 32 * 1200
        assert!(cc.can_send());
        assert_eq!(cc.cwnd_packets(), 32);
    }

    #[test]
    fn test_slow_start_growth() {
        let mut cc = CongestionController::new(1200);
        let initial = cc.cwnd();
        cc.on_send(1200);
        cc.on_ack(1200, Duration::from_millis(50));
        assert!(cc.cwnd() > initial);
    }

    #[test]
    fn test_loss_reduces_cwnd() {
        let mut cc = CongestionController::new(1200);
        let initial = cc.cwnd();
        cc.on_loss(1200);
        assert!(cc.cwnd() < initial);
    }

    #[test]
    fn test_isolated_slow_start_loss_does_not_exit_slow_start() {
        // A single dropped packet (wireless noise, a brief handover blip) is
        // normal on real links and must not permanently downgrade the
        // session from exponential to linear growth.
        let mut cc = CongestionController::new(1200);
        cc.on_loss(1200);
        assert_eq!(cc.phase, Phase::SlowStart, "one isolated loss must not exit slow start");

        // It should still shrink the window somewhat (not ignored entirely),
        // just far less punishing than the sustained-congestion case.
        let after_one = cc.cwnd();
        assert!(after_one < INITIAL_CWND_PACKETS * 1200);
    }

    #[test]
    fn test_sustained_slow_start_loss_exits_slow_start() {
        // Losses landing close together (within SLOW_START_LOSS_WINDOW) are
        // a real congestion signal and must still trigger the harsher
        // exit-slow-start + halve response.
        let mut cc = CongestionController::new(1200);
        for _ in 0..SLOW_START_LOSS_TOLERANCE {
            cc.on_loss(1200);
        }
        assert_eq!(cc.phase, Phase::ProbeBandwidth, "sustained loss must exit slow start");
    }

    #[test]
    fn test_slow_start_loss_window_resets_after_expiry() {
        // Two losses far enough apart (window expired between them) must
        // each be treated as isolated, not accumulated toward the sustained-
        // loss threshold.
        let mut cc = CongestionController::new(1200);
        cc.on_loss(1200);
        assert_eq!(cc.phase, Phase::SlowStart);

        // Simulate the window having expired by resetting its start
        // directly (std::thread::sleep in a unit test would be flaky/slow).
        cc.slow_start_loss_window_start = Instant::now() - SLOW_START_LOSS_WINDOW - Duration::from_millis(1);
        cc.on_loss(1200);
        assert_eq!(cc.phase, Phase::SlowStart, "a loss after the window expired must restart the count, not accumulate");
        assert_eq!(cc.slow_start_losses, 1);
    }

    #[test]
    fn test_can_send_limits() {
        let mut cc = CongestionController::new(1200);
        // Send until cwnd is exhausted
        for _ in 0..32 {
            cc.on_send(1200);
        }
        assert!(!cc.can_send()); // cwnd exhausted
    }

    #[test]
    fn test_retransmit_budget() {
        let cc = CongestionController::new(1200);
        let budget = cc.retransmit_budget();
        assert!(budget >= 2);
        assert!(budget <= 64);
    }

    #[test]
    fn test_rtt_tracking_first_sample() {
        let mut cc = CongestionController::new(1200);
        cc.on_send(1200);
        cc.on_ack(1200, Duration::from_millis(25));
        // After first sample: SRTT = 25ms, RTTVAR = 12ms
        assert_eq!(cc.smoothed_rtt(), Duration::from_millis(25));
    }

    #[test]
    fn test_rto_rfc6298() {
        let mut cc = CongestionController::new(1200);
        // After first sample with RTT=50ms: SRTT=50ms, RTTVAR=25ms, RTO=150ms
        cc.on_send(1200);
        cc.on_ack(1200, Duration::from_millis(50));
        let rto = cc.rto();
        // RTO = 50 + 4*25 = 150ms; clamped to [50ms, 16s]
        assert!(rto >= RTO_MIN);
        assert!(rto <= RTO_MAX);
        assert_eq!(rto, Duration::from_millis(150));
    }

    #[test]
    fn test_on_ack_no_rtt_grows_window_without_touching_srtt() {
        let mut cc = CongestionController::new(1200);
        // Establish a known SRTT with a real sample.
        cc.on_send(1200);
        cc.on_ack(1200, Duration::from_millis(40));
        let srtt_before = cc.smoothed_rtt();
        let cwnd_before = cc.cwnd();

        // A Karn's-algorithm ACK (all acked frames were retransmitted): window
        // must advance, RTT estimate must be untouched.
        cc.on_send(1200);
        cc.on_ack_no_rtt(1200);
        assert!(cc.cwnd() > cwnd_before, "cwnd should still grow on a no-RTT ack");
        assert_eq!(cc.smoothed_rtt(), srtt_before, "SRTT must not move on a no-RTT ack");
    }

    #[test]
    fn test_rto_clamp_min() {
        let cc = CongestionController::new(1200);
        // Even with no RTT samples, RTO should not go below RTO_MIN
        assert!(cc.rto() >= RTO_MIN);
    }

    #[test]
    fn test_rto_adapts_after_multiple_samples() {
        let mut cc = CongestionController::new(1200);
        // Feed several consistent RTT samples
        for _ in 0..8 {
            cc.on_send(1200);
            cc.on_ack(1200, Duration::from_millis(20));
        }
        // After convergence, RTTVAR should be small → RTO close to SRTT + small margin
        let rto = cc.rto();
        // Should be well below 100ms (the old hardcoded default)
        assert!(rto < Duration::from_millis(200));
        assert!(rto >= RTO_MIN);
    }
}
