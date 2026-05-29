# Licensing

agent-start is **open core**. Most of the project is free and open source under
the MIT License; a small set of commercial features lives under a separate
source-available Enterprise Edition (EE) license.

This page is the source of truth for *which license applies to which code*.

## TL;DR

| Part of the repo | License | You may… |
| --- | --- | --- |
| Everything **except** `enterprise/` | [MIT](./LICENSE) | use, modify, self-host, and redistribute freely — including commercially |
| The `enterprise/` directory | [Enterprise Edition License](./enterprise/LICENSE) | read the source and contribute; **production use requires a paid subscription** |

If a file is not inside `enterprise/`, it is MIT. There are no other exceptions.

## Why open core

The single-user, self-hosted experience — launching `claude` / `codex` and
other agents, persistent terminals, worktrees, diff viewer, chat UI — is and
will remain **MIT**. That is the part we want everyone to run, fork, and build
on without ever talking to us.

The features that only matter once *a team or a regulated organization* adopts
agent-start — SSO/SAML, role-based access control, audit logging, multi-host
orchestration, policy enforcement, cost/usage observability — live under the
Enterprise Edition license and fund the project.

We follow the model used by GitLab (`ee/`), Sentry, and PostHog: a permissive
open-source core plus a clearly fenced-off commercial directory.

## The MIT core

The MIT License in [`LICENSE`](./LICENSE) covers the entire repository **except**
the `enterprise/` directory. You can self-host the core, modify it, and even
ship it inside your own commercial product, subject only to the MIT terms
(keep the copyright notice).

## The Enterprise Edition (`enterprise/`)

Code under [`enterprise/`](./enterprise/) is licensed under the
[agent-start Enterprise Edition License](./enterprise/LICENSE). It is
**source-available, not open source**:

- You may read the code, build it, and run it for **development, testing, and
  evaluation**.
- You may submit contributions to it (see [CONTRIBUTING.md](./CONTRIBUTING.md)).
- **Using Enterprise features in production requires a valid commercial
  subscription** from imanect-labs.
- You may not copy EE code into the MIT core or any other project to work
  around these terms.

Want a subscription, a trial, or just to talk through whether you need one?
Email **goto@imanect.co.jp**.

## Contributing

By contributing to this repository you agree that:

- Contributions to MIT-licensed code are made under the MIT License.
- Contributions to `enterprise/` are made under the Enterprise Edition License
  (its contribution terms, Section 5, grant us the rights needed to keep
  offering the EE features commercially).

All contributions require a **Developer Certificate of Origin (DCO)** sign-off —
add `Signed-off-by: Your Name <you@example.com>` to each commit (`git commit -s`)
to certify you wrote the change or otherwise have the right to submit it under
the applicable license. The full text is in [`DCO`](./DCO). For significant
contributions to `enterprise/` we may additionally ask you to sign a Contributor
License Agreement (CLA). See [CONTRIBUTING.md](./CONTRIBUTING.md).

## A note on the open-core boundary

The boundary is **structural**: it is the `enterprise/` directory, nothing
else. We will never silently relicense code that already shipped under MIT — a
released MIT version stays MIT forever. New commercial features go into
`enterprise/` from the start; we do not move existing MIT code behind the
paywall.

## Not legal advice

These documents describe our intent and are provided as-is. The Enterprise
Edition License text is a starting template and should be reviewed by a lawyer
before you rely on it commercially. If anything here conflicts with the actual
license files, the license files in [`LICENSE`](./LICENSE) and
[`enterprise/LICENSE`](./enterprise/LICENSE) control.
