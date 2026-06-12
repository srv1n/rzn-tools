# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security seriously. If you discover a security vulnerability, please report it responsibly.

### How to Report

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email security concerns to: [security@rzn-tools.dev]

Or use GitHub's private vulnerability reporting:
1. Go to the Security tab of this repository
2. Click "Report a vulnerability"
3. Fill out the form with details

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Initial Response**: Within 48 hours
- **Status Update**: Within 7 days
- **Resolution Target**: Within 30 days (depending on severity)

### What to Expect

1. Acknowledgment of your report
2. Assessment of severity and impact
3. Development of a fix
4. Coordinated disclosure (if applicable)
5. Credit in the security advisory (if desired)

## Security Best Practices for Users

### Credential Storage

**Warning:** The `FileAuthStore` stores credentials as **plaintext JSON**. This is suitable for single-user development machines but not recommended for shared or production environments.

Credentials are stored in:
- **macOS/Linux**: `~/.config/rzn-tools/auth.json` (mode 0600)
- **Windows**: `%APPDATA%\rzn-tools\auth.json`

On Unix systems, the file is automatically set to mode 0600 (owner read/write only). On Windows, file permissions depend on the user's NTFS settings.

**Recommendations:**
- Use environment variables for CI/CD and production environments
- Use environment variables on shared machines
- Rotate API keys regularly
- Use minimal permission scopes for tokens
- Never commit credentials to version control
- Consider encrypting your home directory on laptops

### Environment Variables

Preferred method for production:

```bash
export OPENAI_API_KEY="sk-..."
export GITHUB_TOKEN="ghp_..."
export SLACK_TOKEN="xoxb-..."
```

### File Permissions

The auth.json file should have restricted permissions:

```bash
chmod 600 ~/.config/rzn-tools/auth.json
```

## Security Features

### Built-in Protections

- No credentials logged or printed
- HTTPS for all external requests
- Rate limiting to prevent abuse
- Input validation on all parameters

### Authentication Methods

| Method | Security Level | Notes |
|--------|---------------|-------|
| API Keys | Medium | Rotate regularly |
| OAuth Tokens | High | Use refresh tokens |
| Browser Cookies | Low | Session-based, expires |
| Environment Vars | High | Recommended for production |

## Known Limitations

- Browser cookie extraction requires filesystem access
- Some connectors may expose data in MCP tool responses
- Rate limiting is per-connector, not global

## Acknowledgments

We appreciate security researchers who help keep rzn-tools secure. Contributors will be acknowledged in security advisories (with permission).
