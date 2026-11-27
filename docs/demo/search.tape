# Regenerates docs/images/search.gif for the README's lead demo.
#
# Prerequisites: `cargo build --release` (the tape records
# target/release/bouncer), vhs on PATH, run from the repo root.
#
# Story: one library across four launchers, searched the way people
# actually type. "hollow" finds both Hollow Knights plus the Epic-only Mina
# the Hollower; "gta" hits Grand Theft Auto by its initials; "elysum" is a
# typo that still finds Disco Elysium, owned on GOG and Steam both.
# Ends on the platform column, which is the point: no matter which launcher
# owns it, it's one search away.

Output "docs/images/search.gif"

Set Shell zsh
Set FontSize 14
Set Width 900
Set Height 480
Set Theme "Tomorrow Night"
Set TypingSpeed 90ms

Source docs/demo/common-setup.tape
Type "clear && bouncer"
Enter
Sleep 1s
Show

Sleep 1200ms
Type "hollow"
Sleep 2000ms
Backspace@60ms 6
Sleep 400ms

Type "gta"
Sleep 2000ms
Backspace@60ms 3
Sleep 400ms

Type "elysum"
Sleep 2500ms

Hide
Escape
Sleep 300ms
