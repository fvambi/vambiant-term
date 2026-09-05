//! The [`TerminalCore`] trait — the only surface the rest of the workspace sees.

use crate::cell::{CellSnapshot, GridSize};
use crate::damage::DamageSet;
use crate::error::CoreError;
use crate::event::TermEvent;
use crate::key::KeyEvent;

/// A VT state machine: bytes in, grid + damage out.
///
/// Implementations are single-threaded by design; the daemon owns one per
/// session on its reader thread (docs/02 §4). Damage is *cumulative* between
/// calls to [`TerminalCore::take_damage`], which is how the flush step
/// coalesces at display cadence.
pub trait TerminalCore {
    /// Feed raw PTY bytes through the parser. Must never allocate per byte.
    fn advance(&mut self, bytes: &[u8]);

    /// Resize the grid, reflowing scrollback where the backend supports it.
    fn resize(&mut self, size: GridSize) -> Result<(), CoreError>;

    /// Current grid dimensions.
    fn size(&self) -> GridSize;

    /// Take and reset the accumulated damage since the previous call.
    fn take_damage(&mut self) -> DamageSet;

    /// Snapshot the visible grid (plus cursor) for a viewer.
    fn snapshot(&mut self) -> CellSnapshot;

    /// Window title set through OSC 0/2, if any.
    fn title(&self) -> Option<String>;

    /// Bytes the terminal wants written back to the PTY (query responses
    /// such as DA, DSR, XTGETTCAP). Drained by the reader thread after every
    /// [`TerminalCore::advance`].
    fn take_responses(&mut self) -> Vec<u8>;

    /// Out-of-band events raised while parsing (bell, title, cwd, clipboard
    /// writes). Drained alongside [`TerminalCore::take_responses`].
    fn take_events(&mut self) -> Vec<TermEvent>;

    /// Encode a key event into the bytes the running program expects, honouring
    /// the terminal's current keyboard modes (application cursor keys,
    /// `modifyOtherKeys`, kitty keyboard flags). Empty when the key produces
    /// nothing (a lone modifier, a release the program did not ask for).
    fn encode_key(&mut self, event: &KeyEvent) -> Vec<u8>;

    /// OSC 8 hyperlink target under a visible cell, if any. Looked up on
    /// demand (hover/click) rather than carried per cell across the snapshot.
    fn hyperlink_at(&self, row: u16, col: u16) -> Option<String>;
}
