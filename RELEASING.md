# Releasing garlic

## Checklist

1. **Run tests**: `make test`
2. **Update CHANGELOG.md**: Move items from `[Unreleased]` to a new version section
3. **Bump version**: `make bump-patch` (updates `pyproject.toml`)
4. **Commit**: `git add -A && git commit -m "Bump version to X.Y.Z"`
5. **Push**: `git push`
6. **Publish to PyPI**: `make publish`
7. **Create GitHub release**: `gh release create vX.Y.Z --title "vX.Y.Z" --notes "See CHANGELOG.md"`

## Notes

- After any changes to hook commands or `garlic setup`, remind users to re-run `garlic setup` to update their hooks.
- Version is stored in `pyproject.toml` under `[project]`.
- `make bump-patch` uses `uv version --bump patch`.
- `make publish` runs `uv run twine upload dist/*` — requires PyPI credentials.
