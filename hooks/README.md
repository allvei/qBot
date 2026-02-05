# Git Hooks

This directory contains Git hooks that enforce project standards.

## Installation

To install the hooks, run:

```bash
cp hooks/pre-push .git/hooks/pre-push
chmod +x .git/hooks/pre-push
```

Or use the install script:

```bash
./hooks/install.sh
```

## Pre-push Hook

The pre-push hook enforces the following requirements before allowing a push:

1. **CHANGELOG.md must be updated** - Every push must include changes to the changelog
2. **Version must be bumped** - The version in `Cargo.toml` must be incremented

### What it checks

- ✓ CHANGELOG.md has been modified in the commits being pushed
- ✓ Cargo.toml version has been changed
- ✓ Version number has been incremented (not just modified)

### Workflow

1. Make your code changes and commit them
2. Update CHANGELOG.md with your changes
3. Bump the version in Cargo.toml (e.g., 0.4.0 → 0.4.1)
4. Commit the changelog and version bump:
   ```bash
   git add CHANGELOG.md Cargo.toml
   git commit -m "chore: bump version to 0.4.1 and update changelog"
   ```
5. Push your changes

### Bypassing the hook

If you absolutely need to bypass the hook (not recommended):

```bash
git push --no-verify
```

### Version Numbering

Follow semantic versioning (MAJOR.MINOR.PATCH):

- **MAJOR** - Breaking changes
- **MINOR** - New features (backwards compatible)
- **PATCH** - Bug fixes (backwards compatible)

Examples:
- Bug fix: 0.4.0 → 0.4.1
- New feature: 0.4.1 → 0.5.0
- Breaking change: 0.5.0 → 1.0.0
