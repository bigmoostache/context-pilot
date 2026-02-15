# Claude Code API Documentation

Complete guide to using Claude Code's OAuth credentials to access the Anthropic API programmatically.

---

## Table of Contents

1. [Overview](#overview)
2. [Credential Files](#credential-files)
3. [Authentication](#authentication)
4. [Basic API Calls](#basic-api-calls)
5. [Streaming](#streaming)
6. [Tool Use (Function Calling)](#tool-use-function-calling)
7. [Token Management](#token-management)
8. [OAuth Configuration](#oauth-configuration)
9. [Error Handling](#error-handling)
10. [Complete Code Examples](#complete-code-examples)

---

## Overview

Claude Code uses OAuth 2.0 tokens to authenticate with the Anthropic API. These tokens are different from standard API keys and require specific headers to work.

### Key Differences: OAuth Tokens vs API Keys

| Feature | OAuth Tokens (Claude Code) | API Keys |
|---------|---------------------------|----------|
| Prefix | `sk-ant-oat01-...` | `sk-ant-api03-...` |
| Header | `Authorization: Bearer <token>` | `x-api-key: <key>` |
| Beta Header | **Required**: `anthropic-beta: oauth-2025-04-20` | Not required |
| Expiration | ~7 days | Never (until revoked) |
| Refresh | Yes, via refresh token | No |
| Source | `claude login` | console.anthropic.com |

---

## Credential Files

Claude Code stores credentials in the `~/.claude/` directory.

### Active Credentials (Use This One!)

```
~/.claude/.credentials.json
```

> **Important**: Note the dot prefix (`.credentials.json`) - this is a hidden file!

### File Structure

```json
{
  "claudeAiOauth": {
    "accessToken": "sk-ant-oat01-...",
    "refreshToken": "sk-ant-ort01-...",
    "expiresAt": 1770135538659,
    "scopes": [
      "user:inference",
      "user:mcp_servers",
      "user:profile",
      "user:sessions:claude_code"
    ],
    "subscriptionType": "max",
    "rateLimitTier": "default_claude_max_20x"
  }
}
```

### Field Descriptions

| Field | Description |
|-------|-------------|
| `accessToken` | The OAuth access token for API calls |
| `refreshToken` | Token used to obtain new access tokens |
| `expiresAt` | Unix timestamp in milliseconds when token expires |
| `scopes` | Permissions granted to this token |
| `subscriptionType` | Your Claude subscription tier |
| `rateLimitTier` | Rate limiting tier for API calls |

### Loading Credentials in Python

```python
import json
from pathlib import Path

def load_credentials():
    """Load OAuth credentials from Claude Code's config"""
    creds_path = Path.home() / ".claude" / ".credentials.json"

    # Fallback to non-hidden file if hidden doesn't exist
    if not creds_path.exists():
        creds_path = Path.home() / ".claude" / "credentials.json"

    with open(creds_path) as f:
        creds = json.load(f)

    return creds["claudeAiOauth"]

# Usage
oauth = load_credentials()
access_token = oauth["accessToken"]
```

---

## Authentication

### Required Headers

Every API request with OAuth tokens **must** include these headers:

```
Authorization: Bearer <access_token>
Content-Type: application/json
anthropic-version: 2023-06-01
anthropic-beta: oauth-2025-04-20
```

> **Critical**: The `anthropic-beta: oauth-2025-04-20` header is **mandatory**. Without it, you'll get: `"OAuth authentication is currently not supported."`

### cURL Authentication

```bash
curl https://api.anthropic.com/v1/messages \
  -H "Authorization: Bearer sk-ant-oat01-YOUR_TOKEN_HERE" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -d '{"model": "claude-3-haiku-20240307", "max_tokens": 100, "messages": [...]}'
```

### Python SDK Authentication

```python
import anthropic

client = anthropic.Anthropic(
    auth_token="sk-ant-oat01-YOUR_TOKEN_HERE",
    default_headers={"anthropic-beta": "oauth-2025-04-20"}
)
```

### Python Requests Authentication

```python
import requests

headers = {
    "Authorization": f"Bearer {access_token}",
    "Content-Type": "application/json",
    "anthropic-version": "2023-06-01",
    "anthropic-beta": "oauth-2025-04-20",
}

response = requests.post(
    "https://api.anthropic.com/v1/messages",
    headers=headers,
    json={...}
)
```

---

## Basic API Calls

### Endpoint

```
POST https://api.anthropic.com/v1/messages
```

### Request Body Structure

```json
{
  "model": "claude-3-haiku-20240307",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "Hello, Claude!"}
  ]
}
```

### Available Models

| Model | Description |
|-------|-------------|
| `claude-3-opus-20240229` | Most capable, best for complex tasks |
| `claude-3-sonnet-20240229` | Balanced performance and speed |
| `claude-3-haiku-20240307` | Fastest, most cost-effective |
| `claude-3-5-sonnet-20241022` | Latest Sonnet with improved capabilities |

### Complete cURL Example

```bash
curl -s https://api.anthropic.com/v1/messages \
  -H "Authorization: Bearer sk-ant-oat01-..." \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -d '{
    "model": "claude-3-haiku-20240307",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Explain quantum computing in one sentence."}
    ]
  }'
```

### Complete Python SDK Example

```python
import anthropic
import json
from pathlib import Path

# Load credentials
with open(Path.home() / ".claude" / ".credentials.json") as f:
    token = json.load(f)["claudeAiOauth"]["accessToken"]

# Create client
client = anthropic.Anthropic(
    auth_token=token,
    default_headers={"anthropic-beta": "oauth-2025-04-20"}
)

# Make request
message = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    messages=[
        {"role": "user", "content": "Explain quantum computing in one sentence."}
    ]
)

print(message.content[0].text)
```

### Response Structure

```json
{
  "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
  "type": "message",
  "role": "assistant",
  "model": "claude-3-haiku-20240307",
  "content": [
    {
      "type": "text",
      "text": "Quantum computing harnesses quantum mechanical phenomena..."
    }
  ],
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {
    "input_tokens": 15,
    "output_tokens": 42
  }
}
```

### Multi-Turn Conversations

```python
messages = [
    {"role": "user", "content": "What is Python?"},
    {"role": "assistant", "content": "Python is a high-level programming language..."},
    {"role": "user", "content": "What are its main uses?"}
]

response = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    messages=messages
)
```

### System Prompts

```python
response = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    system="You are a helpful coding assistant. Always provide code examples.",
    messages=[
        {"role": "user", "content": "How do I read a file in Python?"}
    ]
)
```

---

## Streaming

Streaming allows you to receive responses token-by-token as they're generated.

### Enable Streaming

Add `"stream": true` to your request body.

### cURL Streaming Example

```bash
curl -s https://api.anthropic.com/v1/messages \
  -H "Authorization: Bearer sk-ant-oat01-..." \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -d '{
    "model": "claude-3-haiku-20240307",
    "max_tokens": 200,
    "stream": true,
    "messages": [{"role": "user", "content": "Count from 1 to 10."}]
  }'
```

### Server-Sent Events (SSE) Format

Streaming responses use SSE format:

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_...","model":"claude-3-haiku-20240307",...}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"1"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"..."}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":50}}

event: message_stop
data: {"type":"message_stop"}
```

### Python SDK Streaming - Text Only

```python
with client.messages.stream(
    model="claude-3-haiku-20240307",
    max_tokens=200,
    messages=[{"role": "user", "content": "Count from 1 to 10."}]
) as stream:
    for text in stream.text_stream:
        print(text, end="", flush=True)
```

### Python SDK Streaming - All Events

```python
with client.messages.stream(
    model="claude-3-haiku-20240307",
    max_tokens=200,
    messages=[{"role": "user", "content": "Hello!"}]
) as stream:
    for event in stream:
        print(f"Event type: {event.type}")

        if hasattr(event, 'delta') and hasattr(event.delta, 'text'):
            print(f"Text: {event.delta.text}")
```

### Python Requests Streaming

```python
import requests
import json

response = requests.post(
    "https://api.anthropic.com/v1/messages",
    headers={
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "anthropic-version": "2023-06-01",
        "anthropic-beta": "oauth-2025-04-20",
    },
    json={
        "model": "claude-3-haiku-20240307",
        "max_tokens": 200,
        "stream": True,
        "messages": [{"role": "user", "content": "Hello!"}]
    },
    stream=True
)

for line in response.iter_lines():
    if line:
        decoded = line.decode('utf-8')
        if decoded.startswith('data: '):
            data = decoded[6:]
            try:
                event = json.loads(data)
                if event.get('type') == 'content_block_delta':
                    text = event.get('delta', {}).get('text', '')
                    print(text, end='', flush=True)
            except json.JSONDecodeError:
                pass
```

### Streaming Event Types

| Event Type | Description |
|------------|-------------|
| `message_start` | Initial message metadata |
| `content_block_start` | New content block beginning |
| `content_block_delta` | Incremental content update |
| `content_block_stop` | Content block complete |
| `message_delta` | Message-level updates (stop_reason, usage) |
| `message_stop` | Stream complete |
| `ping` | Keep-alive ping |

---

## Tool Use (Function Calling)

Tool use allows Claude to call functions you define, enabling integration with external systems.

### Tool Definition Schema

```python
tools = [
    {
        "name": "get_weather",
        "description": "Get the current weather in a given location",
        "input_schema": {
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city and state, e.g. San Francisco, CA"
                },
                "unit": {
                    "type": "string",
                    "enum": ["celsius", "fahrenheit"],
                    "description": "Temperature unit"
                }
            },
            "required": ["location"]
        }
    }
]
```

### Making a Tool Use Request

```python
response = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    tools=tools,
    messages=[
        {"role": "user", "content": "What's the weather in Tokyo?"}
    ]
)
```

### Tool Use Response

When Claude wants to use a tool, the response has `stop_reason: "tool_use"`:

```json
{
  "id": "msg_...",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": "I'll check the weather in Tokyo for you."
    },
    {
      "type": "tool_use",
      "id": "toolu_01A09q90qw90lq917835lgs0",
      "name": "get_weather",
      "input": {
        "location": "Tokyo, Japan",
        "unit": "celsius"
      }
    }
  ],
  "stop_reason": "tool_use",
  "usage": {"input_tokens": 380, "output_tokens": 82}
}
```

### Processing Tool Use Responses

```python
# Check if Claude wants to use tools
if response.stop_reason == "tool_use":
    for block in response.content:
        if block.type == "tool_use":
            tool_name = block.name
            tool_input = block.input
            tool_use_id = block.id

            # Execute your tool
            result = execute_tool(tool_name, tool_input)
```

### Sending Tool Results

After executing a tool, send the results back:

```python
messages = [
    # Original user message
    {"role": "user", "content": "What's the weather in Tokyo?"},

    # Claude's response (including tool_use)
    {"role": "assistant", "content": response.content},

    # Tool results
    {
        "role": "user",
        "content": [
            {
                "type": "tool_result",
                "tool_use_id": "toolu_01A09q90qw90lq917835lgs0",
                "content": '{"temperature": 22, "unit": "celsius", "conditions": "Partly cloudy"}'
            }
        ]
    }
]

# Continue the conversation
final_response = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    tools=tools,
    messages=messages
)
```

### Complete Tool Use Example

```python
import anthropic
import json
from pathlib import Path

# Load credentials
with open(Path.home() / ".claude" / ".credentials.json") as f:
    token = json.load(f)["claudeAiOauth"]["accessToken"]

client = anthropic.Anthropic(
    auth_token=token,
    default_headers={"anthropic-beta": "oauth-2025-04-20"}
)

# Define tools
tools = [
    {
        "name": "calculate",
        "description": "Perform mathematical calculations",
        "input_schema": {
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "Math expression to evaluate"
                }
            },
            "required": ["expression"]
        }
    }
]

