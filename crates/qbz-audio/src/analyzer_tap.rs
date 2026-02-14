//! Analyzer tap — captures audio samples for loudness analysis.
//!
//! Sits in the audio pipeline as a transparent `Source<Item = f32>` wrapper.
//! Batches samples and sends them to the loudness analyzer thread via a bounded
//! channel. Uses `try_send` so it never blocks the audio thread — if the channel
//! is full, the batch is silently dropped (graceful degradation).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::Duration;

use rodio::Source;

/// Messages sent from the audio pipeline to the loudness analyzer thread.
pub enum AnalyzerMessage {
    /// A batch of interleaved f32 samples.
    Samples(Vec<f32>),
    /// A new track has started — reset analyzer state.
    NewTrack {
        track_id: u64,
        sample_rate: u32,
        channels: u16,
        target_lufs: f32,
        /// Shared gain atomic — analyzer writes, DynamicAmplify reads.
        gain_atomic: Arc<AtomicU32>,
    },
    /// Seek occurred — reset accumulated samples but keep current gain.
    Reset,
    /// Shut down the analyzer thread.
    Shutdown,
}

const BATCH_SIZE: usize = 4096;

pub struct AnalyzerTap<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    sender: SyncSender<AnalyzerMessage>,
    enabled: Arc<AtomicBool>,
    buffer: Vec<f32>,
}

impl<S> AnalyzerTap<S>
where
    S: Source<Item = f32>,
{
    pub fn new(source: S, sender: SyncSender<AnalyzerMessage>, enabled: Arc<AtomicBool>) -> Self {
        Self {
            inner: source,
            sender,
            enabled,
            buffer: Vec::with_capacity(BATCH_SIZE),
        }
    }

    #[inline]
    fn flush_if_full(&mut self) {
        if self.buffer.len() >= BATCH_SIZE {
            let batch = std::mem::replace(&mut self.buffer, Vec::with_capacity(BATCH_SIZE));
            // Non-blocking send — drop batch if channel is full
            let _ = self.sender.try_send(AnalyzerMessage::Samples(batch));
        }
    }
}

impl<S> Iterator for AnalyzerTap<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;

        if self.enabled.load(Ordering::Relaxed) {
            self.buffer.push(sample);
            self.flush_if_full();
        }

        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for AnalyzerTap<S>
where
    S: Source<Item = f32>,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}
