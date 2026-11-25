# Using mockingbird-studio from Claude Desktop

`mockingbird-studio mcp` is an MCP server over stdio. Claude Desktop starts
it as a child process when it opens and stops it when it quits. The window
doesn't need to be open.

## Build the app

Install Tauri's CLI once: `cargo install tauri-cli --version "^2" --locked`.
(If it says it needs a newer rustc, pin an older release, such as
`--version "=2.11.5"` for Rust 1.89.)
Then, from `app/`, run `cargo tauri build`. The bundle is
`<repo>/target/release/bundle/macos/mockingbird-studio.app`, at the repo
root, not in `app/`. Copy it to
`/Applications`. It isn't signed or notarized, so the first time, right-click
it and choose Open.

The binary inside the bundle is
`mockingbird-studio.app/Contents/MacOS/mockingbird-studio`. It's the same
program: with no arguments it opens the window, and `mcp`, `setup` and the
other commands run without one.

## Register it

Run `setup` with the binary you want Claude Desktop to start:

```sh
/Applications/mockingbird-studio.app/Contents/MacOS/mockingbird-studio setup
```

It shows the entry and the file it will edit, and asks `Write it? [y/N]`.
Anything but `y` changes nothing. `--yes` skips the question for scripts, and
`--config <file>` edits a different file. Run from `target/` or from a
translocated (quarantined, not yet moved) app, the path won't last, so the
prompt warns and `--yes` refuses. It keeps every other entry in the
file, replaces an older `mockingbird-studio` entry (showing both, and keeping the
old one's `env` if the new one has none), and refuses to touch a
file that isn't valid JSON. If `MOCKINGBIRD_STUDIO_HOME` is set when you run
it, the entry carries it, so Claude Desktop uses the same projects folder as
the CLI and the window.

Then quit Claude Desktop completely (⌘Q) and open it again.

To do it by hand instead, open Settings → Developer → Edit Config (the file is
`~/Library/Application Support/Claude/claude_desktop_config.json`) and add:

```json
{
  "mcpServers": {
    "mockingbird-studio": {
      "command": "/full/path/to/mockingbird-studio",
      "args": ["mcp"]
    }
  }
}
```

To keep projects somewhere other than `~/.mockingbird-studio`, add
`"env": { "MOCKINGBIRD_STUDIO_HOME": "/some/folder" }`. For a plain
`cargo build --release`, the binary is `target/release/mockingbird-studio`.

## Check it works

In a new chat, open the tools menu: `mockingbird-studio` lists one tool per
operation, such as `status`, `submit` and `approve`. Ask "what's the status of my projects?" and
Claude calls `projects`.

Every reply ends with a line like
`Open notes: 1. Steps waiting on you: my-video/script (redo).`, so the
agent sees your notes without being asked to look.

## When it doesn't show up

- Claude Desktop writes the server's stderr and its own connection log to
  `~/Library/Logs/Claude/mcp-server-mockingbird-studio.log` and
  `~/Library/Logs/Claude/mcp.log`.
- Run `/full/path/to/mockingbird-studio mcp` in a terminal. It should wait
  for input without printing anything. Press Ctrl-D to stop it.
- The path in `command` must be absolute. Claude Desktop doesn't use your
  shell's `PATH`.
