#!/bin/sh
# Instala o TidyFlow a partir do pacote de release (TidyFlow-<versão>-linux-x64.tar.gz).
# Uso: ./install.sh            instala em ~/.local/bin
#      PREFIX=/opt/x ./install.sh   instala em /opt/x/bin
# Não liga o daemon: revise as regras antes (o exemplo manda para a lixeira o que tem mais de 30 dias).
set -eu

cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"

install -Dm755 bin/tidyflow "$BIN/tidyflow"
install -Dm755 bin/tidyflow-app "$BIN/tidyflow-app"
echo "binários em $BIN"

# Host de native messaging para a extensão de navegador (só onde há navegador).
if "$BIN/tidyflow" browser install >/dev/null; then
  echo "host da extensão de navegador registrado"
else
  echo "aviso: não foi possível registrar o host da extensão (rode: tidyflow browser install)"
fi

if [ -e "$CONFIG/tidyflow/config.toml" ]; then
  echo "config mantido: $CONFIG/tidyflow/config.toml"
else
  install -Dm644 config.example.toml "$CONFIG/tidyflow/config.toml"
  echo "config de exemplo copiado para $CONFIG/tidyflow/config.toml"
fi

mkdir -p "$CONFIG/systemd/user"
sed "s|^ExecStart=.*|ExecStart=$BIN/tidyflow daemon|" contrib/tidyflow.service > "$CONFIG/systemd/user/tidyflow.service"
systemctl --user daemon-reload 2>/dev/null || true

mkdir -p "$DATA/applications"
sed "s|^Exec=.*|Exec=$BIN/tidyflow-app|" contrib/tidyflow.desktop > "$DATA/applications/tidyflow.desktop"
for size in 32x32 64x64 128x128; do
  install -Dm644 "icons/$size.png" "$DATA/icons/hicolor/$size/apps/tidyflow.png"
done
install -Dm644 icons/256x256.png "$DATA/icons/hicolor/256x256/apps/tidyflow.png"

if command -v nautilus >/dev/null 2>&1; then
  mkdir -p "$DATA/nautilus/scripts"
  cp -r contrib/nautilus/TidyFlow "$DATA/nautilus/scripts/"
  echo "menu de contexto do Nautilus instalado (Scripts → TidyFlow)"
fi
if command -v nemo >/dev/null 2>&1; then
  mkdir -p "$DATA/nemo/actions"
  cp contrib/nemo/*.nemo_action "$DATA/nemo/actions/"
  echo "menu de contexto do Nemo instalado"
fi

cat <<EOF

Pronto. Próximos passos:
  1. Revise as regras em $CONFIG/tidyflow/config.toml
  2. tidyflow check && tidyflow run --dry-run
  3. tidyflow service enable   (liga o daemon no login)
  4. Opcional: instale a extensão de navegador (veja o README, seção "Extensão de navegador")
EOF
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) echo "Aviso: $BIN não está no PATH." ;;
esac
