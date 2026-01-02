# stringtree

Indented String Tree (IST) is an indentation-based syntax for representing
ordered tree data where the only scalar type is a string. It is designed to be
machine-generated, human-readable, diff-stable, and intentionally limited: IST
is not YAML, JSON, or a general data format. Instead, it provides a deterministic
syntax for a strictly defined tree model with ordered objects and arrays.

## Data model

IST defines exactly three value types:

- **String (atomic)**: single-line UTF-8 text with no escaping or implicit
  typing.
- **Object (non-atomic)**: ordered mapping from unique string keys to values,
  preserving insertion order.
- **Array (non-atomic)**: ordered list of values.

No other scalar types (numbers, booleans, null, etc.) are permitted, and cycles
or references are disallowed.

## Core rules

- Atomic values are inline only; non-atomic values are block-only. Blocks never
  represent strings.
- UTF-8 encoding, `\n` line endings, no tabs, and no trailing whitespace.
- Indentation uses exactly two spaces. It may increase by one level at a time
  and decrease by any number of levels; other patterns are errors.

## Syntax

Every non-empty, non-comment line must be one of the following forms:

- **Object entry**: `key: value` for inline strings, or `key:` followed by an
  indented block.
- **Array entry**: `- value` for inline strings, or a bare `-` followed by an
  indented block.

Keys are unquoted and must match `[A-Za-z_][A-Za-z0-9_]*`. Duplicate keys within
an object level are errors. The first line of a block determines whether the
block is an object (`key:`) or an array (`-`). Inline containers are illegal.
Empty blocks represent empty containers.

Inline values are always strings extending to end-of-line; leading and trailing
spaces are significant. An empty string is written as `key:` when no block
follows.

## Comments

If supported by a parser, comment lines start with `#` and must occupy the entire
line. Inline comments are not permitted. Emitters must not generate comments.

## Canonical semantic form

The canonical representation is the parsed data model serialized as JSON using
UTF-8, with objects as JSON objects, arrays as JSON arrays, and all values as
JSON strings. This form is authoritative for hashing, signing, and equality
comparisons.

## Project direction

This repository provides a clean Python interface backed by a fast Rust
implementation. The reference Python parser lives in ``src/stringtree`` and a
Rust implementation is available under ``rust/`` for performance-sensitive use
cases. Future work will connect the Rust backend to Python bindings and add
additional conformance tests.

## Development

- **Python**: The test suite uses ``pytest`` with ``src`` on the import path.
  Install the optional ``dev`` dependencies declared in ``pyproject.toml`` and
  run ``pytest`` from the repository root.
- **Rust**: Run ``cargo fmt -- --check``, ``cargo clippy -- -D warnings``, and
  ``cargo test --all --all-features`` from the ``rust/`` directory. The Rust
  CI workflow installs the stable toolchain with the required components.