def execute_tool(name, inputs):
    """Execute a tool and return results"""
    if name == "calculate":
        try:
            result = eval(inputs["expression"])  # Use safe parser in production!
            return json.dumps({"result": result})
        except Exception as e:
            return json.dumps({"error": str(e)})
    return json.dumps({"error": "Unknown tool"})

# Start conversation
messages = [{"role": "user", "content": "What is 15 * 27 + 83?"}]

response = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    tools=tools,
    messages=messages
)

# Handle tool use loop
while response.stop_reason == "tool_use":
    # Add assistant response to messages
    messages.append({"role": "assistant", "content": response.content})

    # Process tool calls
    tool_results = []
    for block in response.content:
        if block.type == "tool_use":
            result = execute_tool(block.name, block.input)
            tool_results.append({
                "type": "tool_result",
                "tool_use_id": block.id,
                "content": result
            })

    # Add tool results
    messages.append({"role": "user", "content": tool_results})

    # Continue conversation
    response = client.messages.create(
        model="claude-3-haiku-20240307",
        max_tokens=1024,
        tools=tools,
        messages=messages
    )

# Print final response
for block in response.content:
    if block.type == "text":
        print(block.text)
```

### Multiple Tools

```python
tools = [
    {
        "name": "get_weather",
        "description": "Get weather for a location",
        "input_schema": {
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }
    },
    {
        "name": "search_web",
        "description": "Search the web",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }
    },
    {
        "name": "send_email",
        "description": "Send an email",
        "input_schema": {
            "type": "object",
            "properties": {
                "to": {"type": "string"},
                "subject": {"type": "string"},
                "body": {"type": "string"}
            },
            "required": ["to", "subject", "body"]
        }
    }
]
```

### Streaming with Tools

```python
with client.messages.stream(
    model="claude-3-haiku-20240307",
    max_tokens=1024,
    tools=tools,
    messages=[{"role": "user", "content": "Calculate 123 * 456"}]
) as stream:
    for event in stream:
        if event.type == 'content_block_start':
            if event.content_block.type == 'tool_use':
                print(f"Tool: {event.content_block.name}")
        elif event.type == 'content_block_delta':
            if hasattr(event.delta, 'text'):
                print(event.delta.text, end='')
            elif hasattr(event.delta, 'partial_json'):
                print(event.delta.partial_json, end='')
