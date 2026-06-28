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

    /// Record that `bytes` were acknowledged with the given RTT sample.
    pub fn on_ack(&mut self, bytes: u64, rtt: Duration) {
        let now = Instant::now();
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes);
        self.total_acked = self.total_acked.saturating_add(bytes);

        // Update RTT measurements
        self.update_rtt(rtt, now);

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

        self.update_pacing_rate();
        self.last_ack_time = now;
    }

    /// Record a loss event.
    pub fn on_loss(&mut self, bytes_lost: u64) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes_lost);
        self.loss_count += 1;

        match self.phase {
            Phase::SlowStart => {
                // Exit slow start, set ssthresh to half of cwnd
                self.ssthresh = self.cwnd / 2;
                self.cwnd = self.ssthresh.max(MIN_CWND_PACKETS * self.mtu);
                self.phase = Phase::ProbeBandwidth;
                tracing::debug!(cwnd = self.cwnd, ssthresh = self.ssthresh, "congestion: loss during slow start");
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
