.PHONY: help build clean test bump-patch publish release

help:
	@echo "Available targets:"
	@echo "  build       - Build sdist and wheel into dist/"
	@echo "  clean       - Remove dist/"
	@echo "  test        - Run tests"
	@echo "  bump-patch  - Increment patch version in pyproject.toml"
	@echo "  publish     - Clean, build, and upload to PyPI"
	@echo "  release     - Full release: test, bump, tag, push, gh release, publish"

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

release:
	@echo "==> Checkout and pull latest main" && \
	git checkout main && \
	git pull && \
	echo "==> Running tests" && \
	uv run pytest && \
	echo "==> Update CHANGELOG.md now, then press Enter to continue..." && \
	read _ && \
	echo "==> Bumping patch version" && \
	uv version --bump patch && \
	NEW_VERSION=$$(grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/') && \
	echo "==> Committing release v$$NEW_VERSION" && \
	git add CHANGELOG.md pyproject.toml uv.lock && \
	git commit -m "Release v$$NEW_VERSION" && \
	echo "==> Tagging v$$NEW_VERSION" && \
	git tag "v$$NEW_VERSION" && \
	echo "==> Pushing to main" && \
	git push && \
	git push --tags && \
	echo "==> Creating GitHub release" && \
	NOTES=$$(awk '/^## \['"$$NEW_VERSION"'\]/{found=1;next} /^## \[/{if(found)exit} found{print}' CHANGELOG.md) && \
	gh release create "v$$NEW_VERSION" --title "v$$NEW_VERSION" --notes "$$NOTES" && \
	echo "==> Publishing to PyPI" && \
	rm -rf dist/ && \
	uv build && \
	uv run twine upload dist/* && \
	echo "==> Released v$$NEW_VERSION"
