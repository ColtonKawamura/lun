# Using lun from agents

lun has two audiences, and the design serves both:

1. **Agents (and scripts) drive it from the CLI.** Every operation is a plain
   command with plain-text output, so an agent can file, move, and annotate
   tasks with ordinary shell calls.
2. **Every change is a commit.** Each task carries a git-style history
   (who/what/why, newest first). An agent can pull just the slice it needs —
   the last three entries of one task — instead of loading everything into its
   context.
3. **The person gets a UI.** The same database backs the web board
   (`lun serve`, a phone-friendly PWA) and the full-screen TUI, so the human
   sees and edits exactly what the agents wrote.

This page is the agent-facing cheat sheet. Full command reference:
[CLI Commands](cli-commands.md).

## Ground rules

- **The board is the nearest `.lun/lun.db` at or above the current
  directory**, found the way git finds a repo. The search stops at `$HOME`
  (it never uses a `~/.lun`), and there is no environment variable. Run
  commands from anywhere under the board's directory (`cd ~/repos && lun ...`
  or any subdirectory); `lun root` prints which board is in use, and a
  command that had to walk up says `lun: using board at <dir>` on stderr.
  An error like `not-initialized: no lun board found in ... or any parent
  directory` means *wrong directory* — do not run `lun init` there, it would
  create a second, empty board.
- **Several boards** (for example work and personal) are just several
  directories that each ran `lun init` once, e.g. `~/repos` and
  `~/personal-board`, each with its own `lun serve` on its own port. Which
  board a command uses depends only on where it runs. `lun init` never walks
  up: inside an existing board it creates a separate nested board and says so.
- **Never block on a prompt.** Changes ask for a commit message on stdin
  (`add task` also asks for status, priority and assignee). Pass
  `--message "..."` where the command takes it (`add proj`, `proj`, `move`,
  `task --status`, `task edit`) and close stdin (`</dev/null`) everywhere
  else to accept the defaults.
- **Safe alongside the UI.** The database runs in WAL mode with a busy
  timeout, so CLI writes while `lun serve` is up are fine; the web board shows
  them on its next refresh.

## Statuses

Tasks move through five columns, left to right:

| Status | Meaning |
|---|---|
| `planning` | Scope still being worked out. New tasks start here. |
| `review` | Back with results, a decision to make, or a PR to review. |
| `waiting-on` | Out with other people or an external process (email, referee, CI, cluster jobs). |
| `backlog` | Parked for later. |
| `closed` | Finished. `lun task complete` and `lun pr merge` land here. |

The pre-v9 names still work as input: `todo`/`doing` → `planning`,
`follow-up` → `review`, `blocked` → `waiting-on`, `done` → `closed`.
Who may set which status is a convention of the team using the board, not
something lun enforces.

## Reading state cheaply

Load the least that answers the question; widen only if it does not.

```sh
lun brief                           # session start: ONE LINE per open task (closed hidden)
lun brief --project "soft pack" --last   # ...for one project, plus each task's newest commit message
lun brief --status review --json    # machine-readable (key,title,status,priority,project_key,scope)
lun status                          # one row per project, counts per status
lun status --json                   # same, machine-readable
lun status "soft pack" --board      # one project's board, one section per status
lun task ls --project "soft pack" --status review --scope physics
lun task ls --fields key,status,title   # only the columns you ask for, one line per task
lun task ls --json --fields key,status  # same as JSON (key order preserved)
```

Reading one task:

```sh
lun task T-014 --brief              # fields, notes, links, PRs — history omitted (count only)
lun task T-014 --log 3              # ...plus only the 3 newest history entries
lun task T-014 --fields status,notes   # just those fields, "name: value" lines
lun task T-014 --json --log 3       # JSON incl. links, history_count and the 3 newest entries
lun task T-014                      # fields, notes, links, and the FULL history (largest output)
```

`--fields` takes any of `key,title,status,priority,assignee,branch,scope,labels,project,project_key,notes,created_at,updated_at,archived`
(plus `links`, `history_count`, `history` on a single task); an unknown name is an error that lists the valid ones.

## History: pull only what you need

```sh
lun log T-014 -3 --oneline          # the 3 newest entries of one task, one line each
lun log T-014 --oneline             # every entry of one task, one line each
lun log P-002 -5                    # 5 newest entries for a project (incl. its tasks)
lun log -5 --oneline                # 5 newest entries anywhere on the board
lun log HEAD~1                      # the 2nd-newest entry anywhere (HEAD~0 = newest)
lun log --since 120 --oneline       # only entries with id > 120, anywhere (poll for changes)
lun log T-014 --since 120 --json    # same for one entity, JSON (id, entity, user, action, message, details)
lun grep "restitution"              # find tasks and commits by text
```

`--oneline` gives one line per entry —
`timestamp  user  ACTION  commit message | details`, e.g.
`2026-11-22 10:20  me  UPDATE  PR #12 open | status: planning -> review`.
With `--since` or `--json` each entry also carries its log id and entity key
(`#121  T-014  ...`), so remember the highest id you have seen and ask for
`--since <that id>` next time — an empty result is the cheap "nothing changed".
Start with `-N --oneline`, and widen only if the answer is not there. Because
the commit message is what shows up here, write `--message` text that says
*why* the change happened.

## Writing

```sh
# Create a task with flags (no stdin prompts); unspecified ones default to planning / low / me
lun add task "soft pack - X" proj "soft pack" --status planning --priority med --assignee agent-name --scope physics --message "open task for X"
# (the old four-line stdin protocol still works when no flag is given)

# Move it; the --message becomes the commit line in its history
lun task T-014 --status review --message "PR #12 open for review"

# Fields (notes are markdown; [[wikilinks]] render as obsidian:// links on the web board)
lun task edit T-014 --notes "..." --labels pr-open-needs-review --message "status note"
lun task edit T-014 --branch fix-x --message "branch fix-x"

# Append to notes instead of rewriting them (adds a blank line between entries)
lun task edit T-014 --append-notes "2026-11-22: jobs resubmitted" --message "progress note"
lun task edit T-014 --scope physics --message "set scope"       # --scope none clears it

# Several tasks at once (comma list, no spaces): every key is resolved first, so a typo changes nothing
lun task T-014,T-015,T-016 --status review --message "PRs open"
lun task edit T-014,T-015 --labels pr-open-needs-review --priority high

# Free-form commit entry
lun log T-014 -m "jobs 4812331-2 submitted"

# Links and PR records (these read a commit message from stdin: close it)
lun link task T-014 "PR #12" "https://github.com/me/repo/pull/12" </dev/null
lun pr new T-014 --to main </dev/null
```

Each write prints a one- or two-line confirmation (`Updated T-014: ...` /
`Committed: ...`); read the task back with `lun task T-014` or
`lun log T-014 -1` when you need to verify.

## Gaps to know about

- `--json` works on `lun status [query]`, `lun task <T>`, `lun task ls`, `lun log` and `lun brief`.
  It is not available on write commands.
- Bulk edit supports `--status --priority --assignee --labels --scope --project --append-notes --message`;
  `--title`, `--notes` and `--branch` are per-task.
- `lun log HEAD~N` does not combine with `--json`/`--since` (use `-N`).
- Labels are stored and shown by `lun task`, but the web board does not
  display them.
