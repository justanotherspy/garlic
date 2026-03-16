.PHONY: help build clean test bump-patch publish

help:
	@echo "Available targets:"
	@echo "  build       - Build sdist and wheel into dist/"
	@echo "  clean       - Remove dist/"
	@echo "  test        - Run tests"
	@echo "  bump-patch  - Increment patch version in pyproject.toml"
	@echo "  publish     - Clean, build, and upload to PyPI"

build:
	uv build

clean:
	rm -rf dist/

test:
	uv run pytest

bump-patch:
	uv version --bump patch

publish: clean build
	uv run twine upload dist/*
