# Vambiant Term zsh integration. Original work (MIT), not derived from any
# other terminal's script. Emits OSC 133 prompt marks and OSC 7 cwd so the
# terminal can segment output into blocks. Adds itself to the existing hook
# arrays instead of replacing them, and stays silent in non-interactive
# shells. If it detects that another precmd/preexec hook prints (which
# corrupts the marks — Powerlevel10k, starship), it still emits marks; the
# terminal decides whether they are trustworthy.
[[ -o interactive ]] || return
[[ -n "$VAMBIANT_TERM" ]] || return
typeset -g __vt_integration_loaded
[[ -z "$__vt_integration_loaded" ]] || return
__vt_integration_loaded=1

autoload -Uz add-zsh-hook

__vt_osc7() {
  printf '\033]7;file://%s%s\033\\' "${HOST}" "${PWD}"
}
__vt_precmd() {
  local exit=$?
  # Close the previous command, if one was running.
  [[ -n "$__vt_cmd_active" ]] && printf '\033]133;D;%s\033\\' "$exit"
  __vt_cmd_active=
  __vt_osc7
  printf '\033]133;A\033\\'
}
# 633;E carries the command line so a block can be titled; `;` and `\`
# are escaped as \x3b and \x5c, newlines as \x0a (the 633 convention).
__vt_escape() {
  local s=$1
  s=${s//\\/\\x5c}
  s=${s//;/\\x3b}
  s=${s//$'\n'/\\x0a}
  print -rn -- "$s"
}
__vt_preexec() {
  printf '\033]633;E;%s\033\\' "$(__vt_escape "$1")"
  printf '\033]133;C\033\\'
  __vt_cmd_active=1
}
# The command-start mark (B) is emitted by the prompt itself so it lands at
# the right column; a widget cannot know it. zle keeps the position stable
# across line edits.
__vt_mark_input() { print -n '\033]133;B\033\\' }

add-zsh-hook precmd __vt_precmd
add-zsh-hook preexec __vt_preexec
# Wrap the prompt so B is emitted just before the editable region. %{...%}
# tells zsh the bytes are zero-width, so line editing counts columns right.
PROMPT="%{$(__vt_mark_input)%}$PROMPT"
