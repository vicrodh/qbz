//! Tapped Audio Source
//!
//! A wrapper around any rodio Source that intercepts samples for visualization
//! without affecting audio playback. The tap is completely transparent to the
//! audio pipeline.

use std::sync::Arc;
use std::time::Duration;
use rodio::Source;

use super::RingBuffer;

/// Wraps a Source and sends samples to a ring buffer for visualization
pub struct TappedSource<S>
where
    S: Source<Item = i16>,
{
    inner: S,
    ring_buffer: Arc<RingBuffer>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl<S> TappedSource<S>
where
    S: Source<Item = i16>,
{
    /// Create a new TappedSource wrapping the given source
    pub fn new(
        source: S,
        ring_buffer: Arc<RingBuffer>,
        enabled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            inner: source,
            ring_buffer,
            enabled,
        }
    }
}

impl<S> Iterator for TappedSource<S>
where
    S: Source<Item = i16>,
{
    type Item = i16;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;

        // Only send to visualizer if enabled - this is a fast atomic check
        if self.enabled.load(std::sync::atomic::Ordering::Relaxed) {
            // Convert i16 to f32 normalized to [-1, 1]
            // This is the only computation we do in the hot path
            let normalized = sample as f32 / i16::MAX as f32;
            self.ring_buffer.push(normalized);
        }

        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for TappedSource<S>
where
    S: Source<Item = i16>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_tapped_source_passes_through() {
        let samples: Vec<i16> = vec![1000, 2000, 3000, -1000, -2000];
        let source = SamplesBuffer::new(1, 44100, samples.clone());

        let ring_buffer = Arc::new(RingBuffer::new(16));
        let enabled = Arc::new(AtomicBool::new(true));

        let tapped = TappedSource::new(source, ring_buffer, enabled);
        let output: Vec<i16> = tapped.collect();

        // Samples should pass through unchanged
        assert_eq!(output, samples);
    }

    #[test]
    fn test_tapped_source_fills_ring_buffer() {
        let samples: Vec<i16> = vec![i16::MAX, 0, i16::MIN];
        let source = SamplesBuffer::new(1, 44100, samples);

        let ring_buffer = Arc::new(RingBuffer::new(16));
        let enabled = Arc::new(AtomicBool::new(true));

        let tapped = TappedSource::new(source, ring_buffer.clone(), enabled);
        let _: Vec<i16> = tapped.collect();

        // Check ring buffer received normalized samples
        let mut snapshot = [0.0f32; 3];
        ring_buffer.snapshot(&mut snapshot);

        assert!((snapshot[0] - 1.0).abs() < 0.001); // i16::MAX -> ~1.0
        assert!((snapshot[1] - 0.0).abs() < 0.001); // 0 -> 0.0
        assert!((snapshot[2] - (-1.0)).abs() < 0.01); // i16::MIN -> ~-1.0
    }
}
