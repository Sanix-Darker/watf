use super::{DICT_ENTRY, HEADER, MAGIC, MAX_FILE, MAX_RECORDS, POSTING, RECORD};
use crate::{
    record::{Arity, Capability, Kind},
    Error, Result,
};
use memmap2::{Mmap, MmapOptions};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

enum Storage {
    Mapped(Mmap),
    Owned(Vec<u8>),
}
impl AsRef<[u8]> for Storage {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Mapped(m) => m,
            Self::Owned(v) => v,
        }
    }
}

pub struct Index {
    storage: Storage,
    pub(crate) count: usize,
    terms: usize,
    dictionary: usize,
    pub(crate) postings: usize,
    pub(crate) records: usize,
    pool: usize,
}

fn invalid() -> Error {
    Error::message("invalid or corrupt watf index; rebuild it with watf index")
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at.checked_add(4).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}
fn u64_at(bytes: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes
            .get(at..at.checked_add(8).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}

impl Index {
    /// The writer publishes new inodes, never overwrites mapped files. Do not
    /// truncate an index from another process. Use owned mode for untrusted files.
    pub fn open(path: &Path, mmap: bool) -> Result<Self> {
        let file = File::open(path).map_err(|e| {
            Error::message(format!("{}: {e}; run watf index first", path.display()))
        })?;
        let length = file.metadata()?.len();
        if !(HEADER as u64..=MAX_FILE).contains(&length) {
            return Err(invalid());
        }
        let storage = if mmap {
            // SAFETY: mapping is read-only and bounds are checked before use.
            // Atomic publication is required of every writer; see docs/SECURITY.md.
            Storage::Mapped(unsafe { MmapOptions::new().map(&file)? })
        } else {
            let mut bytes = Vec::with_capacity(length as usize);
            file.take(length + 1).read_to_end(&mut bytes)?;
            Storage::Owned(bytes)
        };
        Self::from_storage(storage)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_storage(Storage::Owned(bytes))
    }

    fn from_storage(storage: Storage) -> Result<Self> {
        let b = storage.as_ref();
        if b.len() < HEADER || b.get(..8) != Some(MAGIC.as_slice()) || u32_at(b, 8)? != 1 {
            return Err(invalid());
        }
        let count = u32_at(b, 12)? as usize;
        let terms = u32_at(b, 16)? as usize;
        let dictionary = usize::try_from(u64_at(b, 24)?).map_err(|_| invalid())?;
        let postings = usize::try_from(u64_at(b, 32)?).map_err(|_| invalid())?;
        let records = usize::try_from(u64_at(b, 40)?).map_err(|_| invalid())?;
        let pool = usize::try_from(u64_at(b, 48)?).map_err(|_| invalid())?;
        if count == 0
            || count > MAX_RECORDS
            || terms > 10_000_000
            || dictionary != HEADER
            || dictionary.checked_add(terms.checked_mul(DICT_ENTRY).ok_or_else(invalid)?)
                != Some(postings)
            || postings > records
            || (records - postings) % POSTING != 0
            || records.checked_add(count.checked_mul(RECORD).ok_or_else(invalid)?) != Some(pool)
            || pool > b.len()
            || u64_at(b, 56)? != b.len() as u64
            || b.len() as u64 > MAX_FILE
        {
            return Err(invalid());
        }
        Ok(Self {
            storage,
            count,
            terms,
            dictionary,
            postings,
            records,
            pool,
        })
    }

    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn term_count(&self) -> usize {
        self.terms
    }
    pub fn file_bytes(&self) -> usize {
        self.bytes().len()
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        self.storage.as_ref()
    }
    pub(crate) fn number(&self, at: usize) -> Result<u32> {
        u32_at(self.bytes(), at)
    }
    pub(crate) fn record_at(&self, doc: u32) -> Result<usize> {
        if doc as usize >= self.count {
            return Err(invalid());
        }
        Ok(self.records + doc as usize * RECORD)
    }
    pub(crate) fn string(&self, offset: u32) -> Result<&str> {
        let start = self.pool.checked_add(offset as usize).ok_or_else(invalid)?;
        let length = self.number(start)? as usize;
        let begin = start.checked_add(4).ok_or_else(invalid)?;
        let end = begin.checked_add(length).ok_or_else(invalid)?;
        std::str::from_utf8(self.bytes().get(begin..end).ok_or_else(invalid)?)
            .map_err(|_| invalid())
    }
    pub(crate) fn field(&self, doc: u32, byte: usize) -> Result<&str> {
        let at = self.record_at(doc)?;
        self.string(self.number(at + byte)?)
    }
    pub fn command_key(&self, doc: u32) -> Result<&str> {
        self.field(doc, 4)
    }
    pub fn root(&self, doc: u32) -> Result<&str> {
        self.field(doc, 36)
    }
    pub(crate) fn kind_code(&self, doc: u32) -> Result<u8> {
        Ok(self.bytes()[self.record_at(doc)? + 32])
    }
    pub(crate) fn priority(&self, doc: u32) -> Result<u8> {
        Ok(self.bytes()[self.record_at(doc)? + 35])
    }

    pub(crate) fn term(&self, query: &str) -> Result<Option<(usize, usize)>> {
        let mut low = 0;
        let mut high = self.terms;
        while low < high {
            let mid = low + (high - low) / 2;
            let at = self.dictionary + mid * DICT_ENTRY;
            match self.string(self.number(at)?)?.cmp(query) {
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
                std::cmp::Ordering::Equal => {
                    let offset = self.number(at + 4)? as usize;
                    let count = self.number(at + 8)? as usize;
                    let start = self.postings.checked_add(offset).ok_or_else(invalid)?;
                    let end = start
                        .checked_add(count.checked_mul(POSTING).ok_or_else(invalid)?)
                        .ok_or_else(invalid)?;
                    if offset % POSTING != 0 || end > self.records {
                        return Err(invalid());
                    }
                    return Ok(Some((start, count)));
                }
            }
        }
        Ok(None)
    }

    pub fn ids_for_command(&self, command: &str) -> Result<Vec<u32>> {
        let Some((start, count)) = self.term(&format!("@command:{command}"))? else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::with_capacity(count);
        for n in 0..count {
            let doc = self.number(start + n * POSTING)?;
            self.record_at(doc)?;
            ids.push(doc);
        }
        Ok(ids)
    }

    pub fn capability(&self, doc: u32) -> Result<Capability> {
        let at = self.record_at(doc)?;
        let b = self.bytes();
        let kind = match b[at + 32] {
            0 => Kind::Command,
            1 => Kind::Option,
            2 => Kind::InputField,
            3 => Kind::Example,
            _ => return Err(invalid()),
        };
        let arity = match b[at + 33] {
            0 => Arity::None,
            1 => Arity::One,
            2 => Arity::Optional,
            3 => Arity::Many,
            4 => Arity::Unknown,
            5 => Arity::Two,
            _ => return Err(invalid()),
        };
        let typ = self.field(doc, 28)?;
        Ok(Capability {
            id: self.field(doc, 0)?.to_owned(),
            command: self.field(doc, 4)?.split(' ').map(str::to_owned).collect(),
            kind,
            name: self.field(doc, 8)?.to_owned(),
            summary: self.field(doc, 12)?.to_owned(),
            aliases: serde_json::from_str(self.field(doc, 16)?)?,
            choices: serde_json::from_str(self.field(doc, 20)?)?,
            source: serde_json::from_str(self.field(doc, 24)?)?,
            arity,
            required: b[at + 34] != 0,
            value_type: if typ.is_empty() {
                None
            } else {
                Some(typ.to_owned())
            },
        })
    }

    /// Full digest and structural verification is deliberately not on the hot path.
    pub fn verify(&self) -> Result<()> {
        let digest = Sha256::digest(&self.bytes()[HEADER..]);
        if digest.as_slice() != &self.bytes()[64..96] {
            return Err(Error::message("index SHA-256 mismatch"));
        }
        let mut previous = "";
        for n in 0..self.terms {
            let at = self.dictionary + n * DICT_ENTRY;
            let word = self.string(self.number(at)?)?;
            if n > 0 && word <= previous {
                return Err(invalid());
            }
            previous = word;
            let (start, count) = self.term(word)?.ok_or_else(invalid)?;
            let mut last = None;
            for p in 0..count {
                let doc = self.number(start + p * POSTING)?;
                let score = f32::from_bits(self.number(start + p * POSTING + 4)?);
                if doc as usize >= self.count
                    || last.is_some_and(|l| doc <= l)
                    || !score.is_finite()
                    || score < 0.0
                {
                    return Err(invalid());
                }
                last = Some(doc);
            }
        }
        for doc in 0..self.count as u32 {
            self.capability(doc)?.validate()?;
        }
        Ok(())
    }
}
