# Vambiant Term bash integration. Original work (MIT). Emits OSC 133 prompt
# marks and OSC 7 cwd. The prompt-start mark MUST be wrapped in \[ \] so
# readline does not count its bytes as visible width (docs/10 §10); the
# other marks are emitted from PROMPT_COMMAND and a DEBUG trap, which run
# outside the visible prompt, so they need no wrapping.
case "$-" in *i*) ;; *) return ;; esac
[ -n "$VAMBIANT_TERM" ] || return
[ -z "$__vt_integration_loaded" ] || return
__vt_integration_loaded=1

__vt_osc7() { printf '\033]7;file://%s%s\033\\' "$HOSTNAME" "$PWD"; }
__vt_precmd() {
  local exit=$?
  [ -n "$__vt_cmd_active" ] && printf '\033]133;D;%s\033\\' "$exit"
  __vt_cmd_active=
  __vt_osc7
  printf '\033]133;A\033\\'
}
__vt_preexec() {
  # The DEBUG trap fires before every simple command; only the first one
  # after a prompt is the user's command line.
  [ -n "$__vt_cmd_active" ] && return
  case "$BASH_COMMAND" in __vt_precmd) return ;; esac
  printf '\033]133;C\033\\'
  __vt_cmd_active=1
}
# B goes in PS1, wrapped so its width is zero.
PS1='\[\033]133;B\033\\\]'"$PS1"
case "$PROMPT_COMMAND" in
  *__vt_precmd*) ;;
  '') PROMPT_COMMAND=__vt_precmd ;;
  *) PROMPT_COMMAND="__vt_precmd;$PROMPT_COMMAND" ;;
esac
trap '__vt_preexec' DEBUG
