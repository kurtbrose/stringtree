import importlib


def test_package_importable():
    assert importlib.import_module("stringtree") is not None
