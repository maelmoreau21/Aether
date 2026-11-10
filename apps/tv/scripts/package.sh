#!/usr/bin/env sh
# Assemble et empaquette la coquille TV d'Aether.
#
#   apps/tv/scripts/package.sh tizen [profil-de-signature]   -> apps/tv/dist/Aether.wgt   (Tizen CLI)
#   apps/tv/scripts/package.sh webos                         -> apps/tv/dist/*.ipk        (ares-package)
#   apps/tv/scripts/package.sh stage                         -> apps/tv/build/{tizen,webos} sans empaqueter
#
# La coquille commune (shell/) est copiée à côté des fichiers propres à chaque plateforme.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build="$root/build"
dist="$root/dist"

stage() {
  platform=$1
  rm -rf "$build/$platform"
  mkdir -p "$build/$platform"
  cp "$root"/shell/index.html "$root"/shell/shell.css "$root"/shell/shell.js "$root"/shell/logo.svg "$build/$platform/"
  cp "$root/$platform"/* "$build/$platform/"
  echo "assemblé : $build/$platform"
}

case "${1:-}" in
  tizen)
    stage tizen
    command -v tizen >/dev/null 2>&1 || { echo "Tizen CLI introuvable : installez Tizen Studio (CLI) et ajoutez tizen-studio/tools/ide/bin au PATH." >&2; exit 1; }
    profile=${2:-${TIZEN_PROFILE:-}}
    [ -n "$profile" ] || { echo "Profil de signature manquant : package.sh tizen <profil> (Certificate Manager de Tizen Studio)." >&2; exit 1; }
    mkdir -p "$dist"
    tizen package -t wgt -s "$profile" -o "$dist" -- "$build/tizen"
    ;;
  webos)
    stage webos
    command -v ares-package >/dev/null 2>&1 || { echo "ares-package introuvable : npm install -g @webos-tools/cli" >&2; exit 1; }
    mkdir -p "$dist"
    ares-package --no-minify -o "$dist" "$build/webos"
    ;;
  stage)
    stage tizen
    stage webos
    ;;
  *)
    echo "usage : $0 tizen <profil> | webos | stage" >&2
    exit 2
    ;;
esac
