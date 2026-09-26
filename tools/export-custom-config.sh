#!/usr/bin/env bash
# Run as your regular Fedora user. Copies only known UI configuration folders.
set -euo pipefail
out="${1:-$PWD/vicios-custom-config.tar.gz}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/config"
for name in hypr quickshell waybar rofi swaync alacritty fastfetch; do
    if [[ -d "$HOME/.config/$name" ]]; then cp -a "$HOME/.config/$name" "$tmp/config/"; fi
done
if command -v hyprctl >/dev/null; then
    hyprctl version > "$tmp/hyprland-version.txt" || true
    hyprctl monitors -j > "$tmp/monitors.json" || true
fi
cat > "$tmp/README.txt" <<'NOTE'
These are real user UI configs, not a reconstructed approximation.
Review scripts before sharing: they can contain home paths, network details or tokens.
Wallpaper files referenced outside these folders must be supplied separately.
NOTE
tar -czf "$out" -C "$tmp" .
printf 'Created: %s\n' "$out"
