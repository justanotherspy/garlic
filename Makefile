.PHONY: help build clean test

help:
	@echo "Available targets:"
	@echo "  build  - Build sdist and wheel into dist/"
	@echo "  clean  - Remove dist/"
	@echo "  test   - Run tests"

build:
	uv build

clean:
	rm -rf dist/

test:
	uv run pytest
