#!/bin/sh
# Remove o TidyFlow instalado pelo install.sh. O config e o histórico ficam em
# ~/Library/Application Support/TidyFlow (apague à mão se quiser).
set -u
APP="$HOME/Applications/TidyFlow.app"
T="$APP/Contents/MacOS/tidyflow"
if [ -x "$T" ]; then
  "$T" service disable || true
  "$T" browser uninstall || true
fi
rm -rf "$APP" "$HOME/Library/Services/Organizar com o TidyFlow.workflow"
[ -L "$HOME/.local/bin/tidyflow" ] && rm -f "$HOME/.local/bin/tidyflow"
echo "TidyFlow removido."
