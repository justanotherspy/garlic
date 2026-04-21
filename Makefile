.PHONY: help build clean test bump changelog release

# Default bump type (patch, minor, major)
BUMP ?= patch

help:
	@echo "Available targets:"
	@echo "  build       - Build sdist and wheel into dist/"
	@echo "  clean       - Remove dist/"
	@echo "  test        - Run tests"
	@echo "  bump        - Increment version (BUMP=patch|minor|major, default: patch)"
	@echo "  changelog   - Regenerate CHANGELOG.md from commit history"
	@echo "  release     - Create version bump PR (BUMP=patch|minor|major)"
	@echo ""
	@echo "Release workflow:"
	@echo "  1. make release BUMP=patch  - Creates PR with version bump"
	@echo "  2. Merge PR                 - Release drafter updates draft release"
	@echo "  3. Publish draft release    - GHA publishes to PyPI via OIDC"

build:
	uv build

clean:
	rm -rf dist/

test:
	uv run pytest

bump:
	uv version --bump $(BUMP)

changelog:
	git-cliff -o CHANGELOG.md

release:
	@echo "==> Checking for clean working tree" && \
	git diff --quiet && git diff --cached --quiet || (echo "Error: Working tree not clean" && exit 1) && \
	echo "==> Fetching latest main" && \
	git fetch origin main && \
	echo "==> Creating release branch from origin/main" && \
	git checkout -b release-prep-$$(date +%s) origin/main && \
	echo "==> Running tests" && \
	uv run pytest && \
	echo "==> Bumping $(BUMP) version" && \
	uv version --bump $(BUMP) && \
	NEW_VERSION=$$(grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/') && \
	echo "==> Generating changelog for v$$NEW_VERSION" && \
	git-cliff --tag "v$$NEW_VERSION" -o CHANGELOG.md && \
	echo "==> Committing release v$$NEW_VERSION" && \
	git add CHANGELOG.md pyproject.toml uv.lock && \
	git commit -m "chore(release): v$$NEW_VERSION" && \
	echo "==> Pushing branch" && \
	git push -u origin HEAD && \
	echo "==> Creating pull request" && \
	gh pr create \
		--title "chore(release): v$$NEW_VERSION" \
		--body "Bump version to v$$NEW_VERSION and update changelog." \
		--label "release" && \
	echo "==> Release PR created for v$$NEW_VERSION" && \
	echo "==> Next steps:" && \
	echo "    1. Review and merge the PR" && \
	echo "    2. Release drafter will update the draft release" && \
	echo "    3. Publish the draft release to trigger PyPI publish"