```

### Tool Use with cURL

```bash
curl -s https://api.anthropic.com/v1/messages \
  -H "Authorization: Bearer sk-ant-oat01-..." \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -d '{
    "model": "claude-3-haiku-20240307",
    "max_tokens": 1024,
    "tools": [
      {
        "name": "get_weather",
        "description": "Get weather for a location",
        "input_schema": {
          "type": "object",
          "properties": {
            "location": {"type": "string", "description": "City name"}
          },
          "required": ["location"]
        }
      }
    ],
    "messages": [
      {"role": "user", "content": "What is the weather in Paris?"}
    ]
  }'
```

---

## Token Management

### Checking Token Expiry

```python
from datetime import datetime
import json
from pathlib import Path

def check_token_status():
    with open(Path.home() / ".claude" / ".credentials.json") as f:
        oauth = json.load(f)["claudeAiOauth"]

    expires_at = oauth["expiresAt"]
    expires_date = datetime.fromtimestamp(expires_at / 1000)
    now = datetime.now()

    is_expired = now > expires_date
    time_remaining = expires_date - now if not is_expired else None

    return {
        "expired": is_expired,
        "expires_at": expires_date,
        "time_remaining": time_remaining
    }

status = check_token_status()
print(f"Token expired: {status['expired']}")
print(f"Expires at: {status['expires_at']}")
if status['time_remaining']:
    print(f"Time remaining: {status['time_remaining']}")
