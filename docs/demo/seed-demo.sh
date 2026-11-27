#!/usr/bin/env bash
# Seeds a throwaway $HOME for the docs/demo tapes and prints its path.
#
# Usage (from a tape, repo root as cwd):
#   export HOME=$(docs/demo/seed-demo.sh)
#
# bouncer resolves its cache, config, and queue paths from $HOME, so
# redirecting it isolates all three at once and the recording never touches
# the real library or queue. The caches are stamped two hours old: fresh
# enough that the TUI skips its first-run sync (no network, no Steam key),
# and old enough that the header shows a believable "synced 2h ago".
#
# Titles come from a real synced library, spelling and all (™, ®, colons), so the
# demos exercise the same name shapes search has to handle in practice.
set -euo pipefail

home=$(cd "$(mktemp -d)" && pwd -P)
cache="$home/.cache/bouncer"
share="$home/.local/share/bouncer"
mkdir -p "$cache" "$share"

stamp=$(date -u -v-2H +%Y-%m-%dT%H:%M:%S+00:00)

# write_cache <platform> <source>, titles on stdin one per line.
write_cache() {
  local platform=$1 source=$2 first=1
  {
    printf '{"last_updated":"%s","source":"%s","games":[' "$stamp" "$source"
    while IFS= read -r name; do
      [ -z "$name" ] && continue
      [ $first -eq 1 ] || printf ','
      first=0
      printf '{"name":"%s","platform":"%s"}' "$name" "$platform"
    done
    printf ']}\n'
  } > "$cache/$platform.json"
}

write_cache steam steam_api <<'EOF'
Half-Life 2
Portal
Portal 2
Hollow Knight
Hollow Knight: Silksong
Hades
Celeste
Disco Elysium - The Final Cut
Outer Wilds
Grand Theft Auto V
Grand Theft Auto IV: The Complete Edition
Okami HD
NieR:Automata™
DARK SOULS™: Prepare To Die Edition
Darkest Dungeon®
Spiritfarer®: Farewell Edition
BioShock™ Remastered
Sleeping Dogs™
Stardew Valley
Slay the Spire
Into the Breach
Return of the Obra Dinn
Inscryption
Vampire Survivors
1000xRESIST
EOF

write_cache epic heroic_cache <<'EOF'
Batman™: Arkham Knight
Batman: Arkham City™
Control
Death Stranding
Dishonored®: Death of the Outsider™
Far Cry® 3 Blood Dragon
Hades
HUMANKIND™
Kingdoms of Amalur: Reckoning™
LEGO® Builder's Journey
Middle-earth™: Shadow of War™
Mina the Hollower
Outer Wilds
Sid Meier's Civilization® VI
Subnautica
Tacoma
What Remains of Edith Finch
EOF

write_cache gog heroic_cache <<'EOF'
Cyberpunk 2077
Disco Elysium
The Witcher 3: Wild Hunt
Baldur's Gate 3
Frostpunk
Pentiment
Heroes of Might and Magic® 3: Complete
Planescape: Torment: Enhanced Edition
EOF

write_cache amazon nile <<'EOF'
Fallout New Vegas®: Honest Hearts
Spyro™ Reignited Trilogy
Tomb Raider GOTY
Ni no Kuni Wrath of the White Witch™ Remastered
Retro City Rampage™ DX
EOF

# Entry order is queue rank. Outer Wilds is both queued and played (a
# replay), so the played filter and the queued filter overlap on screen.
cat > "$share/queue.json" <<'EOF'
{
  "version": 1,
  "entries": [
    {"key": "hollow knight: silksong", "name": "Hollow Knight: Silksong", "queued": true, "played": false},
    {"key": "disco elysium - the final cut", "name": "Disco Elysium - The Final Cut", "queued": true, "played": false},
    {"key": "outer wilds", "name": "Outer Wilds", "queued": true, "played": true},
    {"key": "pentiment", "name": "Pentiment", "queued": true, "played": false},
    {"key": "hades", "name": "Hades", "queued": false, "played": true},
    {"key": "celeste", "name": "Celeste", "queued": false, "played": true},
    {"key": "portal 2", "name": "Portal 2", "queued": false, "played": true}
  ]
}
EOF

# The tapes run `zsh -f` (no rc files, so a themed prompt cannot fight
# `Set Theme`), so the prompt is set here rather than typed in the tape.
cat > "$home/.demo-prompt" <<'EOF'
PROMPT='$ '
EOF

echo "$home"
