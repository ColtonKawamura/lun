# Shell Completion

lun provides zsh and bash completion scripts.

## zsh

### Install
```sh
# Add to ~/.zshrc
eval "$(lun completion zsh)"
```

### Usage
- Tab after command: Shows available commands
- Tab after subcommand: Shows options
- Tab after flags: Shows values

```sh
lun <TAB>                    # add, attach, link, log, move, pr, rm, serve, status, task
lun rm proj <TAB>            # project names and keys; then --move-to, --cascade, --yes
lun task <TAB>               # complete, edit, ls, reopen, archive, unarchive
lun task edit T-001 --<TAB>  # title, status, priority, project, scope, assignee, branch
```

## bash

### Install
```sh
# Add to ~/.bashrc
eval "$(lun completion bash)"
```

### Usage
- Tab for completion
- Space to select
- Enter to execute

## Custom Aliases

Add to your shell config:

```sh
alias ls='lun status --board'
alias ll='lun task ls'
```

## Tips

- Use quotes for multi-word arguments: `lun add task "My task" proj "My project"`
- Completion works with `--json` flag
- TUI also has completion (press `/` then tab)
