# PRs & Git Integration

## PR Management

GitHub-style pull request tracking for tasks.

### CLI Commands

#### Create PR
```sh
lun pr new <T-00N|title> [--from <branch>] [--to <branch>]
```

```sh
lun pr new T-001 --from feature --to main
lun pr new "Fix login bug"
```

#### Show PR
```sh
lun pr show <PR-00N|task>
```

```sh
lun pr show PR-001
lun pr show T-001
```

#### List PRs
```sh
lun pr ls
```

#### Merge PR
```sh
lun pr merge <PR-00N|task>
```

```sh
lun pr merge PR-001
lun pr merge T-001
```

### Workflow

1. Create task
2. Start work on branch
3. Create PR: `lun pr new T-001 --from feature --to main`
4. Complete task: `lun task complete T-001`
5. Merge PR: `lun pr merge T-001`
6. Git merge: `git merge feature`

### Schema

PRs table:
```sql
CREATE TABLE pulls (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL,
  from_branch TEXT NOT NULL,
  to_branch TEXT NOT NULL,
  created_at TEXT NOT NULL,
  merged_at TEXT
);
```

## Git Integration

### Branch Management

Tasks can have an optional `branch` field:
```sh
lun task edit T-001 --branch feature
```

### Merge Command

After completing a task:
```sh
lun task complete T-001
lun pr merge T-001
git merge feature
```

### Optional Git

lun does not require git, but integrates when present:
- `lun pr` manages PR metadata
- Actual git operations are manual
- `branch` field is stored but not enforced

## Tips

- Use `lun pr ls` to see all open PRs
- `lun pr show T-001` shows PR for task
- PRs are linked to tasks, not projects
- Merge moves the task to `closed`
