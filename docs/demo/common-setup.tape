# Shared hidden setup for the docs/demo tapes. Sourced, not copied, so the
# setup has one home instead of several that drift.
#
# Ends still inside Hide with no `clear` and no `Show`: each tape supplies
# its own. Sourcing inlines these commands, so the Hide opened here stays in
# effect across the boundary.
#
# vhs gotchas: don't rename this file to start with an underscore (the
# lexer rejects `Source _setup.tape`), and the Source path resolves against
# the process working directory, so every tape must run from the repo root.

Hide
Type "exec zsh -f"
Enter
Sleep 500ms
Type 'BOUNCER_REPO=$PWD; export HOME=$($BOUNCER_REPO/docs/demo/seed-demo.sh) || exit 1'
Enter
Sleep 1s
Type '[ -x "$BOUNCER_REPO/target/release/bouncer" ] || exit 1'
Enter
Sleep 200ms
Type 'export PATH=$BOUNCER_REPO/target/release:$PATH'
Enter
Sleep 200ms
Type 'source $HOME/.demo-prompt'
Enter
Sleep 200ms
