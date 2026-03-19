.PHONY: help build clean test bump-patch publish changelog release

help:
	@echo "Available targets:"
	@echo "  build       - Build sdist and wheel into dist/"
	@echo "  clean       - Remove dist/"
	@echo "  test        - Run tests"
	@echo "  bump-patch  - Increment patch version in pyproject.toml"
	@echo "  publish     - Clean, build, and upload to PyPI"
	@echo "  changelog   - Regenerate CHANGELOG.md from commit history"
	@echo "  release     - Full release: test, changelog, bump, tag, push, gh release, publish"

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

changelog:
	git-cliff -o CHANGELOG.md

release:
	@echo "==> Checkout and pull latest main" && \
	git checkout main && \
	git pull && \
	echo "==> Running tests" && \
	uv run pytest && \
	echo "==> Bumping patch version" && \
	uv version --bump patch && \
	NEW_VERSION=$$(grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/') && \
	echo "==> Generating changelog for v$$NEW_VERSION" && \
	git-cliff --tag "v$$NEW_VERSION" -o CHANGELOG.md && \
	echo "==> Committing release v$$NEW_VERSION" && \
	git add CHANGELOG.md pyproject.toml uv.lock && \
	git commit -m "chore(release): v$$NEW_VERSION" && \
	echo "==> Tagging v$$NEW_VERSION" && \
	git tag "v$$NEW_VERSION" && \
	echo "==> Pushing to main" && \
	git push && \
	git push --tags && \
	echo "==> Creating GitHub release" && \
	NOTES=$$(git-cliff --latest --strip header) && \
	gh release create "v$$NEW_VERSION" --title "v$$NEW_VERSION" --notes "$$NOTES" && \
	echo "==> Publishing to PyPI" && \
	rm -rf dist/ && \
	uv build && \
	uv run twine upload dist/* && \
	echo "==> Released v$$NEW_VERSION"
