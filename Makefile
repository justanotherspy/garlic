.PHONY: help build clean test fmt lint release

# Default bump type (patch, minor, major)
BUMP ?= patch

help:
	@echo "Available targets:"
	@echo "  build       - Build the release binary into target/release/"
	@echo "  clean       - Remove target/"
	@echo "  test        - Run tests"
	@echo "  fmt         - Format the code"
	@echo "  lint        - Check formatting and run clippy (-D warnings)"
	@echo "  release     - Create version bump PR (BUMP=patch|minor|major)"
	@echo ""
	@echo "Release workflow:"
	@echo "  1. make release BUMP=patch  - Creates PR with version bump (needs cargo-edit)"
	@echo "  2. Merge PR                 - Release drafter updates draft release"
	@echo "  3. Publish draft release    - GHA ships to crates.io + uploads binaries"

build:
	cargo build --release

clean:
	cargo clean

test:
	cargo test

fmt:
	cargo fmt

lint:
	cargo fmt --check && cargo clippy --all-targets -- -D warnings

# Requires cargo-edit for `cargo set-version`: cargo install cargo-edit
release:
	@echo "==> Checking for clean working tree" && \
	git diff --quiet && git diff --cached --quiet || (echo "Error: Working tree not clean" && exit 1) && \
	echo "==> Fetching latest main" && \
	git fetch origin main && \
	echo "==> Creating release branch from origin/main" && \
	git checkout -b release-prep-$$(date +%s) origin/main && \
	echo "==> Running tests" && \
	cargo test && \
	echo "==> Bumping $(BUMP) version" && \
	cargo set-version --bump $(BUMP) && \
	NEW_VERSION=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/') && \
	git add Cargo.toml Cargo.lock && \
	git commit -m "chore(release): v$$NEW_VERSION" && \
	echo "==> Pushing branch" && \
	git push -u origin HEAD && \
	echo "==> Creating pull request" && \
	gh pr create \
		--title "chore(release): v$$NEW_VERSION" \
		--body "Bump version to v$$NEW_VERSION." \
		--label "release" && \
	echo "==> Release PR created for v$$NEW_VERSION" && \
	echo "==> Next steps:" && \
	echo "    1. Review and merge the PR" && \
	echo "    2. Release drafter will update the draft release" && \
	echo "    3. Publish the draft release to trigger crates.io publish + binary uploads"
