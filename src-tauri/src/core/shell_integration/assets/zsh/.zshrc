# Caret ZDOTDIR shim: run the user's .zshrc, then (for .zshrc) load Caret's shell integration.
CARET_ZDOTDIR="$ZDOTDIR"
if [[ -z "$CARET_USER_ZDOTDIR" ]]; then CARET_USER_ZDOTDIR="$HOME"; fi
if [[ -f "$CARET_USER_ZDOTDIR/.zshrc" ]]; then
  ZDOTDIR="$CARET_USER_ZDOTDIR"
  . "$CARET_USER_ZDOTDIR/.zshrc"
  if [[ "$ZDOTDIR" != "$CARET_USER_ZDOTDIR" ]]; then CARET_USER_ZDOTDIR="$ZDOTDIR"; fi
  ZDOTDIR="$CARET_ZDOTDIR"
fi
. "$CARET_ZDOTDIR/caret-integration.zsh"
# Hand ZDOTDIR back to the user so later files (.zlogin) and subshells behave normally.
ZDOTDIR="$CARET_USER_ZDOTDIR"
if [[ "$ZDOTDIR" == "$HOME" ]]; then unset ZDOTDIR; fi
