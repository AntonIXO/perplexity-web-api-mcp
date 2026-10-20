# Perplexity Web API MCP Server

<p>
    <a href="https://cursor.com/en/install-mcp?name=perplexity-web&config=eyJ0eXBlIjoic3RkaW8iLCJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsIkBhbnRvbmx4L3BlcnBsZXhpdHktd2ViLWFwaS1tY3AiXSwiZW52Ijp7IlBFUlBMRVhJVFlfU0VTU0lPTl9UT0tFTiI6IiJ9fQ==" target="_blank">
        <img src="https://custom-icon-badges.demolab.com/badge/Install_in_Cursor-000000?style=for-the-badge&logo=cursor-ai-white" alt="Install in Cursor">
    </a>
    <a href="https://vscode.dev/redirect/mcp/install?name=perplexity-web&config=%7B%22type%22%3A%22stdio%22%2C%22command%22%3A%22npx%22%2C%22args%22%3A%5B%22-y%22%2C%22%40antonlx%2Fperplexity-web-api-mcp%22%5D%2C%22env%22%3A%7B%22PERPLEXITY_SESSION_TOKEN%22%3A%22%22%7D%7D" target="_blank">
        <img src="https://custom-icon-badges.demolab.com/badge/Install_in_VS_Code-007ACC?style=for-the-badge&logo=vsc&logoColor=white" alt="Install in VS Code">
    </a>
    <a href="https://www.npmjs.com/package/@antonlx/perplexity-web-api-mcp" target="_blank">
        <img
            src="https://img.shields.io/npm/v/%40antonlx%2Fperplexity-web-api-mcp?style=for-the-badge&logo=npm&logoColor=white&color=CB3837"
            alt="NPM Version" />
    </a>
</p>

MCP (Model Context Protocol) server that exposes Perplexity AI search, research, and reasoning capabilities as tools.

