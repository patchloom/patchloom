# Patchloom demo: 6 edits across 4 files in 3 formats, one command
# Run demo/setup.sh first, then: vhs demo/demo.tape
Output demo/demo.gif
Set Shell bash
Set FontSize 16
Set Width 900
Set Height 560
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms
Set CursorBlink false

# Enter pre-created demo workspace
Hide
Type "export PATH=/home/sebtardif/patchloom/target/release:$PATH"
Enter
Type "cd $(cat /tmp/patchloom-demo-dir) && clear"
Enter
Sleep 500ms
Show

# Show the YAML with comments — the thing to watch for
Type "cat config.yaml"
Enter
Sleep 3s

# Show the batch operations (6 edits, 4 files, 3 formats)
Type "cat ops.txt"
Enter
Sleep 3.5s

# Preview the diff across all files
Type "patchloom batch --diff < ops.txt"
Enter
Sleep 6s

# Apply all edits atomically
Type "patchloom batch --apply < ops.txt"
Enter
Sleep 2s

# The payoff: YAML comments and formatting survived
Type "cat config.yaml"
Enter
Sleep 4s
