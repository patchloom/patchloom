# Patchloom demo: batch edit 4 files in one call
# Run demo/setup.sh first, then record this tape.
Output demo/demo.gif
Set Shell bash
Set FontSize 15
Set Width 900
Set Height 480
Set Padding 15
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

# Show current state
Type "cat config.json"
Enter
Sleep 2s

Type "cat config.yaml"
Enter
Sleep 2s

# Run the batch preview
Type "# Bump version in 4 files with one command:"
Enter
Sleep 1.5s

Type "patchloom batch --diff < ops.txt"
Enter
Sleep 5s

# Apply
Type "patchloom batch --apply < ops.txt"
Enter
Sleep 2s

# Verify YAML comments preserved
Type "cat config.yaml"
Enter
Sleep 3s