This repository is the [`AntonIXO` fork](https://github.com/AntonIXO/perplexity-web-api-mcp) of the [original project by `mishamyrt`](https://github.com/mishamyrt/perplexity-web-api-mcp). Fork releases are published as [`@antonlx/perplexity-web-api-mcp`](https://www.npmjs.com/package/@antonlx/perplexity-web-api-mcp).

## No API Key Required

This MCP server talks to Perplexity's web endpoints — **no API key needed**.

- Without a session token, `perplexity_search` and `perplexity_ask` work anonymously.
- With a session token, the server can use your account's models, history, quotas, connectors, and file uploads.

## Tokenless Mode

The server can run **without authentication**. In this mode:

- Only `perplexity_search` and `perplexity_ask` are available; account-only tools require a token.
- Both tools use the `turbo` model; model overrides are rejected.
- File attachments and connectors are unavailable.
- `PERPLEXITY_INCOGNITO=false` still works: it creates an anonymous, non-incognito thread. It cannot appear in an account's signed-in history because there is no account session.

To use tokenless mode, omit `PERPLEXITY_SESSION_TOKEN` from your configuration.

For full access to all tools and model selection, provide a session token as described in the [Configuration](#configuration) section below.

## Requirements

### Supported Platforms

- macOS (arm64, x86_64)
- Linux (x86_64, aarch64)
- Windows (x86_64)

## Installation

For MCP clients, use the npm package directly (no global install required):

```bash
npx -y @antonlx/perplexity-web-api-mcp
```

To install the latest prebuilt binary on macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/AntonIXO/perplexity-web-api-mcp/releases/latest/download/perplexity-web-api-mcp-installer.sh | sh
```

On Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/AntonIXO/perplexity-web-api-mcp/releases/latest/download/perplexity-web-api-mcp-installer.ps1 | iex"
```

Platform archives and checksums are available on the [GitHub Releases page](https://github.com/AntonIXO/perplexity-web-api-mcp/releases/latest).

## Configuration

### Getting Your Session Cookie

A token is optional for anonymous search. To enable authenticated tools, extract the session cookie from your browser:

1. Log in to [perplexity.ai](https://www.perplexity.ai) in your browser.
2. Open Developer Tools (F12 or right-click → Inspect).
3. Go to Application → Cookies → `https://www.perplexity.ai`.
4. For current accounts, find the cookie whose name starts with `__Secure-pplx.session.`. Copy its full **name** and **value**.
5. Set the cookie value as `PERPLEXITY_SESSION_TOKEN` and the full name as `PERPLEXITY_SESSION_COOKIE_NAME`.

The suffix after `__Secure-pplx.session.` is the account UUID. The server derives the required `x-pplx-account` header from it automatically.

### Environment Variables

- `PERPLEXITY_SESSION_TOKEN` (optional): Value of the Perplexity session cookie. Required for account-only tools, model selection, connectors, and file attachments. Omit it for tokenless mode.
- `PERPLEXITY_SESSION_COOKIE_NAME` (optional): Full cookie name, normally `__Secure-pplx.session.<account-uuid>`. The account UUID is derived from this name and sent as `x-pplx-account`. If unset, the legacy `__Secure-next-auth.session-token` cookie name is used.
- `PERPLEXITY_ACCOUNT_ID` (optional): Explicitly overrides the account UUID derived from `PERPLEXITY_SESSION_COOKIE_NAME`.
- `PERPLEXITY_NO_ACCOUNT_HEADER` (optional): Set to any value to suppress `x-pplx-account` for debugging legacy sessions.
- `PERPLEXITY_DEBUG_COOKIES` (optional): Set to `1` to print cookie names and authentication warm-up status to stderr. Never share this output unredacted.
- `PERPLEXITY_ASK_MODEL` (optional, requires token): Model for `perplexity_ask`.
  Valid values: `turbo`, `pro-auto` (default), `pro-upgraded`, `sonar`, `nemotron-3-super`, `claude-4.6-sonnet`, `claude-4.6-opus`, `gemini-3.0-flash`, `gemini-3.0-pro`, `gpt-5-pro`, `gpt-5.3-codex`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.2`, `gpt-5.2-pro`, `grok-4.1`.
- `PERPLEXITY_REASON_MODEL` (optional, requires token): Model for `perplexity_reason`.
  Valid values: `gemini-3.1-pro` (default), `gemini-3.0-flash-high`, `claude-4.6-sonnet-thinking`, `claude-4.6-opus-thinking`, `gpt-5-thinking`, `gpt-5.1-thinking`, `gpt-5.2-thinking`, `gpt-5.4-thinking`, `grok-4.1-reasoning`, `kimi-k2.5-thinking`.
- `PERPLEXITY_COMPUTER_MODEL` (optional, requires token): Model for `perplexity_computer`.
  Valid values: `asi`, `asi-beta`, `claude-4.6-sonnet` / `claude-4.6-sonnet-thinking`, `claude-4.6-opus` / `claude-4.6-opus-thinking` (default), `gpt-5.4`, `kimi`, `qwen`.
- **`raw:` escape hatch:** any of the three model vars also accepts `raw:<preference>` to pass an arbitrary Perplexity preference string straight through, for models newer than this build's validated list (e.g. `PERPLEXITY_REASON_MODEL=raw:glm_5_2`). No recompile needed. **Important:** each mode only accepts models from its own family — `PERPLEXITY_ASK_MODEL` takes an "ask"-family preference, `PERPLEXITY_REASON_MODEL` takes a "reasoning"-family preference, and the two are not interchangeable. Setting a reasoning-only model (e.g. GLM 5.2 via `raw:glm_5_2`) as `PERPLEXITY_ASK_MODEL` will make `perplexity_ask` silently return `answer: null` instead of erroring — Perplexity's backend accepts the (wrong-family) preference string but produces no output for it. If `perplexity_ask` or `perplexity_reason` starts returning `null` answers after changing a model var, check that the model actually belongs to that tool's family before assuming a rate-limit or auth issue. See [Discovering available models](#discovering-available-models) below for how to find which family a model preference belongs to.
- `PERPLEXITY_TIMEOUT_SECS` (optional, default: `30`): Request timeout in seconds for fast modes (search, ask, reason).
- `PERPLEXITY_LONG_TIMEOUT_SECS` (optional, default: `600`): Request timeout in seconds for long-running modes — **Deep Research**, Computer, and Document Review. Raise this if deep-research runs are being cut off.
  **Note:** this timeout lives inside the Perplexity HTTP client only. If you're also being cut off by your *MCP client's own* tool-call timeout (a separate, client-side setting — see [Progress heartbeats](#progress-heartbeats-for-long-running-tools) below), raising this variable alone won't help; you also need to raise (or auto-extend via progress) the client's timeout.
- `PERPLEXITY_PROGRESS_INTERVAL_SECS` (optional, default: `10`): How often (in seconds) to emit `notifications/progress` heartbeats during a tool call, when the caller's request included a `progressToken`. See [Progress heartbeats](#progress-heartbeats-for-long-running-tools) below.
- `PERPLEXITY_INCOGNITO` (optional, default: `true`): Whether requests use Perplexity's incognito mode.
  Valid values: `true` or `false`. Set it to `false` for normal saved history when authenticated, or for an anonymous non-incognito thread when no token is supplied. Anonymous threads are not attached to an account history.

### Diagnosing a Session

Perplexity recently moved from one shared NextAuth cookie to per-account cookies. If the cookie name is wrong or `x-pplx-account` is missing, the API usually returns HTTP 200 but treats the request as anonymous, which looks like empty answers or zero quotas. The current client sends the account header on warm-up, searches, quota checks, thread reads, and authenticated upload requests.

Check the session without making a search:

```bash
PERPLEXITY_SESSION_TOKEN='...' \
PERPLEXITY_SESSION_COOKIE_NAME='__Secure-pplx.session.<account-uuid>' \
perplexity-web-api-mcp --check
```

`--check` prints `ok remaining_pro=<n>` for an authenticated session, `anonymous remaining_pro=0` for a silent anonymous downgrade, or `error ...` for a transport failure. The legacy cookie path remains available when `PERPLEXITY_SESSION_COOKIE_NAME` is omitted.

### Discovering Available Models

Perplexity doesn't publish a stable model list in this server's code alone — the typed enums (`SearchModel`, `ReasonModel`, `ComputerModel` in `crates/perplexity-web-api/src/models.rs`) are a **snapshot** that goes stale as Perplexity ships new models. Two live sources let you check what's actually available on your account before using the `raw:` escape hatch:

1. **Model config endpoint** (no login required): [`https://www.perplexity.ai/rest/models/config`](https://www.perplexity.ai/rest/models/config) — returns the full JSON list of every model Perplexity's frontend knows about, grouped by which mode(s) it's valid for (`ask`, `reason`/copilot, `computer`/agentic, etc.) along with its internal `preference` string (the exact value you pass to `raw:<preference>`). This is the authoritative source for "does model X exist" and "which family (ask vs reason vs computer) does it belong to" — cross-check against this before filing a bug report about a model returning `null`.
2. **Rate limit / usage endpoint** (requires a logged-in browser session — Cloudflare blocks non-browser requests): [`https://www.perplexity.ai/rest/rate-limit/all`](https://www.perplexity.ai/rest/rate-limit/all) — returns your account's current quota status per feature (`remaining_pro`, `remaining_research`, `remaining_labs`, `remaining_agentic_research`, plus per-source monthly limits). Also exposed as the `perplexity_usage` MCP tool, but that tool call hits the same Cloudflare wall as any other non-browser client and will return an HTTP 403 unless you're proxying through a real browser session (see the code comment on `perplexity_usage` in `crates/perplexity-web-api-mcp/src/server.rs` for the current status of that limitation).

To pull the config JSON from a terminal:

```bash
curl -s 'https://www.perplexity.ai/rest/models/config' | jq .
```

If you find a model preference that isn't in the typed enums yet, either (a) use it immediately via `raw:<preference>` — no recompile needed — or (b) open a PR adding it to `models.rs` so it gets schema validation and shows up in tool descriptions.

### Progress Heartbeats for Long-Running Tools

`perplexity_research`, `perplexity_computer`, and `perplexity_document_review` (and, less commonly, `perplexity_reason` on a heavy query) can legitimately hold Perplexity's SSE connection open for **minutes** with no intermediate bytes — this looks identical to a hung request from an MCP client's point of view.

Per the [MCP Lifecycle spec's Timeouts section](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle#timeouts), a client **MAY** reset its own per-request timeout clock every time it receives a `notifications/progress` message tied to that request's `progressToken` — but the spec doesn't let a *server* force this; it's entirely a client-side opt-in (`resetTimeoutOnProgress` in the TypeScript SDK, or equivalent in other clients). A `SHOULD`-level absolute maximum timeout still applies on top, regardless of progress notifications, so this cannot make a request run forever.

This server implements the server side of that contract:

- If (and only if) the caller's `tools/call` request includes `_meta.progressToken`, a background task sends a `notifications/progress` message every `PERPLEXITY_PROGRESS_INTERVAL_SECS` (default 10s) for the duration of the call.
- `progress` is a monotonically increasing tick counter — Perplexity's streaming search API doesn't expose a real percent-complete signal, and the spec only requires `progress` to increase, not to mean anything specific. `message` explains this in plain English for clients that surface it to a human ("Still working — Perplexity request in flight, no timeout yet.").
- The heartbeat is a `tokio` task tied to the tool call's lifetime: it's spawned right before the underlying HTTP request and aborted on drop, whichever way the call ends (success, error, or client cancellation).
- No `progressToken` in the request → **zero overhead**, no task is spawned.

**This does not replace `PERPLEXITY_LONG_TIMEOUT_SECS`** — that variable still governs how long this server's own HTTP client will wait for Perplexity's API before giving up. The two settings solve different halves of the same problem: `PERPLEXITY_LONG_TIMEOUT_SECS` controls how long *this server* is willing to wait, and the progress heartbeat controls how long *your MCP client* is willing to wait, provided your client supports `resetTimeoutOnProgress` (or equivalent) and actually sends a `progressToken`. Many MCP clients (including some simple stdio wrappers) don't set a `progressToken` at all, in which case only the client's static configured timeout applies and you'll need to raise that directly in your client's config.

### Claude Code

```bash
claude mcp add perplexity \
  --env PERPLEXITY_SESSION_TOKEN="your-session-token" \
  --env PERPLEXITY_SESSION_COOKIE_NAME="__Secure-pplx.session.your-account-uuid" \
  -- npx -y @antonlx/perplexity-web-api-mcp
```

### Cursor, Claude Desktop & Windsurf

I recommend using the one-click install badge at the top of this README for Cursor.

For manual setup, all these clients use the same `mcpServers` format:

| Client | Config File |
|--------|-------------|
| Cursor | `~/.cursor/mcp.json` |
| Claude Desktop | `claude_desktop_config.json` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` |

```json
{
  "mcpServers": {
    "perplexity": {
      "command": "npx",
      "args": ["-y", "@antonlx/perplexity-web-api-mcp"],
      "env": {
        "PERPLEXITY_SESSION_TOKEN": "your-session-token",
        "PERPLEXITY_SESSION_COOKIE_NAME": "__Secure-pplx.session.your-account-uuid"
      }
    }
  }
}
```

### Zed

Add following following to `context_servers` in your [settings file](https://zed.dev/docs/configuring-zed.html#settings-files):

```json
{
  "context_servers": {
    "perplexity": {
      "command": "npx",
      "args": ["-y", "@antonlx/perplexity-web-api-mcp"],
      "env": {
        "PERPLEXITY_SESSION_TOKEN": "your-session-token",
        "PERPLEXITY_SESSION_COOKIE_NAME": "__Secure-pplx.session.your-account-uuid"
      }
    }
  }
}
```

### VS Code

I recommend using the one-click install badge at the top of this README for VS Code, or for manual setup, add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "perplexity": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@antonlx/perplexity-web-api-mcp"],
      "env": {
        "PERPLEXITY_SESSION_TOKEN": "your-session-token",
        "PERPLEXITY_SESSION_COOKIE_NAME": "__Secure-pplx.session.your-account-uuid"
      }
    }
  }
}
```

### Codex

```bash
codex mcp add perplexity \
  --env PERPLEXITY_SESSION_TOKEN="your-session-token" \
  --env PERPLEXITY_SESSION_COOKIE_NAME="__Secure-pplx.session.your-account-uuid" \
  -- npx -y @antonlx/perplexity-web-api-mcp
```

### Building from Source

Source build instructions, including optional cargo features, are documented in [CONTRIBUTING.md](CONTRIBUTING.md).

### Other MCP Clients

Most clients can be manually configured to use the `mcpServers` wrapper in their configuration file (like Cursor). If your client doesn't work, check its documentation for the correct wrapper format.

## Docker

Build the fork's image locally:

```bash
docker build -t perplexity-web-api-mcp .

docker run -d \
  -p 8080:8080 \
  -e PERPLEXITY_SESSION_TOKEN="your-session-token" \
  -e PERPLEXITY_SESSION_COOKIE_NAME="__Secure-pplx.session.your-account-uuid" \
  perplexity-web-api-mcp
```

The container exposes the MCP server via Streamable HTTP at `http://localhost:8080/mcp`.
The Docker image is built with `--features streamable-http`; local/source builds need the same feature if you want HTTP transport.

Configure your MCP client to connect:

```json
{
  "mcpServers": {
    "perplexity": {
      "url": "http://localhost:8080/mcp"
    }
  }
}
```

### Environment Variables (Docker-specific)

| Variable | Default | Description |
|----------|---------|-------------|
| `MCP_TRANSPORT` | `streamable-http` | Transport mode. `stdio` or `streamable-http` (requires the `streamable-http` cargo feature) |
| `MCP_HOST` | `0.0.0.0` | Host address to bind |
| `MCP_PORT` | `8080` | Port to listen on |

The [authentication token, model variables, and incognito flag](#configuration) described above work the same way in Docker.

## Available Tools

### `perplexity_search`

Quick web search using the `turbo` model. Returns only links, titles, and snippets — no generated answer.

**Best for:** Finding relevant URLs and sources quickly.

**Parameters:**

- `query` (required): The search query or question
- `sources` (optional): Array of sources — `"web"`, `"scholar"`, `"social"`, plus connectors like
  `"google_drive"`, `"gcal"` / `"google_calendar"`, `"github_mcp_direct"` / `"github"`,
  `"hugging_face"` / `"huggingface"`, `"notion_mcp"` / `"notion"`, `"linear_alt"` / `"linear"`,
  `"slack_direct"` / `"slack"`, `"jira_mcp_merge"` / `"jira"`, `"confluence_mcp_merge"` / `"confluence"`,
  `"microsoft_teams_mcp_merge"` / `"teams"`, `"onedrive"`, `"sharepoint"`, `"dropbox"`, `"box"`.
  Unknown connector IDs are passed through unchanged. Defaults to `["web"]`
- `language` (optional): Language code, e.g., `"en-US"`. Defaults to `"en-US"`

> File attachments are not supported by this tool.

### `perplexity_ask`

Ask Perplexity AI a question and get a comprehensive answer with source citations. By default uses the best model (Pro auto mode) when authenticated, or `turbo` in tokenless mode. Can be configured via `PERPLEXITY_ASK_MODEL`.

**Best for:** Getting detailed answers to questions with web context.

**Parameters:** Same as `perplexity_search`, plus:

- `files` (optional, requires token): Array of file attachments for document analysis. See [File Attachments](#file-attachments).

### `perplexity_reason`

Advanced reasoning and problem-solving. By default uses Perplexity's `sonar-reasoning` model, but can be configured via `PERPLEXITY_REASON_MODEL`.

**Best for:** Logical problems, complex analysis, decision-making, and tasks requiring step-by-step reasoning.

**Parameters:** Same as `perplexity_ask`.

### `perplexity_research`

Deep, comprehensive research using Perplexity's sonar-deep-research (`pplx_alpha`) model.

**Best for:** Complex topics requiring detailed investigation, comprehensive reports, and in-depth analysis. Provides thorough analysis with citations.

**Parameters:** Same as `perplexity_ask`.

### `perplexity_view_chat`

Loads the context of an existing Perplexity search/chat and asks Perplexity to extract or summarize it.

**Parameters:**

- `url_or_id` (required): Full `perplexity.ai/search/...` URL, thread UUID, or thread slug
- `query` (optional): A specific question about the thread. By default, returns a detailed question-by-question breakdown

Requires `PERPLEXITY_SESSION_TOKEN` and, for current accounts, `PERPLEXITY_SESSION_COOKIE_NAME`. This reads the existing thread directly; it does not add a follow-up turn.

## File Attachments

`perplexity_ask`, `perplexity_research`, and `perplexity_reason` accept an optional `files` parameter for document analysis. **Requires authentication token.**

Each entry in the `files` array must have:

- `filename` (required): Filename with extension, e.g. `"report.pdf"` or `"notes.txt"`
- `text` (mutually exclusive with `data`): Plain-text file content. Use for `.txt`, `.md`, `.csv`, `.json`, source code, etc.
- `data` (mutually exclusive with `text`): Base64-encoded binary content. Use for `.pdf`, `.docx`, images, etc.

**Example — plain text:**

```json
{
  "query": "Summarise the key points",
  "files": [
    {
      "filename": "notes.txt",
      "text": "Meeting notes: Q1 revenue up 12%..."
    }
  ]
}
```

**Example — binary file (PDF):**

```json
{
  "query": "What does this contract say about termination?",
  "files": [
    {
      "filename": "contract.pdf",
      "data": "JVBERi0xLjQK..."
    }
  ]
}
```

Multiple files can be passed in a single request — they are uploaded to Perplexity's storage in parallel before the query is sent.

## Response Format

`perplexity_search` returns only web results:

```json
{
  "web_results": [
    {
      "name": "Source name",
      "url": "https://example.com",
      "snippet": "Source snippet"
    }
  ]
}
```

`perplexity_ask`, `perplexity_research`, and `perplexity_reason` return a full response:

```json
{
  "answer": "The generated answer text...",
  "web_results": [
    {
      "name": "Source name",
      "url": "https://example.com",
      "snippet": "Source snippet"
    }
  ],
  "follow_up": {
    "backend_uuid": "uuid-for-follow-up-queries",
    "attachments": []
  }
}
```

## License

MIT
