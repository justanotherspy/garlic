.PHONY: help build clean test

help:
	@echo "Available targets:"
	@echo "  build       - Build sdist and wheel into dist/"
	@echo "  clean       - Remove dist/"
	@echo "  test        - Run tests"
	@echo "  bump-patch  - Increment patch version in pyproject.toml"

build:
	uv build

clean:
	rm -rf dist/

test:
	uv run pytest

bump-patch:
	uv version --bump patch
