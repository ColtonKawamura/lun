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

- **The board is `$CWD/.lun/lun.db`.** lun resolves its data directory from
  the current working directory only; there is no environment variable or
  parent-directory search. Run every command from the directory that holds
  the board (`cd ~/repos && lun ...`). An error like
  `not-initialized: .../.lun/lun.db does not exist; run lun init first`
  means *wrong directory* — do not run `lun init` there, it would create a
  second, empty board.
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

```sh
lun status                          # one row per project, counts per status
lun status --json                   # same, machine-readable
lun status "soft pack" --board      # one project's board, one section per status
lun task ls --project "soft pack" --status review
lun task T-014                      # fields, notes, links, and the FULL history
```

`lun task T-014` prints the whole history at the bottom. When you only need
the recent past, ask the log directly:

## History: pull only what you need

```sh
lun log T-014 -3 --oneline          # the 3 newest entries of one task, one line each
lun log T-014 --oneline             # every entry of one task, one line each
lun log P-002 -5                    # 5 newest entries for a project (incl. its tasks)
lun log -5 --oneline                # 5 newest entries anywhere on the board
lun log HEAD~1                      # the 2nd-newest entry anywhere (HEAD~0 = newest)
lun grep "restitution"              # find tasks and commits by text
```

`--oneline` gives one line per entry —
`timestamp  user  ACTION  commit message | details`, e.g.
`2026-11-22 10:20  me  UPDATE  PR #12 open | status: planning -> review`.
Start with `-N --oneline`, and widen only if the answer is not there. Because
the commit message is what shows up here, write `--message` text that says
*why* the change happened.

## Writing

```sh
# Create a task: answers for the four prompts on stdin
printf 'planning\nmed\nagent-name\nopen task for X\n' | lun add task "soft pack - X" proj "soft pack"

# Move it; the --message becomes the commit line in its history
lun task T-014 --status review --message "PR #12 open for review"

# Fields (notes are markdown; [[wikilinks]] render as obsidian:// links on the web board)
lun task edit T-014 --notes "..." --labels pr-open-needs-review --message "status note"
lun task edit T-014 --branch fix-x --message "branch fix-x"

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

- `--json` works on `lun status` and `lun status <query>` only; `lun task`,
  `lun task ls` and `lun log` reject it.
- `scope` can be set from the web UI and the HTTP API, not from the CLI.
- Labels are stored and shown by `lun task`, but the web board does not
  display them.
