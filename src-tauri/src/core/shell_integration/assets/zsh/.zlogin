# Caret ZDOTDIR shim: run the user's .zlogin, then (for .zshrc) load Caret's shell integration.
CARET_ZDOTDIR="$ZDOTDIR"
if [[ -z "$CARET_USER_ZDOTDIR" ]]; then CARET_USER_ZDOTDIR="$HOME"; fi
if [[ -f "$CARET_USER_ZDOTDIR/.zlogin" ]]; then
  ZDOTDIR="$CARET_USER_ZDOTDIR"
  . "$CARET_USER_ZDOTDIR/.zlogin"
  if [[ "$ZDOTDIR" != "$CARET_USER_ZDOTDIR" ]]; then CARET_USER_ZDOTDIR="$ZDOTDIR"; fi
  ZDOTDIR="$CARET_ZDOTDIR"
fi
