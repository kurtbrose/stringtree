import json

import pytest

from stringtree import ParseError, loads, to_canonical_json, validate_semantics


def test_loads_parses_nested_structures_with_ordering():
    source = """
title: Demo
items:
  - first
  - second
metadata:
  author:
    name: Ada
""".strip()

    parsed = loads(source)

    assert list(parsed.keys()) == ["title", "items", "metadata"]
    assert parsed["title"] == "Demo"
    assert parsed["items"] == ["first", "second"]
    assert parsed["metadata"]["author"]["name"] == "Ada"


def test_loads_rejects_bad_indentation():
    source = """
root:
      child: value
""".strip()

    with pytest.raises(ParseError) as excinfo:
        loads(source)

    assert "indentation must increase by exactly one level" in str(excinfo.value)


def test_validate_semantics_rejects_numbers():
    with pytest.raises(TypeError):
        validate_semantics({"key": 10})


def test_canonical_json_serialization():
    source = """
name: Example
data:
  - alpha
  - beta
""".strip()

    parsed = loads(source)
    json_output = to_canonical_json(parsed)

    assert json.loads(json_output) == {
        "name": "Example",
        "data": ["alpha", "beta"],
    }
    assert "," in json_output and ":" in json_output

