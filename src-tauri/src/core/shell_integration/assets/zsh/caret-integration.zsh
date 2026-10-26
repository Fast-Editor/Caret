# Caret shell integration for zsh — emits OSC 133 (prompt/command markers), OSC 7 (cwd)
# and the private OSC 7770;cmd;<base64> (command text). Disable with settings.terminal.shellIntegration=false.
if [[ -o interactive ]] && [[ -z "$CARET_SHELL_INTEGRATION_LOADED" ]]; then
  typeset -g CARET_SHELL_INTEGRATION_LOADED=1
  typeset -g __caret_in_cmd=""
  __caret_osc() { builtin printf '\e]%s\a' "$1"; }
  __caret_b64() { builtin printf '%s' "$1" | command base64 2>/dev/null | command tr -d '\n'; }
  __caret_precmd() {
    local st=$?
    if [[ -n "$__caret_in_cmd" ]]; then __caret_osc "133;D;$st"; __caret_in_cmd=""; fi
    __caret_osc "7;file://${HOST:-localhost}${PWD}"
    __caret_osc "133;A"
  }
  __caret_preexec() {
    __caret_in_cmd=1
    __caret_osc "7770;cmd;$(__caret_b64 "$1")"
    __caret_osc "133;B"
    __caret_osc "133;C"
  }
  autoload -Uz add-zsh-hook
  add-zsh-hook precmd __caret_precmd
  add-zsh-hook preexec __caret_preexec
fi
