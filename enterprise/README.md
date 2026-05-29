# agent-start Enterprise Edition (`enterprise/`)

> ⚠️ **Different license.** Code in this directory is **NOT** MIT. It is
> source-available under the [agent-start Enterprise Edition License](./LICENSE).
> Production use requires a paid subscription. Everything *outside* this
> directory is MIT. See the top-level [LICENSING.md](../LICENSING.md).

This directory holds the commercial, team-and-enterprise features that fund
agent-start. The single-user, self-hosted core stays free and MIT — these are
the capabilities that only matter once an organization adopts agent-start.

## Planned scope

These features will land here (none implemented yet — this directory currently
only establishes the licensing boundary):

- **Authentication** — SSO / SAML / OIDC, session management.
- **RBAC** — roles, per-project and per-agent permissions.
- **Audit logging** — who ran which agent, with what prompt, against which repo.
- **Policy enforcement** — who may use `--dangerously-skip-permissions`, secret
  and network boundaries per workspace.
- **Multi-host orchestration** — one console across many hosts / developers.
- **Cost & usage observability** — agent run cost aggregation and dashboards.

## Building / running

The EE Software may be built and run for development, testing, and evaluation
without a subscription. See [LICENSING.md](../LICENSING.md) for what counts as
production use.

## Contributing

Contributions here are accepted under the Enterprise Edition License; see
[CONTRIBUTING.md](../CONTRIBUTING.md). Significant contributions may require a
Contributor License Agreement.
