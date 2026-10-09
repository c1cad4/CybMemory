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

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;

/// Stores journal entries as a length-delimited binary format.
/// This is a prototype; it does not fsync, lock, or authenticate records.
pub fn save(journal: &Journal, path: &Path) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    for entry in journal.entries() {
        file.write_all(&entry.sequence.to_le_bytes())?;
        for field in [&entry.kind, &entry.payload] {
            let bytes = field.as_bytes();
            let len = u32::try_from(bytes.len()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "field too long")
            })?;
            file.write_all(&len.to_le_bytes())?;
            file.write_all(bytes)?;
        }
    }
    file.flush()
}

/// Replays complete records, rejecting truncated or corrupt journals.
pub fn load(path: &Path) -> std::io::Result<Journal> {
    use std::io::{Error, ErrorKind, Read};
    let mut reader = BufReader::new(OpenOptions::new().read(true).open(path)?);
    let mut journal = Journal::default();
    loop {
        let mut sequence = [0u8; 8];
        let count = reader.read(&mut sequence[..1])?;
        if count == 0 {
            break;
        }
        reader.read_exact(&mut sequence[1..])?;
        let sequence = u64::from_le_bytes(sequence);
        let mut fields = Vec::with_capacity(2);
        for _ in 0..2 {
            let mut size = [0u8; 4];
            reader.read_exact(&mut size)?;
            let len = u32::from_le_bytes(size) as usize;
            if len > 1024 * 1024 {
                return Err(Error::new(ErrorKind::InvalidData, "oversized field"));
            }
            let mut bytes = vec![0; len];
            reader.read_exact(&mut bytes)?;
            fields
                .push(String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))?);
        }
        if sequence != journal.entries().len() as u64 + 1 {
            return Err(Error::new(ErrorKind::InvalidData, "invalid sequence"));
        }
        journal
            .append(&fields[0], &fields[1])
            .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
    }
    Ok(journal)
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn roundtrip_journal() {
        let path =
            std::env::temp_dir().join(format!("cybmemory-{}-roundtrip.bin", std::process::id()));
        let mut journal = Journal::default();
        journal.append("task", "started").unwrap();
        journal.append("review", "accepted").unwrap();
        save(&journal, &path).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(restored.entries(), journal.entries());
        std::fs::remove_file(path).unwrap();
    }
}

use sha2::{Digest, Sha256};

/// Store a checksummed snapshot via a sibling temporary file and rename.
/// The checksum detects accidental corruption, not malicious tampering.
/// This function does not lock concurrent writers.
pub fn save_checked(journal: &Journal, path: &Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let temporary = path.with_extension("cybtmp");
    if temporary == path {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "invalid temporary path",
        ));
    }
    save(journal, &temporary)?;
    let contents = std::fs::read(&temporary)?;
    let digest = Sha256::digest(&contents);
    let mut file = OpenOptions::new().append(true).open(&temporary)?;
    file.write_all(&digest)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

/// Read a checksummed snapshot, rejecting corruption before decoding entries.
pub fn load_checked(path: &Path) -> std::io::Result<Journal> {
    use std::io::{Error, ErrorKind};
    let bytes = std::fs::read(path)?;
    if bytes.len() < 32 {
        return Err(Error::new(ErrorKind::InvalidData, "missing checksum"));
    }
    let (payload, checksum) = bytes.split_at(bytes.len() - 32);
    if Sha256::digest(payload).as_slice() != checksum {
        return Err(Error::new(ErrorKind::InvalidData, "checksum mismatch"));
    }
    let temporary = path.with_extension("cybread");
    std::fs::write(&temporary, payload)?;
    let result = load(&temporary);
    let _ = std::fs::remove_file(&temporary);
    result
}

#[cfg(test)]
mod checked_tests {
    use super::*;

    #[test]
    fn checked_roundtrip() {
        let path =
            std::env::temp_dir().join(format!("cybmemory-{}-checked.bin", std::process::id()));
        let mut journal = Journal::default();
        journal.append("mission", "completed").unwrap();
        save_checked(&journal, &path).unwrap();
        assert_eq!(load_checked(&path).unwrap().entries(), journal.entries());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn detects_tampering() {
        let path =
            std::env::temp_dir().join(format!("cybmemory-{}-tamper.bin", std::process::id()));
        let mut journal = Journal::default();
        journal.append("mission", "completed").unwrap();
        save_checked(&journal, &path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[10] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(load_checked(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
