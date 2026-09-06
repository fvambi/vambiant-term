# Vambiant Term fish integration. Original work (MIT). Emits OSC 133 prompt
# marks and OSC 7 cwd using fish's native events. fish measures prompt
# width itself and ignores escape sequences, so no width wrapping is needed.
status is-interactive; or exit 0
set -q VAMBIANT_TERM; or exit 0
set -q __vt_integration_loaded; and exit 0
set -g __vt_integration_loaded 1

function __vt_osc7 --on-variable PWD
  printf '\033]7;file://%s%s\033\\' (hostname) "$PWD"
end
function __vt_prompt --on-event fish_prompt
  printf '\033]133;A\033\\'
  __vt_osc7
end
function __vt_preexec --on-event fish_preexec
  set -l line (string replace -a '\\' '\\x5c' -- $argv[1] | string replace -a ';' '\\x3b' | string join '\\x0a')
  printf '\033]633;E;%s\033\\' "$line"
  printf '\033]133;C\033\\'
end
function __vt_postexec --on-event fish_postexec
  printf '\033]133;D;%s\033\\' $status
end
# Mark the start of the editable region at the end of the prompt.
functions -q fish_prompt; and functions -c fish_prompt __vt_user_prompt
function fish_prompt
  __vt_user_prompt
  printf '\033]133;B\033\\'
end
