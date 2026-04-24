## [0.1.11] - 2026-04-24

### 🚀 Features

- *(engine)* Add max_generation_minutes cap (garlic-5p0) (#27)
- *(cli)* Add garlic stats command for monthly summary (garlic-yyy) (#34)
- *(hooks)* Migrate nudge delivery to additionalContext JSON response (JUS-19) (#41)

### 🐛 Bug Fixes

- *(tests)* Use shared garlic_env fixture for ephemeral test dirs (garlic-bhv) (#21)
- *(nudges)* Prevent '1h 60m' in time formatting (garlic-57p) (#22)
- *(cli)* Validate threshold lists are sorted and positive (garlic-85s) (#26)
- *(reset)* Clear bedtime_nudge_given on garlic reset (JUS-12) (#45)
- *(ci)* Skip code review on draft PR open event (JUS-34) (#46)
- *(format)* Consolidate time formatting to single helper with floor rounding (JUS-13) (#47)
- *(hooks)* Avoid double save in bedtime branch (JUS-14) (#49)
- *(cli)* Derive default nudge interval from DEFAULTS (JUS-15) (#50)

### 💼 Other

- Update README and CLAUDE.md to reflect current project state (#37)
- Set up secure automated release workflow (#38)
- Install SessionEnd hook to finalize session state (#51)
- Support `garlic --version` alongside `garlic version` (#52)

### 🚜 Refactor

- *(workflow)* Replace beads with Linear for issue tracking (JUS-6) (#36)

### 📚 Documentation

- Reflect max_generation_minutes cap in README prose (garlic-3ag) (#33)
- *(template)* Update PR template with Linear issue link (JUS-10) (#40)

### ⚙️ Miscellaneous Tasks

- *(review)* Skip Claude code review for Dependabot PRs (#25)
- Add explicit permissions to CI workflow (#29)
- *(hooks)* Remove stale bd prime hooks from repo settings (JUS-9) (#39)
- *(test)* Add Python 3.13 to test matrix (JUS-33) (#42)
## [0.1.10] - 2026-03-29

### 🚀 Features

- *(nudges)* Show exact time in nudge messages instead of approximate (garlic-u78) (#19)
- *(cli)* Add weekly usage tracking with garlic week command (garlic-we0) (#20)

### 🐛 Bug Fixes

- *(setup)* Print CLAUDE.md install confirmation line (garlic-0kd) (#18)

### ⚙️ Miscellaneous Tasks

- *(release)* V0.1.10
## [0.1.9] - 2026-03-19

### 🐛 Bug Fixes

- *(setup)* Deduplicate nudge relay and document CLAUDE.md install step (#17)

### ⚙️ Miscellaneous Tasks

- *(release)* V0.1.9
## [0.1.8] - 2026-03-19

### 🚀 Features

- *(setup)* Install nudge-relay instruction into ~/.claude/CLAUDE.md (garlic-w16) (#16)

### 🐛 Bug Fixes

- *(nudges)* Improve time accuracy and simplify status output (garlic-6re) (#15)

### ⚙️ Miscellaneous Tasks

- *(release)* V0.1.8
## [0.1.7] - 2026-03-19

### 🚀 Features

- Adopt conventional commits and git-cliff for automated changelogs (garlic-djp) (#14)

### ⚙️ Miscellaneous Tasks

- *(release)* V0.1.7
## [0.1.0] - 2026-03-16
