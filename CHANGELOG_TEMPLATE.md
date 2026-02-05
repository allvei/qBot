# Changelog Template

Use this template when updating the changelog for a new version.

## Format

```markdown
__DD.MM.YY__
## Users & Admins
__Added__
- User-facing feature 1
- User-facing feature 2

__Improved__
- User-facing improvement 1
- User-facing improvement 2

__Fixed__
- User-facing bug fix 1
- User-facing bug fix 2

## Developers
__Added__
- Technical feature 1
- Technical feature 2

__Improved__
- Technical improvement 1
- Technical improvement 2

__Fixed__
- Technical bug fix 1
- Technical bug fix 2

__Refactored__
- Code refactoring 1
- Code refactoring 2

## Known Issues
- Known issue 1
- Known issue 2
```

## Section Guidelines

### Users & Admins
**Purpose:** Changes that directly affect end users and server administrators.
**Audience:** Discord community members who use the bot.
**Examples:**
- New commands or features users can interact with
- UI/UX improvements on dashboards
- Bug fixes that affect user experience
- Admin tools and configuration options
- Changes to how queues, games, or ELO work

**What NOT to include:**
- Internal code changes
- Performance optimizations (unless user-visible)
- Database schema changes
- API changes
- Development tools

### Developers
**Purpose:** Technical changes for developers and contributors.
**Audience:** GitHub contributors, developers maintaining the codebase.
**Examples:**
- Performance optimizations and caching
- Database schema changes
- Code refactoring
- API changes
- Development tools (hooks, scripts, etc.)
- Internal architecture improvements
- Dependency updates

**What NOT to include:**
- User-facing features (put in Users & Admins)
- Changes that don't affect code or development

## Categories

### __Added__
New features, commands, or functionality.

### __Improved__
Enhancements to existing features.

### __Fixed__
Bug fixes and corrections.

### __Refactored__ (Developers only)
Code restructuring without changing functionality.

## Tips

1. **Be concise** - Users don't need technical details
2. **Be specific** - Developers need enough detail to understand changes
3. **Use present tense** - "Add feature" not "Added feature"
4. **Group related changes** - Combine similar items with sub-bullets
5. **User perspective** - Write from the user's point of view for Users & Admins section

## Example

```markdown
__05.02.26__
## Users & Admins
__Added__
- Confirmation prompt when changing player ELO outside their current rank range.
  - Shows current vs new rank and ELO values
  - Automatically updates Discord roles

__Fixed__
- Player rank now correctly determined from Discord roles.
- Missing default rank configuration now shows helpful error message.

## Developers
__Added__
- Discord tag caching system for improved performance.
- Pre-push Git hook to enforce changelog and version updates.

__Refactored__
- Database schema restructured for better foreign key integrity.
- Player.rank changed to Option<Rank> for safer null handling.
```

## Publishing

**For Discord (Users & Admins section only):**
Copy only the "Users & Admins" section to post in your Discord announcements channel.

**For GitHub (Full changelog):**
The complete changelog with both sections is available in CHANGELOG.md for developers and contributors.
