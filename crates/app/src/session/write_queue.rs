//! Bounded PTY write queue shared by the model owner, the UI and the writer.
//!
//! Protocol replies have a 16 KiB reserve separate from the user-input queue
//! (64 KiB typed, 1 MiB explicit paste). Enqueueing never blocks. The writer
//! services up to four replies, then one input chunk, when both are ready.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

pub const REPLY_RESERVE_BYTES: usize = 16 * 1024;
pub const TYPED_INPUT_BYTES: usize = 64 * 1024;
pub const PASTE_INPUT_BYTES: usize = 1024 * 1024;
const REPLIES_PER_INPUT: u8 = 4;
/// Paste data is handed to the writer in bounded chunks so replies interleave.
const INPUT_CHUNK_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Typed,
    Paste,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error("the terminal is busy; input was not accepted")]
    InputBusy,
    #[error("the child is not reading protocol replies")]
    ReplyBackpressure,
    #[error("the session is closed")]
    Closed,
}

#[derive(Default)]
struct State {
    replies: VecDeque<Vec<u8>>,
    reply_bytes: usize,
    input: VecDeque<Vec<u8>>,
    input_bytes: usize,
    replies_since_input: u8,
    closed: bool,
}

/// Nonblocking producer side, blocking consumer side.
#[derive(Default)]
pub struct WriteQueue {
    state: Mutex<State>,
    ready: Condvar,
}

impl WriteQueue {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queue a protocol reply. Fails instead of blocking when the reserve is full.
    pub fn push_reply(&self, bytes: Vec<u8>) -> Result<(), QueueError> {
        let mut state = self.lock();
        if state.closed {
            return Err(QueueError::Closed);
        }
        if state.reply_bytes + bytes.len() > REPLY_RESERVE_BYTES {
            return Err(QueueError::ReplyBackpressure);
        }
        state.reply_bytes += bytes.len();
        state.replies.push_back(bytes);
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    /// Queue user input. Accepted input is never discarded; a full queue is reported.
    pub fn push_input(&self, bytes: &[u8], kind: InputKind) -> Result<(), QueueError> {
        let limit = match kind {
            InputKind::Typed => TYPED_INPUT_BYTES,
            InputKind::Paste => PASTE_INPUT_BYTES,
        };
        let mut state = self.lock();
        if state.closed {
            return Err(QueueError::Closed);
        }
        if state.input_bytes + bytes.len() > limit {
            return Err(QueueError::InputBusy);
        }
        state.input_bytes += bytes.len();
        state
            .input
            .extend(bytes.chunks(INPUT_CHUNK_BYTES).map(<[u8]>::to_vec));
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    /// Whether typed input would currently be refused.
    pub fn input_busy(&self) -> bool {
        self.lock().input_bytes >= TYPED_INPUT_BYTES
    }

    /// Stop accepting writes and wake the writer so it can exit.
    pub fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// Block until data is available. Returns `None` once closed; pending data
    /// is dropped on close because the child is being torn down.
    pub fn next(&self) -> Option<Vec<u8>> {
        let mut state = self.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(bytes) = take_next(&mut state) {
                return Some(bytes);
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

fn take_next(state: &mut State) -> Option<Vec<u8>> {
    let prefer_reply = state.replies_since_input < REPLIES_PER_INPUT || state.input.is_empty();
    if prefer_reply && let Some(reply) = state.replies.pop_front() {
        state.reply_bytes -= reply.len();
        state.replies_since_input += 1;
        return Some(reply);
    }
    let input = state.input.pop_front()?;
    state.input_bytes -= input.len();
    state.replies_since_input = 0;
    Some(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_reserve_is_enforced_without_blocking() {
        let queue = WriteQueue::default();
        queue
            .push_reply(vec![0; REPLY_RESERVE_BYTES])
            .expect("fits");
        assert_eq!(
            queue.push_reply(vec![0; 1]),
            Err(QueueError::ReplyBackpressure)
        );
    }

    #[test]
    fn typed_and_paste_limits_differ() {
        let queue = WriteQueue::default();
        assert_eq!(
            queue.push_input(&vec![0; TYPED_INPUT_BYTES + 1], InputKind::Typed),
            Err(QueueError::InputBusy)
        );
        queue
            .push_input(&vec![0; TYPED_INPUT_BYTES + 1], InputKind::Paste)
            .expect("paste fits");
        assert!(queue.input_busy());
    }

    #[test]
    fn four_replies_then_one_input_chunk() {
        let queue = WriteQueue::default();
        queue.push_input(b"i1", InputKind::Typed).expect("input");
        queue.push_input(b"i2", InputKind::Typed).expect("input");
        for n in 0..6u8 {
            queue.push_reply(vec![n]).expect("reply");
        }
        let order: Vec<Vec<u8>> = (0..8).filter_map(|_| queue.next()).collect();
        let expected: Vec<Vec<u8>> = vec![
            vec![0],
            vec![1],
            vec![2],
            vec![3],
            b"i1".to_vec(),
            vec![4],
            vec![5],
            b"i2".to_vec(),
        ];
        assert_eq!(order, expected);
    }

    #[test]
    fn close_wakes_consumer_and_rejects_writes() {
        let queue = std::sync::Arc::new(WriteQueue::default());
        let consumer = {
            let queue = queue.clone();
            std::thread::spawn(move || queue.next())
        };
        queue.close();
        assert_eq!(consumer.join().expect("join"), None);
        assert_eq!(
            queue.push_input(b"x", InputKind::Typed),
            Err(QueueError::Closed)
        );
    }

    #[test]
    fn paste_is_chunked_for_interleaving() {
        let queue = WriteQueue::default();
        queue
            .push_input(&vec![7; INPUT_CHUNK_BYTES * 2 + 1], InputKind::Paste)
            .expect("paste");
        queue.push_reply(b"r".to_vec()).expect("reply");
        assert_eq!(queue.next(), Some(b"r".to_vec()));
        assert_eq!(queue.next().map(|c| c.len()), Some(INPUT_CHUNK_BYTES));
    }
}
