# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| latest  | :white_check_mark: |
| < 1.0   | :x:                |

## Reporting a Vulnerability

We take the security of Context Pilot seriously. If you believe you have found a security vulnerability, please report it to us as described below.

### How to Report

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please send an email to **g.draznieks@gmail.com** with the following information:

1. **Type of vulnerability** (e.g., remote code execution, information disclosure, authentication bypass)
2. **Location** of the affected source code (file path, line numbers if known)
3. **Step-by-step instructions** to reproduce the issue
4. **Proof-of-concept or exploit code** (if possible)
5. **Impact assessment** - what an attacker could achieve

### What to Expect

- **Acknowledgment**: We will acknowledge receipt of your report within 48 hours
- **Initial Assessment**: We will provide an initial assessment within 5 business days
- **Resolution Timeline**: We aim to resolve critical vulnerabilities within 30 days
- **Disclosure**: We will coordinate with you on public disclosure timing

### Safe Harbor

We consider security research conducted in good faith to be protected activity. We will not pursue legal action against researchers who:

- Make a good faith effort to avoid privacy violations and data destruction
- Do not exploit vulnerabilities beyond what is necessary to demonstrate the issue
- Report vulnerabilities promptly and do not disclose publicly before resolution

## Security Considerations

### API Keys

Context Pilot handles API keys for various LLM providers. Users should:

- Never commit `.env` files or API keys to version control
- Use environment variables for sensitive credentials
- Rotate keys if you suspect compromise

### Local Storage

Context Pilot stores data in `.context-pilot/` directory:

- `config.json` - Configuration (no sensitive data)
- `messages/` - Conversation history (may contain sensitive content)
- `panels/` - Panel metadata
- `states/` - Worker state

**Recommendation**: Do not share your `.context-pilot/` directory publicly.

### Tmux Integration

The tmux tools execute commands in terminal panes. Be aware that:

- The AI assistant can execute arbitrary commands via `console_send_keys`
- Review commands before allowing execution in sensitive environments
- Consider disabling tmux tools in high-security contexts

### File System Access

The AI can read and write files in your project directory:

- `file_open` - Reads file contents
- `file_edit` / `file_write` - Modifies or creates files
- Review all file modifications before committing

### Network Access

The application makes outbound connections to:

- Anthropic API (`api.anthropic.com`)
- xAI/Grok API (`api.x.ai`)
- Groq API (`api.groq.com`)

No telemetry or analytics data is collected.

## Security Best Practices

1. **Review tool executions** - Always review what the AI is doing, especially file edits and terminal commands
2. **Use in sandboxed environments** - For untrusted projects, run in containers or VMs
3. **Keep updated** - Use the latest version for security fixes
4. **Limit permissions** - Run with minimal necessary file system permissions
5. **Audit logs** - Check `.context-pilot/errors/` for any suspicious activity

## Acknowledgments

We appreciate the security research community's efforts in helping keep Context Pilot secure. Researchers who report valid vulnerabilities will be acknowledged here (with permission).

---

Thank you for helping keep Context Pilot and its users safe!
