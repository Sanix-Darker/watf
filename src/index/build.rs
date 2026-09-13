use super::{DICT_ENTRY, HEADER, MAGIC, MAX_FILE, MAX_RECORDS, RECORD};
use crate::{
    record::{Arity, Capability},
    text, Error, Result,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize)]
pub struct BuildStats {
    pub records: usize,
    pub terms: usize,
    pub postings: usize,
    pub bytes: u64,
    pub duplicate_records_removed: usize,
}

#[derive(Default)]
struct Pool {
    bytes: Vec<u8>,
    offsets: HashMap<String, u32>,
}
impl Pool {
    fn put(&mut self, text: &str) -> Result<u32> {
        if let Some(&offset) = self.offsets.get(text) {
            return Ok(offset);
        }
        let offset = u32::try_from(self.bytes.len())
            .map_err(|_| Error::message("string pool exceeds 4 GiB"))?;
        let length =
            u32::try_from(text.len()).map_err(|_| Error::message("string is too large"))?;
        self.bytes.extend_from_slice(&length.to_le_bytes());
        self.bytes.extend_from_slice(text.as_bytes());
        self.offsets.insert(text.to_owned(), offset);
        Ok(offset)
    }
}

struct Cleanup {
    path: PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn private_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// Rebuild completely, then atomically publish. Existing readers retain the old inode.
pub fn build(path: &Path, mut capabilities: Vec<Capability>) -> Result<BuildStats> {
    if capabilities.is_empty() || capabilities.len() > MAX_RECORDS {
        return Err(Error::message("index must contain 1..1000000 records"));
    }
    for cap in &capabilities {
        cap.validate()?;
    }
    let before = capabilities.len();
    // Prefer actual local help over a bundled catalog when a scope overlaps.
    capabilities.sort_by(|a, b| {
        a.key()
            .cmp(&b.key())
            .then(b.source.kind.priority().cmp(&a.source.kind.priority()))
            .then(a.id.cmp(&b.id))
    });
    capabilities.dedup_by(|a, b| a.key() == b.key());
    let count = capabilities.len();
    let mut pool = Pool::default();
    let mut raw_records = Vec::with_capacity(count * RECORD);
    let mut lengths = Vec::with_capacity(count);
    let mut terms: BTreeMap<String, Vec<(u32, f32)>> = BTreeMap::new();
    for (doc, cap) in capabilities.iter().enumerate() {
        let mut frequencies: BTreeMap<String, f32> = BTreeMap::new();
        let mut length = 0_u32;
        for (field, weight) in [
            (cap.command_key(), 6.0),
            (cap.name.clone(), 7.0),
            (cap.aliases.join(" "), 7.0),
            (cap.summary.clone(), 1.0),
        ] {
            for term in text::tokens(&field) {
                *frequencies.entry(term).or_default() += weight;
                length = length.saturating_add(1);
            }
        }
        // Reserved terms support exact scope lookups without parsing all records.
        frequencies.insert(format!("@command:{}", cap.command_key()), 1.0);
        frequencies.insert(format!("@root:{}", cap.root()), 1.0);
        lengths.push(length.max(1) as f32);
        for (term, tf) in frequencies {
            // A separate local-only term namespace prevents catalog posting scans
            // during ordinary Linux queries. It duplicates only non-catalog terms.
            if cap.source.kind.priority() > 1 && !term.starts_with('@') {
                terms
                    .entry(format!("~{term}"))
                    .or_default()
                    .push((doc as u32, tf));
            }
            terms.entry(term).or_default().push((doc as u32, tf));
        }
        let values = [
            pool.put(&cap.id)?,
            pool.put(&cap.command_key())?,
            pool.put(&cap.name)?,
            pool.put(&cap.summary)?,
            pool.put(&serde_json::to_string(&cap.aliases)?)?,
            pool.put(&serde_json::to_string(&cap.choices)?)?,
            pool.put(&serde_json::to_string(&cap.source)?)?,
            pool.put(cap.value_type.as_deref().unwrap_or(""))?,
        ];
        for value in values {
            raw_records.extend_from_slice(&value.to_le_bytes());
        }
        raw_records.extend_from_slice(&[
            cap.kind.code(),
            match cap.arity {
                Arity::None => 0,
                Arity::One => 1,
                Arity::Optional => 2,
                Arity::Many => 3,
                Arity::Unknown => 4,
                Arity::Two => 5,
            },
            u8::from(cap.required),
            cap.source.kind.priority(),
        ]);
        raw_records.extend_from_slice(&pool.put(cap.root())?.to_le_bytes());
        raw_records.extend_from_slice(&length.to_le_bytes());
        raw_records.extend_from_slice(&0_u32.to_le_bytes());
    }
    drop(capabilities);
    let average = lengths.iter().sum::<f32>() / count as f32;
    let term_count = terms.len();
    let mut dictionary = Vec::with_capacity(term_count * DICT_ENTRY);
    let mut postings = Vec::new();
    let mut posting_count = 0;
    for (term, list) in terms {
        let offset = u32::try_from(postings.len())
            .map_err(|_| Error::message("postings exceed format limits"))?;
        let df = list.len();
        let idf = (1.0 + (count as f32 - df as f32 + 0.5) / (df as f32 + 0.5)).ln();
        dictionary.extend_from_slice(&pool.put(&term)?.to_le_bytes());
        dictionary.extend_from_slice(&offset.to_le_bytes());
        dictionary.extend_from_slice(&(df as u32).to_le_bytes());
        dictionary.extend_from_slice(&0_u32.to_le_bytes());
        for (doc, tf) in list {
            let impact =
                idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * lengths[doc as usize] / average));
            postings.extend_from_slice(&doc.to_le_bytes());
            postings.extend_from_slice(&impact.to_le_bytes());
            posting_count += 1;
        }
    }
    let dictionary_offset = HEADER as u64;
    let postings_offset = dictionary_offset + dictionary.len() as u64;
    let records_offset = postings_offset + postings.len() as u64;
    let pool_offset = records_offset + raw_records.len() as u64;
    let file_size = pool_offset + pool.bytes.len() as u64;
    if file_size > MAX_FILE {
        return Err(Error::message("index exceeds 2 GiB format policy"));
    }
    let mut header = [0_u8; HEADER];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&1_u32.to_le_bytes());
    header[12..16].copy_from_slice(&(count as u32).to_le_bytes());
    header[16..20].copy_from_slice(&(term_count as u32).to_le_bytes());
    for (at, value) in [
        (24, dictionary_offset),
        (32, postings_offset),
        (40, records_offset),
        (48, pool_offset),
        (56, file_size),
    ] {
        header[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let lock_path = path.with_extension("widx.lock");
    let mut lock = private_new(&lock_path).map_err(|e| {
        Error::message(format!(
            "cannot acquire {}: {e}; remove a stale lock only after checking no indexer is running",
            lock_path.display()
        ))
    })?;
    let _lock_cleanup = Cleanup { path: lock_path };
    writeln!(lock, "{}", std::process::id())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("tmp.{}.{}", std::process::id(), stamp));
    let _temp_cleanup = Cleanup {
        path: temporary.clone(),
    };
    let mut file = private_new(&temporary)?;
    file.write_all(&header)?;
    let mut hash = Sha256::new();
    for bytes in [&dictionary, &postings, &raw_records, &pool.bytes] {
        file.write_all(bytes)?;
        hash.update(bytes);
    }
    header[64..96].copy_from_slice(&hash.finalize());
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&header)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(BuildStats {
        records: count,
        terms: term_count,
        postings: posting_count,
        bytes: file_size,
        duplicate_records_removed: before - count,
    })
}
