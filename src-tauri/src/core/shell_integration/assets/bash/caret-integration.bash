# Caret shell integration for bash — OSC 133 / OSC 7 / private OSC 7770;cmd;<base64>.
if [[ $- == *i* ]] && [[ -z "$CARET_SHELL_INTEGRATION_LOADED" ]]; then
  CARET_SHELL_INTEGRATION_LOADED=1
  __caret_in_cmd=""
  __caret_ready=""
  __caret_last_status=0
  __caret_osc() { builtin printf '\e]%s\a' "$1"; }
  __caret_b64() { builtin printf '%s' "$1" | command base64 2>/dev/null | command tr -d '\n'; }
  __caret_status() { __caret_last_status=$?; }
  __caret_precmd() {
    if [[ -n "$__caret_in_cmd" ]]; then __caret_osc "133;D;$__caret_last_status"; __caret_in_cmd=""; fi
    __caret_osc "7;file://${HOSTNAME:-localhost}${PWD}"
    __caret_osc "133;A"
    __caret_ready=1
  }
  __caret_preexec() {
    [[ -n "$COMP_LINE" ]] && return
    [[ -z "$__caret_ready" ]] && return
    [[ -n "$__caret_in_cmd" ]] && return
    case "$BASH_COMMAND" in __caret_*|*__caret_*) return;; esac
    # commands that belong to PROMPT_COMMAND are not user commands
    local __pc="${PROMPT_COMMAND[*]:-$PROMPT_COMMAND}"
    [[ -n "$__pc" && "$__pc" == *"$BASH_COMMAND"* ]] && return
    __caret_ready=""
    __caret_in_cmd=1
    local cmd
    cmd=$(HISTTIMEFORMAT= builtin history 1 2>/dev/null | sed 's/^ *[0-9]* *//')
    [[ -z "$cmd" ]] && cmd="$BASH_COMMAND"
    __caret_osc "7770;cmd;$(__caret_b64 "$cmd")"
    __caret_osc "133;B"
    __caret_osc "133;C"
  }
  if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
    PROMPT_COMMAND=(__caret_status "${PROMPT_COMMAND[@]}" __caret_precmd)
  elif [[ -n "$PROMPT_COMMAND" ]]; then
    PROMPT_COMMAND="__caret_status;${PROMPT_COMMAND};__caret_precmd"
  else
    PROMPT_COMMAND="__caret_status;__caret_precmd"
  fi
  __caret_old_debug_trap=$(trap -p DEBUG | sed "s/^trap -- '\(.*\)' DEBUG$/\1/")
  if [[ -n "$__caret_old_debug_trap" ]]; then
    trap "__caret_preexec; $__caret_old_debug_trap" DEBUG
  else
    trap '__caret_preexec' DEBUG
  fi
fi