```

### Refreshing Tokens

```python
import requests

def refresh_token(refresh_token):
    """Refresh OAuth access token"""
    response = requests.post(
        "https://platform.claude.com/v1/oauth/token",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        data={
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
        }
    )

    if response.status_code == 200:
        return response.json()
    else:
        return None

# Usage
with open(Path.home() / ".claude" / ".credentials.json") as f:
    oauth = json.load(f)["claudeAiOauth"]

new_tokens = refresh_token(oauth["refreshToken"])
if new_tokens:
    print(f"New access token: {new_tokens['access_token'][:40]}...")
```

### Token Refresh Response

```json
{
  "access_token": "sk-ant-oat01-NEW_TOKEN...",
  "refresh_token": "sk-ant-ort01-NEW_REFRESH_TOKEN...",
  "expires_in": 604800,
  "token_type": "Bearer"
}
```

### Auto-Refreshing Client Wrapper

```python
import anthropic
import json
import requests
from datetime import datetime
from pathlib import Path

class AutoRefreshingClient:
    """Anthropic client that automatically refreshes OAuth tokens"""

    CREDS_PATH = Path.home() / ".claude" / ".credentials.json"
    CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
    TOKEN_URL = "https://platform.claude.com/v1/oauth/token"

    def __init__(self):
        self._load_credentials()
        self._create_client()

    def _load_credentials(self):
        with open(self.CREDS_PATH) as f:
            self.oauth = json.load(f)["claudeAiOauth"]

    def _is_expired(self):
        expires_at = self.oauth["expiresAt"]
        # Refresh 5 minutes before expiry
        return datetime.now().timestamp() * 1000 > expires_at - 300000

    def _refresh_token(self):
        response = requests.post(
            self.TOKEN_URL,
            headers={"Content-Type": "application/x-www-form-urlencoded"},
            data={
                "grant_type": "refresh_token",
                "refresh_token": self.oauth["refreshToken"],
                "client_id": self.CLIENT_ID
            }
        )

        if response.status_code == 200:
            data = response.json()
            self.oauth["accessToken"] = data["access_token"]
            if "refresh_token" in data:
                self.oauth["refreshToken"] = data["refresh_token"]
            self.oauth["expiresAt"] = int(
                datetime.now().timestamp() * 1000 + data["expires_in"] * 1000
            )
            self._save_credentials()
            self._create_client()
            return True
        return False

    def _save_credentials(self):
        with open(self.CREDS_PATH, 'w') as f:
            json.dump({"claudeAiOauth": self.oauth}, f)

    def _create_client(self):
        self.client = anthropic.Anthropic(
            auth_token=self.oauth["accessToken"],
            default_headers={"anthropic-beta": "oauth-2025-04-20"}
        )

    def messages_create(self, **kwargs):
        if self._is_expired():
            self._refresh_token()
        return self.client.messages.create(**kwargs)

    def messages_stream(self, **kwargs):
        if self._is_expired():
            self._refresh_token()
        return self.client.messages.stream(**kwargs)

