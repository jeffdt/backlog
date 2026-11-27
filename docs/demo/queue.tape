# Regenerates docs/images/queue.gif for the README's Queue section.
#
# Prerequisites: same as search.tape.
#
# Story: the seeded queue already holds four games (seed-demo.sh). Search
# for Into the Breach and queue it with Enter, then Tab over to the queued
# filter where it sits at the bottom of the rank, bump it up two places with
# Ctrl+K, and Tab on to the played filter, where Outer Wilds shows up in
# both lists because queued and played are independent.

Output "docs/images/queue.gif"

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
Type "breach"
Sleep 1200ms
Enter
Sleep 1500ms
Backspace@60ms 6
Sleep 600ms

Tab
Sleep 1500ms
Down@250ms 4
Sleep 600ms
Ctrl+K
Sleep 700ms
Ctrl+K
Sleep 1500ms

Tab
Sleep 2000ms

Hide
Escape
Sleep 300ms
