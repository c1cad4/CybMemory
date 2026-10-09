//! Simple append-only journal with deterministic text serialization.
//! This is a prototype; it does not provide durable fsync or cryptographic integrity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    pub kind: String,
    pub payload: String,
}
#[derive(Default)]
pub struct Journal {
    entries: Vec<Entry>,
}
impl Journal {
    pub fn append(&mut self, kind: &str, payload: &str) -> Result<u64, &'static str> {
        if kind.is_empty() || payload.is_empty() {
            return Err("kind and payload must not be empty");
        }
        let sequence = self.entries.len() as u64 + 1;
        self.entries.push(Entry {
            sequence,
            kind: kind.into(),
            payload: payload.into(),
        });
        Ok(sequence)
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn find_kind(&self, kind: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_ordered_events() {
        let mut journal = Journal::default();
        assert_eq!(journal.append("task", "started"), Ok(1));
        assert_eq!(journal.append("review", "accepted"), Ok(2));
        assert_eq!(journal.entries()[1].sequence, 2);
        assert_eq!(journal.find_kind("review").len(), 1);
    }
    #[test]
    fn rejects_empty_events() {
        let mut journal = Journal::default();
        assert!(journal.append("", "payload").is_err());
        assert!(journal.entries().is_empty());
    }
}