# Usage
client = AutoRefreshingClient()
response = client.messages_create(
    model="claude-3-haiku-20240307",
    max_tokens=100,
    messages=[{"role": "user", "content": "Hello!"}]
)
```

---

## OAuth Configuration

These values were extracted from the Claude Code binary:

### Endpoints

| Endpoint | URL |
|----------|-----|
| API Base | `https://api.anthropic.com` |
| Messages | `https://api.anthropic.com/v1/messages` |
| Token Refresh | `https://platform.claude.com/v1/oauth/token` |
| Console Auth | `https://platform.claude.com/oauth/authorize` |
| Claude AI Auth | `https://claude.ai/oauth/authorize` |

### OAuth Parameters

| Parameter | Value |
|-----------|-------|
| Client ID | `9d1c250a-e61b-44d9-88ed-5944d1962f5e` |
| OAuth Version | `oauth-2025-04-20` |
| Grant Type (refresh) | `refresh_token` |

### Available Scopes

| Scope | Description |
|-------|-------------|
| `user:profile` | Access user profile information |
| `user:inference` | Access Claude inference API |
| `user:sessions:claude_code` | Claude Code sessions |
| `user:mcp_servers` | Model Context Protocol servers |
| `org:create_api_key` | Create API keys (org-level) |

---

## Error Handling

### Common Errors

#### Missing Beta Header
```json
{
  "type": "error",
  "error": {
    "type": "authentication_error",
    "message": "OAuth authentication is currently not supported."
  }
}
```
**Solution**: Add `anthropic-beta: oauth-2025-04-20` header.

#### Expired Token
```json
{
  "type": "error",
  "error": {
    "type": "authentication_error",
    "message": "Invalid bearer token"
  }
}
```
**Solution**: Refresh the token or run `claude login`.

#### Invalid Token
```json
{
  "type": "error",
  "error": {
    "type": "authentication_error",
    "message": "Invalid bearer token"
  }
}
```
**Solution**: Check that you're using the token from `.credentials.json` (hidden file).

#### Rate Limiting
```json
{
  "type": "error",
  "error": {
    "type": "rate_limit_error",
    "message": "Rate limit exceeded"
  }
}
```
**Solution**: Implement exponential backoff and retry.

### Error Handling Example

```python
import anthropic
from anthropic import APIError, AuthenticationError, RateLimitError

try:
    response = client.messages.create(
        model="claude-3-haiku-20240307",
        max_tokens=100,
        messages=[{"role": "user", "content": "Hello"}]
    )
except AuthenticationError as e:
    print(f"Authentication failed: {e}")
    print("Try refreshing token or running 'claude login'")
except RateLimitError as e:
    print(f"Rate limited: {e}")
    print("Waiting before retry...")
except APIError as e:
    print(f"API error: {e}")
```

