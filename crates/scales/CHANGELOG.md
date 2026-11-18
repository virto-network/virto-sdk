# Changelog

## 1.2.0

Serializing with type information is stricter and fixes several encodings,
so some input that used to be accepted is now refused. Such input used to
encode into wrong SCALE bytes, so it is released as a minor version.

### Changed

- Struct fields given as a map are matched by name instead of by position.
  Fields are written in declaration order, whatever the order of the input's
  keys (serde_json objects iterate sorted by key). An unknown, duplicate or
  missing field is an error; a missing `Option` field is `None`.
- An `Option` takes the bare value: `5`, not `{"Some": 5}`, which is now an
  error. A null is `None`.
- `RawPallet` has an `errors_ty` field, so filtered metadata keeps pallet
  error types. Code that builds a `RawPallet` literal must set it.

### Added

- `Value::variant_field_at` returns one field of the selected enum variant,
  whether its payload is a newtype, a tuple or a named struct.

### Fixed

- A newtype at the root (e.g. a transaction extension wrapping
  `Compact<Nonce>`) was serialized without its inner type, as raw bytes.
- Types wrapped in more than one newtype (e.g. a `BoundedBTreeSet` of tuples)
  got wrong length prefixes.
- A Rust `Option` serialized with type information got its `Some` tag twice:
  `Some(5u32)` became `01 01 05 00 00 00`.
- A null for an `Option` type was encoded as `Some` with no payload.
