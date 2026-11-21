#!/bin/sh
# Instala o TidyFlow no macOS a partir do pacote (TidyFlow-<versão>-macos-universal.zip).
# Uso, no Terminal, dentro da pasta extraída:  sh install.sh
# Não liga o daemon: revise as regras antes (o exemplo manda para a lixeira o que tem
# mais de 30 dias).
set -eu

cd "$(dirname "$0")"
APPS="$HOME/Applications"
BIN="$HOME/.local/bin"
SUPPORT="$HOME/Library/Application Support/TidyFlow"
APP="$APPS/TidyFlow.app"

mkdir -p "$APPS" "$BIN"
rm -rf "$APP"
cp -R TidyFlow.app "$APPS/"
# Sem assinatura da Apple: tira a quarentena do download para o Gatekeeper não bloquear.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
ln -sf "$APP/Contents/MacOS/tidyflow" "$BIN/tidyflow"
echo "app em $APP; comando em $BIN/tidyflow"

if [ -e "$SUPPORT/config.toml" ]; then
  echo "config mantido: $SUPPORT/config.toml"
else
  mkdir -p "$SUPPORT"
  cp config.example.toml "$SUPPORT/config.toml"
  echo "config de exemplo copiado para $SUPPORT/config.toml"
fi

if "$BIN/tidyflow" browser install >/dev/null; then
  echo "host da extensão de navegador registrado"
else
  echo "aviso: não foi possível registrar o host da extensão (rode: tidyflow browser install)"
fi

mkdir -p "$HOME/Library/Services"
rm -rf "$HOME/Library/Services/Organizar com o TidyFlow.workflow"
cp -R "Organizar com o TidyFlow.workflow" "$HOME/Library/Services/"
xattr -dr com.apple.quarantine "$HOME/Library/Services/Organizar com o TidyFlow.workflow" 2>/dev/null || true
echo "Ação Rápida do Finder instalada (botão direito → Ações Rápidas → Organizar com o TidyFlow)"

cat <<MSG

Pronto. Próximos passos:
  1. Revise as regras em "$SUPPORT/config.toml"
  2. tidyflow check && tidyflow run --dry-run
  3. tidyflow service enable   (liga o daemon no login)
     Se o macOS perguntar, permita o acesso do tidyflow à pasta Downloads.
  4. Opcional: instale a extensão de navegador (veja o README)
MSG
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) echo "Aviso: $BIN não está no PATH (adicione ao ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\")." ;;
esac
