//! Lock-free input buffer for high-performance input submission
//!
//! Uses crossbeam-channel for lock-free MPSC communication from
//! connection handlers to the game loop.

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

use crate::game::state::PlayerId;
use crate::net::protocol::PlayerInput;

/// Input message from a player connection
#[derive(Debug, Clone)]
pub struct InputMessage {
    pub player_id: PlayerId,
    pub input: PlayerInput,
}

/// Lock-free input buffer using bounded channel
///
/// Multiple connection handlers can submit inputs without blocking,
/// and the game loop drains all pending inputs at the start of each tick.
pub struct InputBuffer {
    /// Sender side - cloned to each connection handler
    sender: Sender<InputMessage>,
    /// Receiver side - used by game loop
    receiver: Receiver<InputMessage>,
    /// Buffer capacity for tracking
    capacity: usize,
}

impl InputBuffer {
    /// Create a new input buffer with given capacity
    ///
    /// Capacity should be large enough to handle burst inputs
    /// between game ticks (e.g., 1000 for 100+ players at 60 Hz client rate)
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = bounded(capacity);
        Self {
            sender,
            receiver,
            capacity,
        }
    }

    /// Create a new sender handle for a connection
    ///
    /// Each connection should hold its own sender clone
    pub fn sender(&self) -> InputSender {
        InputSender {
            sender: self.sender.clone(),
        }
    }

    /// Try to submit an input (non-blocking)
    ///
    /// Returns true if successful, false if buffer is full
    #[inline]
    pub fn try_submit(&self, player_id: PlayerId, input: PlayerInput) -> bool {
        self.sender
            .try_send(InputMessage { player_id, input })
            .is_ok()
    }

    /// Drain all pending inputs for this tick
    ///
    /// Called at the start of each game tick to process all inputs
    pub fn drain(&self) -> Vec<InputMessage> {
        self.receiver.try_iter().collect()
    }

    /// Get number of pending inputs
    #[inline]
    pub fn pending_count(&self) -> usize {
        self.receiver.len()
    }

    /// Check if buffer is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    /// Get buffer capacity
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Default for InputBuffer {
    fn default() -> Self {
        // Default capacity handles ~1000 inputs per tick
        // (100 players * 10 Hz average input rate)
        Self::new(1000)
    }
}

/// Clonable sender handle for connection handlers
#[derive(Clone)]
pub struct InputSender {
    sender: Sender<InputMessage>,
}

impl InputSender {
    /// Submit an input (non-blocking)
    ///
    /// Returns true if successful, false if buffer is full (backpressure)
    #[inline]
    pub fn try_send(&self, player_id: PlayerId, input: PlayerInput) -> Result<(), InputBufferError> {
        self.sender
            .try_send(InputMessage { player_id, input })
            .map_err(|e| match e {
                TrySendError::Full(_) => InputBufferError::Full,
                TrySendError::Disconnected(_) => InputBufferError::Disconnected,
            })
    }
}

/// Input buffer errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBufferError {
    /// Buffer is full (backpressure)
    Full,
    /// Channel disconnected (game loop stopped)
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::vec2::Vec2;
    use uuid::Uuid;

    fn create_test_input(sequence: u64) -> PlayerInput {
        PlayerInput {
            sequence,
            tick: sequence,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::new(0.0, 1.0),
            boost: false,
            fire: false,
            fire_released: false,
        }
    }

    #[test]
    fn test_input_buffer_submit_and_drain() {
        let buffer = InputBuffer::new(10);
        let player_id = Uuid::new_v4();

        // Submit some inputs
        assert!(buffer.try_submit(player_id, create_test_input(1)));
        assert!(buffer.try_submit(player_id, create_test_input(2)));
        assert!(buffer.try_submit(player_id, create_test_input(3)));

        assert_eq!(buffer.pending_count(), 3);

        // Drain all
        let inputs = buffer.drain();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].input.sequence, 1);
        assert_eq!(inputs[1].input.sequence, 2);
        assert_eq!(inputs[2].input.sequence, 3);

        // Should be empty now
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_input_buffer_backpressure() {
        let buffer = InputBuffer::new(2);
        let player_id = Uuid::new_v4();

        // Fill buffer
        assert!(buffer.try_submit(player_id, create_test_input(1)));
        assert!(buffer.try_submit(player_id, create_test_input(2)));

        // Third should fail (full)
        assert!(!buffer.try_submit(player_id, create_test_input(3)));

        // After drain, can submit again
        buffer.drain();
        assert!(buffer.try_submit(player_id, create_test_input(3)));
    }

    #[test]
    fn test_input_sender_clone() {
        let buffer = InputBuffer::new(10);
        let player_id = Uuid::new_v4();

        let sender1 = buffer.sender();
        let sender2 = buffer.sender();

        // Both senders can submit
        assert!(sender1.try_send(player_id, create_test_input(1)).is_ok());
        assert!(sender2.try_send(player_id, create_test_input(2)).is_ok());

        let inputs = buffer.drain();
        assert_eq!(inputs.len(), 2);
    }

    #[test]
    fn test_input_buffer_multiple_players() {
        let buffer = InputBuffer::new(100);

        let player1 = Uuid::new_v4();
        let player2 = Uuid::new_v4();
        let player3 = Uuid::new_v4();

        buffer.try_submit(player1, create_test_input(1));
        buffer.try_submit(player2, create_test_input(2));
        buffer.try_submit(player3, create_test_input(3));
        buffer.try_submit(player1, create_test_input(4));

        let inputs = buffer.drain();
        assert_eq!(inputs.len(), 4);

        // Check player IDs are preserved
        assert_eq!(inputs[0].player_id, player1);
        assert_eq!(inputs[1].player_id, player2);
        assert_eq!(inputs[2].player_id, player3);
        assert_eq!(inputs[3].player_id, player1);
    }

    #[test]
    fn test_input_buffer_default() {
        let buffer = InputBuffer::default();
        assert_eq!(buffer.capacity(), 1000);
    }
}
