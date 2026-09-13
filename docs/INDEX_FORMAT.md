# Binary index version 1

All numeric fields are little-endian. Maximum file size is 2 GiB and maximum
record count is one million. The reader bounds-checks offsets before access.
The format is private to WATF; rebuild on incompatible format changes.

## Header: 128 bytes

| Offset | Width | Field |
|---:|---:|---|
| 0 | 8 | ASCII magic `WATFIDX1` |
| 8 | 4 | Format version, 1 |
| 12 | 4 | Record count |
| 16 | 4 | Dictionary term count |
| 24 | 8 | Dictionary offset, 128 |
| 32 | 8 | Posting section offset |
| 40 | 8 | Record section offset |
| 48 | 8 | Interned string pool offset |
| 56 | 8 | Total file length |
| 64 | 32 | SHA256 of bytes after the header |
| Other | variable | Reserved, zero when written |

Dictionary entries are 16 bytes: string-pool term offset (u32), relative posting
byte offset (u32), document frequency (u32), reserved (u32). Terms are lexically
sorted. Posting entries are 8 bytes: DocID u32 and precomputed f32 impact, sorted
by DocID within a term.

Each record occupies 48 bytes. The first eight u32 values refer to pool strings:
ID, command scope, name, summary, JSON aliases, JSON enum choices, JSON source,
and value type. Bytes 32-35 contain kind, arity, required and source priority.
At byte 36 is the root-program string offset, then document length and reserved.
Pool strings have a u32 byte length followed by UTF-8 bytes, without a NUL terminator.

Kinds: command 0, option 1, input_field 2, example 3. Arities: none 0, one 1,
optional 2, many 3, unknown 4, two 5. Reserved terms `@command:SCOPE` and
`@root:PROGRAM` allow exact lookups. `~TERM` denotes non-catalog postings.

## Integrity and publication

Normal opening checks the header and section arithmetic, not the entire payload
hash. `doctor --verify` validates payload SHA256, dictionary ordering, postings,
record decoding and field policies. This distinction keeps opening independent
of the whole corpus size while offering an explicit full scan.

A writer constructs a new file, takes an exclusive create-new lock, writes and
fsyncs, then atomically renames it into place. Existing mappings continue reading
the old inode. A stale lock requires operator inspection before removal. Writer
locking currently protects publication, not the preceding index computation.
Two concurrent builders can therefore duplicate CPU/memory work before one fails
to acquire the publishing lock. There is no incremental indexing daemon.
