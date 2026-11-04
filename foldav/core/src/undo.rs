//! What file operations replaced, so they can be undone: per step (one user action), what
//! each change was. Notav's `undo.rs`, for moves and deletions.

/// Steps kept; older ones are forgotten.
const KEEP: usize = 100;

/// One change of a step.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Change {
    /// A file or folder of `account` was moved (or renamed) from `from` to `to`.
    Moved {
        account: i64,
        from: String,
        to: String,
    },
    /// The file or folder at `path` of `account` was deleted: it is in the trash bin.
    Deleted { account: i64, path: String },
}

#[derive(Default)]
pub(crate) struct Journal {
    steps: Vec<(u64, Vec<Change>)>,
    next: u64,
}

impl Journal {
    /// Keep a step; its id, None when it changed nothing.
    pub fn record(&mut self, changes: Vec<Change>) -> Option<u64> {
        if changes.is_empty() {
            return None;
        }
        self.next += 1;
        self.steps.push((self.next, changes));
        if self.steps.len() > KEEP {
            self.steps.remove(0);
        }
        Some(self.next)
    }

    /// Remove step `id` (else the latest) and return its changes.
    pub fn take(&mut self, id: Option<u64>) -> Option<Vec<Change>> {
        let i = match id {
            Some(id) => self.steps.iter().position(|s| s.0 == id)?,
            None => self.steps.len().checked_sub(1)?,
        };
        Some(self.steps.remove(i).1)
    }

    pub fn clear(&mut self) {
        self.steps.clear();
    }
}
