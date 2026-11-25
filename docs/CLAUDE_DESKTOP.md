# Using mockingbird-studio from Claude Desktop

`mockingbird-studio mcp` is an MCP server over stdio. Claude Desktop starts
it as a child process when it opens and stops it when it quits. The window
doesn't need to be open.

## Register it

1. Build the binary: `cargo build --release`. It's at
   `target/release/mockingbird-studio`.
2. Open Claude Desktop's config file. In Claude Desktop, go to Settings →
   Developer → Edit Config, or open
   `~/Library/Application Support/Claude/claude_desktop_config.json`.
3. Add the server under `mcpServers`, using the binary's full path:

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
   `"env": { "MOCKINGBIRD_STUDIO_HOME": "/some/folder" }`. The CLI and the
   window must see the same folder.
4. Quit Claude Desktop completely (⌘Q) and open it again.

## Check it works

In a new chat, open the tools menu: `mockingbird-studio` lists ten tools,
from `project_new` to `redo`. Ask "what's the status of my projects?" and
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
