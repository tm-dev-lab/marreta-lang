# Security Policy

## Reporting a vulnerability

Do not open a public issue for a suspected security vulnerability. A public report for a language
runtime exposes users before a fix exists.

Report privately instead, through either:

- GitHub private vulnerability reporting: use the **Report a vulnerability** button under the
  repository's **Security** tab. This opens a private advisory only maintainers can see.
- Email: <contact@marreta.dev>.

## What to include

- A description of the issue and its impact.
- The `marreta --version` output and your platform (Linux, macOS, or Windows via WSL).
- A minimal reproduction (a small `.marreta` project or commands) where possible.
- Any relevant logs, with secrets removed.

## What to expect

This is a small maintainer team. You can expect an initial acknowledgement within a few days. We
will confirm the issue, work on a fix, and coordinate disclosure with you. Please give us a
reasonable window to address the report before any public discussion.

## Supported versions

Marreta is pre-launch and ships from a single line of development. Security fixes land on the latest
release. There is no separate long-term-support branch yet.
