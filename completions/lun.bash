# bash completion for lun
# Tip: `bind 'TAB:menu-complete'` enables cycle-on-repeated-Tab behavior.

_lun_complete() {
  local cur
  cur="${COMP_WORDS[COMP_CWORD]}"

  local -a candidates
  mapfile -t candidates < <(lun complete -- "${COMP_WORDS[@]}" 2>/dev/null)

  COMPREPLY=()
  local c
  for c in "${candidates[@]}"; do
    [[ "$c" == "$cur"* ]] && COMPREPLY+=("$c")
  done
}

complete -F _lun_complete lun
