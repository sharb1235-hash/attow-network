# Canonical codec version 1

Attow Network stores immutable, versioned objects using a restricted deterministic
CBOR value model. This format is implemented by `attow-codec`. It supplies content
identity and serialization; an object hash supplies no authority to execute code,
spend rights, or dispatch effects.

## Values and encoding

`Value` supports null, booleans, unsigned 64-bit integers, negative CBOR integers,
byte strings, UTF-8 text, arrays and maps with text keys. `Negative(n)` denotes
`-1 - n`, covering the full CBOR negative integer range without aliases for
positive integers. Text is not Unicode-normalized: different UTF-8 byte sequences
are distinct values. Byte strings and text strings are distinct types.

Integer and length arguments use their shortest possible representation. Containers
have definite lengths. Map keys are unique and ordered by UTF-8 byte length, then
lexicographically by UTF-8 bytes. For text-only keys this is their deterministic
encoded CBOR key order. For example, `b` precedes `aa`, even though the in-memory
`BTreeMap` sorts the strings differently. Insertion order never affects bytes.

Floating-point values, CBOR tags, undefined, other simple values, indefinite
containers, duplicate map keys, non-shortest arguments, invalid UTF-8, unordered
map keys and trailing bytes are rejected. The decoder requires accepted bytes to
equal the encoder's output using the same limits. Minicbor emits bytes after
preflight validation; the project's parser applies this stricter profile itself.

The underlying encoding conventions are based on the established
[CBOR specification, RFC 8949](https://www.rfc-editor.org/rfc/rfc8949.html).
This value model and the following envelope are original project schema code.

## Versioned object envelope

The registered generic object has kind `1`, schema `1`, and exactly four array
fields:

```text
[0x4154544f, 1, [reference_bytes...], payload]
```

The numeric marker represents `ATTO`. Every reference is a byte string of exactly
32 bytes; reference IDs appear once in ascending byte order. The Rust `Object`
contains `refs: BTreeSet<ObjectId>` and `payload: Value`. Reference order is not
semantically significant. An empty reference set and null payload encode as
`841a4154544f0180f6`, nine bytes.

`references(kind, schema, bytes, limits)` recognizes only this registered
kind/schema pair, strictly decodes the complete envelope, and returns its explicit
references. Unknown kinds or schema versions return `UnknownSchema`; the collector
must not guess references by searching arbitrary payload bytes. Applications must
include every object dependency in `refs`, even if a payload also contains an ID.
Future object schemas require explicit registered traversal behavior.

## Object identity

The object ID is the 32-byte BLAKE3 digest of this unambiguous frame:

```text
ASCII("attow-network/object/v1") || 0x00
|| kind_u16_big_endian
|| schema_u16_big_endian
|| payload_byte_length_u64_big_endian
|| canonical_payload_bytes
```

Here `canonical_payload_bytes` means the entire encoded object envelope, including
references. `object_id` hashes any supplied bytes; it does not validate a schema.
The store separately validates recognized schemas and checks hashes on reads.
`ObjectId` displays as 64 lowercase hexadecimal characters and parses exactly 64
ASCII hexadecimal characters, accepting either letter case.

[BLAKE3's official implementation](https://github.com/BLAKE3-team/BLAKE3) provides
the hash primitive. Content addressing and Merkle structures are established
techniques; this project does not claim their invention.

## Resource limits and failure behavior

`Limits` bounds encoded bytes, nesting depth and item count. Defaults are 1 MiB,
64 levels and 100,000 items. Hard caps are 64 MiB, 128 levels and 1,000,000 items.
Zero limits and values above these caps fail with `InvalidLimits`. The root counts
as depth one. Every container, scalar and map key counts as one item; envelope
fields count too. Byte-string contents count toward bytes, not item count.

The decoder checks the input size before parsing, checks declared lengths and
remaining input before allocation, and checks item/depth limits before descending
or allocating a container. Checked integer conversions prevent length overflow.
Encoding first traverses the existing value under the same limits and calculates
its exact output length, then allocates the output. It does not clone the payload
to construct the object envelope. Custom valid limits are available through
`encode_with_limits`; `Canonical::encode` uses the defaults.

Memory use is bounded by both bytes and items, including Rust container overhead;
`object_bytes` is not a claim that peak process memory equals the encoded size.
Canonical re-encoding and map ordering allocate additional bounded memory. Error
variants identify the failing invariant without including object contents.

## Compatibility evidence

The original [byte/hash vectors](../fixtures/original/protocol/codec-v1.txt)
cover a null object, mixed-length map keys and an explicit reference. Reproduce
them with:

```sh
cargo run -p attow-codec --example vectors --locked
cargo test -p attow-codec --locked
```

Tests consume the committed vectors, check malformed input and every truncated
prefix of a sample object, and exercise roundtrip and accepted-byte re-encoding
properties. The initial test run preceded implementation and failed as expected.
The separate fuzz target extends these properties to arbitrary bounded input;
fuzz tool availability and actual execution evidence are recorded in the
[progress record](progress.md), not implied by the target's existence.
