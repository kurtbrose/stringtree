"""Python interface for the Indented String Tree (IST) format.

The :mod:`stringtree` package provides a minimal reference implementation of the
Indented String Tree (IST) data model as described in :mod:`README.md`.  The
library focuses on validation and deterministic parsing so that higher-level
bindings (including the planned Rust backend) can rely on predictable
semantics.
"""

from __future__ import annotations

from collections import OrderedDict
from dataclasses import dataclass
import json
import re
from typing import Iterable, Iterator, MutableMapping, Sequence

__all__ = [
    "ParseError",
    "loads",
    "validate_semantics",
    "to_canonical_json",
]


KEY_PATTERN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


@dataclass(frozen=True)
class ParseError(ValueError):
    """Raised when a source string violates the IST grammar."""

    line: int
    column: int
    message: str

    def __str__(self) -> str:  # pragma: no cover - trivial presentation
        return f"line {self.line}, column {self.column}: {self.message}"


def _trim_trailing_newline(lines: Iterable[str]) -> Iterator[str]:
    for line in lines:
        yield line.rstrip("\n")


def _ensure_no_tabs(line: str, lineno: int) -> None:
    if "\t" in line:
        raise ParseError(lineno, line.index("\t") + 1, "tabs are not allowed")


def _ensure_no_trailing_whitespace(line: str, lineno: int) -> None:
    if line.rstrip(" ") != line:
        raise ParseError(lineno, len(line), "trailing whitespace is not allowed")


def _leading_indent(line: str, lineno: int) -> int:
    spaces = len(line) - len(line.lstrip(" "))
    if spaces % 2:
        raise ParseError(lineno, spaces + 1, "indentation must use multiples of two spaces")
    return spaces // 2


def _parse_key(segment: str, lineno: int) -> str:
    if not KEY_PATTERN.match(segment):
        raise ParseError(lineno, 1, "object keys must match [A-Za-z_][A-Za-z0-9_]*")
    return segment


def _next_significant(lines: Sequence[tuple[int, str]], start: int) -> int | None:
    for idx in range(start, len(lines)):
        _, text = lines[idx]
        if text.strip() == "":
            continue
        stripped = text.lstrip(" ")
        if stripped.startswith("#"):
            continue
        return idx
    return None


def loads(source: str):
    """Parse an IST document into Python primitives.

    The return value uses ordered mappings (``collections.OrderedDict``) for
    objects and lists for arrays. Inline strings remain Python ``str``
    instances. Violations of the IST grammar raise :class:`ParseError`.
    """

    # Preprocess and validate the raw lines first to surface low-level errors
    # with helpful line/column numbers.
    processed: list[tuple[int, str]] = []
    for lineno, line in enumerate(_trim_trailing_newline(source.splitlines()), start=1):
        _ensure_no_tabs(line, lineno)
        _ensure_no_trailing_whitespace(line, lineno)
        processed.append((lineno, line))

    next_index = _next_significant(processed, 0)
    if next_index is None:
        raise ParseError(1, 1, "document is empty")

    first_indent = _leading_indent(processed[next_index][1], processed[next_index][0])
    if first_indent != 0:
        raise ParseError(processed[next_index][0], 1, "document must start at indentation level 0")

    first_text = processed[next_index][1].lstrip(" ")
    if first_text.startswith("-"):
        container_type = "array"
    elif ":" in first_text:
        container_type = "object"
    else:
        raise ParseError(processed[next_index][0], 1, "root must be an object or array entry")

    parsed, consumed = _parse_container(processed, next_index, indent_level=0, container_type=container_type)
    tail = _next_significant(processed, consumed)
    if tail is not None:
        lineno, _ = processed[tail]
        raise ParseError(lineno, 1, "unexpected content after top-level value")
    return parsed


