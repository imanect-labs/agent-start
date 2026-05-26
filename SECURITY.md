# Security policy

## Threat model & deployment guidance

agent-start is a **self-hosted launcher for command-line agents** (Claude Code,
Codex, etc.). By design it:

- Runs `bash -lc '<agent> ...'` inside a PTY on the host machine.
- Passes flags such as `--dangerously-skip-permissions` / `--full-auto` so that
  the agent does not pause for confirmation.
- Has **no built-in authentication or authorization** layer. Any HTTP client
  that can reach the listener can start new PTY sessions, run arbitrary shell
  commands, read/write files under the configured project roots, and proxy
  through `code-server`.

**This means anyone who can connect to the listener can run arbitrary code
as the user that started the host.**

### Required deployment posture

- The default bind address is `127.0.0.1`. **Keep it that way for any host
  that is not on a fully trusted network.**
- If you bind to a non-loopback interface (`--bind 0.0.0.0` or similar),
  agent-start must only be reachable through:
  - A **tailnet / VPN** (e.g. Tailscale, WireGuard), or
  - A **LAN behind a firewall** that explicitly blocks inbound access from
    outside that LAN.
- **Do not expose agent-start directly to the public internet.** Even behind
  basic-auth or IP allowlists at a reverse proxy, the lack of in-app auth and
  the agent's filesystem access make this dangerous. If you must put it behind
  a reverse proxy, terminate strong authentication (mTLS, OIDC) at the proxy
  and treat misconfiguration as an incident.

The host process runs as your user account and can read/write everything that
user can. Do not run agent-start as `root`.

## Supported versions

agent-start is pre-1.0; only the latest `0.x` release receives security fixes.

| Version  | Supported |
| -------- | --------- |
| 0.1.x    | ✅        |
| < 0.1.0  | ❌        |

## Reporting a vulnerability

**Please do not open a public GitHub issue for security problems.** Use one of
the following private channels:

1. **Preferred**: GitHub's [Private Vulnerability Reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
   on `imanect-labs/agent-start` (Security → Report a vulnerability).
2. Otherwise, email a maintainer privately (see the GitHub organization page
   for current contacts).

Please include:

- A description of the issue and its impact.
- Reproduction steps or a proof of concept.
- The version / commit hash you tested against.
- Any suggested mitigation.

We aim to acknowledge reports within 7 days. Once a fix is available we will
publish a GitHub Security Advisory and credit the reporter (unless you ask
otherwise).

Thank you for helping keep agent-start and its users safe.
