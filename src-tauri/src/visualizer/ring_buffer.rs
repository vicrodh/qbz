//! Lock-free Ring Buffer for Audio Sample Capture
//!
//! This implementation is designed to be called from the audio thread without
//! any blocking operations. The audio thread is the only writer, and the
//! visualizer thread is the only reader.
//!
//! CRITICAL: This must not affect bit-perfect playback in any way.
//! - No locks or mutexes
//! - No allocations
//! - Minimal atomic operations

use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::UnsafeCell;

/// A lock-free single-producer single-consumer ring buffer
///
/// # Safety
/// This is safe because:
/// - Only one thread (audio) calls `push`
/// - Only one thread (visualizer) calls `snapshot`
/// - The buffer size is fixed at creation
pub struct RingBuffer {
    /// The actual sample storage
    buffer: UnsafeCell<Box<[f32]>>,
    /// Current write position (only modified by audio thread)
    write_pos: AtomicUsize,
    /// Buffer size
    size: usize,
}

// Safety: RingBuffer is designed for single-producer single-consumer use
unsafe impl Sync for RingBuffer {}
unsafe impl Send for RingBuffer {}

impl RingBuffer {
    /// Create a new ring buffer with the specified size
    pub fn new(size: usize) -> Self {
        Self {
            buffer: UnsafeCell::new(vec![0.0; size].into_boxed_slice()),
            write_pos: AtomicUsize::new(0),
            size,
        }
    }

    /// Push a sample to the buffer (called from audio thread)
    ///
    /// This is completely lock-free and will not block.
    /// If the visualizer is reading, it might get slightly stale data,
    /// which is perfectly fine for visualization purposes.
    #[inline]
    pub fn push(&self, sample: f32) {
        let pos = self.write_pos.fetch_add(1, Ordering::Relaxed) % self.size;

        // Safety: We're the only writer, and readers can tolerate slightly stale data
        unsafe {
            let buffer = &mut *self.buffer.get();
            buffer[pos] = sample;
        }
    }

    /// Take a snapshot of the most recent samples
    ///
    /// Copies the last `dest.len()` samples into the provided slice.
    /// The samples are ordered from oldest to newest.
    pub fn snapshot(&self, dest: &mut [f32]) {
        let write_pos = self.write_pos.load(Ordering::Relaxed);
        let len = dest.len().min(self.size);

        // Safety: We're the only reader, and the writer only increments write_pos
        unsafe {
            let buffer = &*self.buffer.get();

            for i in 0..len {
                // Calculate the index for the i-th oldest sample
                // We want samples from (write_pos - len) to (write_pos - 1)
                let idx = (write_pos + self.size - len + i) % self.size;
                dest[i] = buffer[idx];
            }
        }
    }

    /// Get the current write position (for debugging)
    #[allow(dead_code)]
    pub fn write_position(&self) -> usize {
        self.write_pos.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let buffer = RingBuffer::new(8);

        // Push some samples
        for i in 0..5 {
            buffer.push(i as f32);
        }

        // Take a snapshot
        let mut snapshot = [0.0f32; 4];
        buffer.snapshot(&mut snapshot);

        // Should get the last 4 samples: 1, 2, 3, 4
        assert_eq!(snapshot, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_ring_buffer_wrap_around() {
        let buffer = RingBuffer::new(4);

        // Push more samples than buffer size
        for i in 0..10 {
            buffer.push(i as f32);
        }

        // Take a snapshot
        let mut snapshot = [0.0f32; 4];
        buffer.snapshot(&mut snapshot);

        // Should get the last 4 samples: 6, 7, 8, 9
        assert_eq!(snapshot, [6.0, 7.0, 8.0, 9.0]);
    }
}
