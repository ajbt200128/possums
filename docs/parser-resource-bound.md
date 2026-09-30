# Pinned streaming JSON parser allocation envelope

Scope: `src/inference/stream.rs::ProtocolParser::event` at `5ddcf65`, on the
repository's 64-bit Rust 1.88.0 / serde_json 1.0.151 build. This is a
**parser-only requested-heap-allocation** upper bound, not RSS, allocator
metadata, transport-backed `Bytes`, callback/rendering output, or an aggregate
104/512-MiB proof. Cumulative allocation over a long stream is not bounded by
this peak-live claim. Two independent source analyses checked the accounting;
local tests cover the excluded RawValue shape but are not the proof.

The [protocol limits](../src/inference/stream.rs) fix frame length `F=262,144`,
line length `L=65,536`, 8,192 guarded JSON nodes `N`, depth 16, and retained
line/payload capacity `B=327,680` bytes. [`validate_json`](../src/inference/stream.rs)
finishes its guard before `WireCounts` is parsed; `WireCounts` remains live while
`Value` is built and while `delta` runs. Budget the guard separately from the
wire/tree phase, not add their peaks. A key is not separately counted as a node,
but each ordinary map entry has a guarded value, so the sum of ordinary map
entries and array elements is at most `N-1` on success. Charge one additional
pending key during rejection. All decoded key/string bytes together are bounded
by their encoded input bytes, apart from synthetic arbitrary-precision Number
keys, charged separately.

The pinned SDK enables serde_json's `raw_value` feature. Its
[`Value` visitor](https://github.com/serde-rs/json/blob/v1.0.151/src/value/de.rs)
can recursively reparse a quoted value under the decoded
`$serde_json::private::RawValue` key, invalidating the guarded node bound.
`5ddcf65` rejects that exact decoded key in the guard, including escaped wire
spellings. The separate private Number interpretation invokes a scalar
[`NumberFromString` parser](https://github.com/serde-rs/json/blob/v1.0.151/src/number.rs),
not recursive `Value` construction; ordinary fractional numbers must remain
accepted. The prior 189,172-byte three-property fixture contains 21,000
singleton maps behind just eight guarded nodes and is now rejected **before**
`Value` construction. The attestation evidence guard separately rejects this
same key; this worksheet does not bound evidence parsing.

For Rust 1.88's [B-tree nodes](https://github.com/rust-lang/rust/blob/1.88.0/library/alloc/src/collections/btree/node.rs),
`String` occupies 24 bytes and `Value` 32 bytes. A BTreeMap leaf/internal node
requests 632/728 bytes; BTreeSet leaf/internal nodes request 280/376. Each
non-root node after insertion has at least five entries, so charging 632 bytes
per ordinary map entry and 280 per guard key covers sparse one-entry maps and
internal nodes. Insertion/split transients are separate below. [`Value` arrays](https://github.com/serde-rs/json/blob/v1.0.151/src/value/de.rs)
use growing Vecs: up to four 32-byte slots per element, or six including old/new
growth overlap, i.e. 192 bytes per array element. Thus for `E` map entries and
`A` array elements, `632E + 192A <= 632N = 5,056 KiB` of Value backing.

| Simultaneously live wire/Value phase allocation | Conservative KiB |
| --- | ---: |
| Fixed parser line/payload buffers | 320 |
| Value map/array backing, including array growth | 5,056 |
| Owned strings, keys and Number storage (`2F + 32N`) | 768 |
| WireCounts choices Vec, including growth | 128 |
| Reusable deserializer scratch, including growth | 200 |
| Temporary numeric scanning/reparsing and overlap | 256 |
| Potential input-bearing error formatting | 1,152 |
| B-tree insertion transients, error boxes and constants | 64 |
| **Total** | **7,944 KiB = 8,134,656 bytes (<8 MiB)** |

A JSON string token cannot cross an inserted SSE newline, so one token is at
most `L` bytes. Scratch old/new growth fits `3(L+4) < 200 KiB`. Input-bearing
debug escaping expands at most sixfold; formatting plus old/new allocation fits
the `18L` error allowance. The guard alone, with BTreeSet storage
`280(N+1)`, original key bytes `F`, synthetic Number keys `28(N+1)`, fixed
buffers, scratch/numeric temporaries and split slack, is below 3.5 MiB. The
Value phase dominates. Both source analyses found no rejected-prefix shape
that exceeds these components **after** the exact-key exclusion.

This does **not** mean the earlier proposed 8-MiB *combined*
parser/renderer/delivery subtotal is valid. Returned HTTP/1 DATA may retain a
larger backing allocation than its parser slice; `delta(content)` executes
while the Value tree is still live, and renderer/delivery buffers overlap it.
The heavy-lane proof must account for those owners together, without raising
the unchanged 104-MiB lane or using a synthetic fixture peak as a bound.