def _parse_container(
    lines: Sequence[tuple[int, str]], start_index: int, *, indent_level: int, container_type: str
):
    if container_type == "object":
        container: MutableMapping[str, object] = OrderedDict()
    elif container_type == "array":
        container = []
    else:  # pragma: no cover - defensive
        raise ValueError(f"unsupported container type {container_type}")

    index = start_index
    while index < len(lines):
        lineno, raw = lines[index]
        if raw.strip() == "":
            index += 1
            continue

        indent = _leading_indent(raw, lineno)
        stripped = raw[indent * 2 :]
        if stripped.startswith("#"):
            index += 1
            continue

        if indent < indent_level:
            break  # caller will handle unwinding
        if indent > indent_level:
            raise ParseError(lineno, 1, "indentation may only increase by one level at a time")

        if container_type == "object":
            key, value, consumed = _parse_object_entry(lines, index, indent_level)
            if key in container:
                raise ParseError(lineno, 1, f"duplicate key '{key}' at this object level")
            container[key] = value
        else:
            value, consumed = _parse_array_entry(lines, index, indent_level)
            container.append(value)

        index = consumed

    return container, index


def _parse_object_entry(lines: Sequence[tuple[int, str]], index: int, indent_level: int):
    lineno, raw = lines[index]
    stripped = raw[indent_level * 2 :]
    if ":" not in stripped:
        raise ParseError(lineno, 1, "object entries must contain a ':' separator")
    key_segment, value_segment = stripped.split(":", 1)
    key = _parse_key(key_segment, lineno)

    if value_segment == "":
        # Could be an empty string or the start of a block; inspect lookahead.
        next_sig = _next_significant(lines, index + 1)
        if next_sig is None:
            return key, "", len(lines)

        next_lineno, next_raw = lines[next_sig]
        next_indent = _leading_indent(next_raw, next_lineno)
        if next_indent == indent_level:
            return key, "", next_sig
        if next_indent != indent_level + 1:
            raise ParseError(next_lineno, 1, "indentation must increase by exactly one level for a block")
        next_stripped = next_raw[next_indent * 2 :]
        if next_stripped.startswith("-"):
            nested_type = "array"
        elif ":" in next_stripped:
            nested_type = "object"
        else:
            raise ParseError(next_lineno, 1, "block must start with an object or array entry")
        value, consumed = _parse_container(lines, next_sig, indent_level=indent_level + 1, container_type=nested_type)
        return key, value, consumed

    if not value_segment.startswith(" "):
        raise ParseError(lineno, len(raw), "inline string values must follow a space")
    return key, value_segment[1:], index + 1


def _parse_array_entry(lines: Sequence[tuple[int, str]], index: int, indent_level: int):
    lineno, raw = lines[index]
    stripped = raw[indent_level * 2 :]
    if not stripped.startswith("-"):
        raise ParseError(lineno, 1, "array entries must start with '-'")
    payload = stripped[1:]

    if payload == "":
        next_sig = _next_significant(lines, index + 1)
        if next_sig is None:
            return "", len(lines)
        next_lineno, next_raw = lines[next_sig]
        next_indent = _leading_indent(next_raw, next_lineno)
        if next_indent == indent_level:
            return "", next_sig
        if next_indent != indent_level + 1:
            raise ParseError(next_lineno, 1, "indentation must increase by exactly one level for a block")
        next_stripped = next_raw[next_indent * 2 :]
        if next_stripped.startswith("-"):
            nested_type = "array"
        elif ":" in next_stripped:
            nested_type = "object"
        else:
            raise ParseError(next_lineno, 1, "block must start with an object or array entry")
        value, consumed = _parse_container(lines, next_sig, indent_level=indent_level + 1, container_type=nested_type)
        return value, consumed

    if not payload.startswith(" "):
        raise ParseError(lineno, len(raw), "inline string values must follow a space")
    return payload[1:], index + 1


def validate_semantics(value: object) -> None:
    """Validate that ``value`` conforms to the IST data model.

    The function accepts strings, ordered mappings with string keys, and lists.
    Any other type (including numbers, booleans, or ``None``) triggers a
    :class:`TypeError`. Nested structures are checked recursively.
    """

    if isinstance(value, str):
        return
    if isinstance(value, MutableMapping):
        for key, nested in value.items():
            if not isinstance(key, str):
                raise TypeError("object keys must be strings")
            validate_semantics(nested)
        return
    if isinstance(value, list):
        for nested in value:
            validate_semantics(nested)
        return
    raise TypeError("values must be strings, ordered mappings, or lists")


def to_canonical_json(value: object) -> str:
    """Return the canonical JSON serialization for a parsed IST value."""

    validate_semantics(value)
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
