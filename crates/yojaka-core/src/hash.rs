//! Content hashing (BLAKE3). Read-only; files are only ever opened for reading.
//!
//! - [`partial_hash`]: `size || first 16 KiB || last 16 KiB`. Cheap; rules out most non-duplicates.
//! - [`full_hash`]: the whole file. Files over 64 MB are memory-mapped and hashed on several threads.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes read from the start and from the end for the partial hash.
pub const PARTIAL_BYTES: u64 = 16 * 1024;
/// Files larger than this are memory-mapped for the full hash.
pub const MMAP_THRESHOLD: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub fn to_hex(&self) -> String {
        blake3::Hash::from_bytes(self.0).to_hex().to_string()
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        blake3::Hash::from_hex(s).ok().map(|h| Self(*h.as_bytes()))
    }

    /// From a stored BLOB; `None` unless it is exactly 32 bytes.
    pub fn from_blob(b: &[u8]) -> Option<Self> {
        <[u8; 32]>::try_from(b).ok().map(Self)
    }
}

impl std::fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContentHash({})", &self.to_hex()[..12])
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_hex(&s).ok_or_else(|| serde::de::Error::custom("not a 64-digit hex hash"))
    }
}

/// A hash and how many bytes were read from the file to get it.
#[derive(Debug, Clone, Copy)]
pub struct Hashed {
    pub hash: ContentHash,
    pub bytes_read: u64,
}

/// Hash of `size || first 16 KiB || last 16 KiB`. `size` is the size recorded in the index; a file
/// that is now shorter is reported as an error rather than hashed.
pub fn partial_hash(path: &Path, size: u64) -> io::Result<Hashed> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());
    let head = size.min(PARTIAL_BYTES);
    let mut buf = vec![0u8; head as usize];
    file.read_exact(&mut buf)?;
    hasher.update(&buf);
    let mut read = head;
    if size > PARTIAL_BYTES {
        // Never re-read bytes the head already covered.
        let tail_start = (size - PARTIAL_BYTES).max(PARTIAL_BYTES);
        let tail = size - tail_start;
        file.seek(SeekFrom::Start(tail_start))?;
        let mut buf = vec![0u8; tail as usize];
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
        read += tail;
    }
    Ok(Hashed {
        hash: ContentHash(*hasher.finalize().as_bytes()),
        bytes_read: read,
    })
}

/// Hash of the whole file. Errors if the file is not exactly `size` bytes long any more.
pub fn full_hash(path: &Path, size: u64) -> io::Result<Hashed> {
    let mut hasher = blake3::Hasher::new();
    if size > MMAP_THRESHOLD {
        hasher.update_mmap_rayon(path)?;
    } else {
        let mut file = File::open(path)?;
        let mut buf = vec![0u8; 1024 * 1024];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;
        }
        if total != size {
            return Err(changed(path));
        }
    }
    Ok(Hashed {
        hash: ContentHash(*hasher.finalize().as_bytes()),
        bytes_read: size,
    })
}

fn changed(path: &Path) -> io::Error {
    io::Error::other(format!("{} changed size while reading", path.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn full_hash_matches_blake3() {
        let d = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let p = write(d.path(), "a", &bytes);
        let h = full_hash(&p, bytes.len() as u64).unwrap();
        assert_eq!(h.hash.0, *blake3::hash(&bytes).as_bytes());
        assert_eq!(h.bytes_read, bytes.len() as u64);
    }

    #[test]
    fn partial_hash_reads_at_most_two_chunks() {
        let d = tempfile::tempdir().unwrap();
        let big = vec![7u8; 1_000_000];
        let h = partial_hash(&write(d.path(), "big", &big), big.len() as u64).unwrap();
        assert_eq!(h.bytes_read, 2 * PARTIAL_BYTES);
        let small = vec![1u8; 100];
        let h = partial_hash(&write(d.path(), "small", &small), 100).unwrap();
        assert_eq!(h.bytes_read, 100);
        let mid = vec![1u8; 20_000];
        let h = partial_hash(&write(d.path(), "mid", &mid), 20_000).unwrap();
        assert_eq!(h.bytes_read, 20_000, "head and tail must not overlap");
    }

    #[test]
    fn partial_hash_sees_a_change_at_either_end_only() {
        let d = tempfile::tempdir().unwrap();
        let base = vec![0u8; 200_000];
        let mut head = base.clone();
        head[5] = 1;
        let mut tail = base.clone();
        tail[199_990] = 1;
        let mut middle = base.clone();
        middle[100_000] = 1;
        let n = base.len() as u64;
        let hash = |name: &str, b: &[u8]| partial_hash(&write(d.path(), name, b), n).unwrap().hash;
        let h0 = hash("base", &base);
        assert_ne!(h0, hash("head", &head));
        assert_ne!(h0, hash("tail", &tail));
        // Equal partial hash, different bytes: only the full hash tells them apart.
        assert_eq!(h0, hash("middle", &middle));
        let full = |name: &str| full_hash(&d.path().join(name), n).unwrap().hash;
        assert_ne!(full("base"), full("middle"));
    }

    #[test]
    fn short_file_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "s", &[0u8; 10]);
        assert!(partial_hash(&p, 100_000).is_err());
        assert!(full_hash(&p, 100_000).is_err());
    }

    #[test]
    fn hex_round_trip() {
        let h = ContentHash(*blake3::hash(b"x").as_bytes());
        assert_eq!(ContentHash::from_hex(&h.to_hex()), Some(h));
        assert_eq!(ContentHash::from_blob(&h.0), Some(h));
        assert_eq!(ContentHash::from_blob(&[0; 5]), None);
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<ContentHash>(&json).unwrap(), h);
    }
}