---

## Complete Code Examples

### Full Working Example

```python
#!/usr/bin/env python3
"""
Complete example of using Claude Code OAuth credentials
"""

import anthropic
import json
from pathlib import Path
from datetime import datetime


def load_credentials():
    """Load and validate OAuth credentials"""
    creds_path = Path.home() / ".claude" / ".credentials.json"
    if not creds_path.exists():
        creds_path = Path.home() / ".claude" / "credentials.json"

    with open(creds_path) as f:
        oauth = json.load(f)["claudeAiOauth"]

    # Check expiry
    expires_at = datetime.fromtimestamp(oauth["expiresAt"] / 1000)
    if datetime.now() > expires_at:
        raise ValueError(f"Token expired at {expires_at}. Run 'claude login'")

    return oauth["accessToken"]


def create_client():
    """Create authenticated Anthropic client"""
    token = load_credentials()
    return anthropic.Anthropic(
        auth_token=token,
        default_headers={"anthropic-beta": "oauth-2025-04-20"}
    )


def main():
    client = create_client()

    # Basic message
    print("=== Basic Message ===")
    response = client.messages.create(
        model="claude-3-haiku-20240307",
        max_tokens=100,
        messages=[{"role": "user", "content": "What is 2+2?"}]
    )
    print(response.content[0].text)

    # Streaming
    print("\n=== Streaming ===")
    with client.messages.stream(
        model="claude-3-haiku-20240307",
        max_tokens=100,
        messages=[{"role": "user", "content": "Count to 5."}]
    ) as stream:
        for text in stream.text_stream:
            print(text, end="", flush=True)
    print()

    # Tool use
    print("\n=== Tool Use ===")
    tools = [{
        "name": "add",
        "description": "Add two numbers",
        "input_schema": {
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["a", "b"]
        }
    }]

    response = client.messages.create(
        model="claude-3-haiku-20240307",
        max_tokens=1024,
        tools=tools,
        messages=[{"role": "user", "content": "Add 5 and 7"}]
    )

    if response.stop_reason == "tool_use":
        for block in response.content:
            if block.type == "tool_use":
                print(f"Tool called: {block.name}")
                print(f"Arguments: {block.input}")
                result = block.input["a"] + block.input["b"]
                print(f"Result: {result}")


if __name__ == "__main__":
    main()
```

### cURL Script

```bash
#!/bin/bash
# claude_api.sh - Use Claude Code OAuth credentials with curl

# Load token from credentials file
TOKEN=$(cat ~/.claude/.credentials.json | python3 -c "import sys,json; print(json.load(sys.stdin)['claudeAiOauth']['accessToken'])")

# Make API call
curl -s https://api.anthropic.com/v1/messages \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -d '{
    "model": "claude-3-haiku-20240307",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "'"$1"'"}
    ]
  }' | python3 -c "import sys,json; print(json.load(sys.stdin)['content'][0]['text'])"
```

Usage:
```bash
chmod +x claude_api.sh
./claude_api.sh "What is the capital of France?"
```

---

## Quick Reference

### Minimum Required Headers

```
Authorization: Bearer sk-ant-oat01-...
Content-Type: application/json
anthropic-version: 2023-06-01
anthropic-beta: oauth-2025-04-20
```

### Python SDK Quick Start

```python
import anthropic, json
from pathlib import Path

token = json.load(open(Path.home()/".claude"/".credentials.json"))["claudeAiOauth"]["accessToken"]

client = anthropic.Anthropic(
    auth_token=token,
    default_headers={"anthropic-beta": "oauth-2025-04-20"}
)

response = client.messages.create(
    model="claude-3-haiku-20240307",
    max_tokens=100,
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.content[0].text)
```

### Troubleshooting Checklist

1. ✅ Using `.credentials.json` (hidden file with dot prefix)?
2. ✅ Including `anthropic-beta: oauth-2025-04-20` header?
3. ✅ Token not expired? Check `expiresAt` field.
4. ✅ Using `Authorization: Bearer` (not `x-api-key`)?
5. ✅ Correct API version header `anthropic-version: 2023-06-01`?

---

*Documentation generated by analyzing Claude Code v2.1.29*
