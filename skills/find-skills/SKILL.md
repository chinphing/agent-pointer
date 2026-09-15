---
name: find-skills
description: Helps users discover and install agent skills when they ask questions like "how do I do X", "find a skill for X", "is there a skill that can...", or express interest in extending capabilities. This skill should be used when the user is looking for functionality that might exist as an installable skill.
---

# Find Skills

This skill helps you discover and install skills from the open agent skills ecosystem.

## When to Use This Skill

Use this skill when the user:

- The conversation includes `<!-- pointer-unsupported-attachment -->` or
  `<!-- pointer-media-processing-failed -->` (host could not parse an attachment inline)
- Asks "how do I do X" where X might be a common task with an existing skill
- Says "find a skill for X" or "is there a skill for X"
- Asks "can you do X" where X is a specialized capability
- Expresses interest in extending agent capabilities
- Wants to search for tools, templates, or workflows
- Mentions they wish they had help with a specific domain (design, testing, deployment, etc.)

## What is the Skills CLI?

The Skills CLI (`npx skills`) is the package manager for the open agent skills ecosystem. Skills are modular packages that extend agent capabilities with specialized knowledge, workflows, and tools.

**Key commands:**

- `npx skills find [query]` - Search for skills interactively or by keyword
- `npx skills add <package>` - Install a skill from GitHub or other sources
- `npx skills check` - Check for skill updates
- `npx skills update` - Update all installed skills

**Prerequisite:** `npx` ships with Node.js. If `npx` or `node` is missing, load the
**`dev-env-setup`** skill first and install the Node.js environment before running
any `npx skills` command.

**Browse skills at:** https://skills.sh/

## How to Help Users Find Skills

### Step 0: Check Already Installed Skills (required)

**Before any `npx skills find` or `npx skills add`:**

1. Read the **可用 Skills** index in your system instructions (enabled skills for this session).
2. Judge whether any skill's `name`, `description`, or `tags` fits the task (attachment type,
   file name, MIME, user goal). If yes, call **`skill_read`** and follow its body —
   **stop here**.
3. **Never** run `npx skills find` when a matching enabled skill already exists.
4. Only continue below when no enabled skill fits.

### Step 1: Understand What They Need

When a user asks for help with something, identify:

1. The domain (e.g., React, testing, design, deployment)
2. The specific task (e.g., writing tests, creating animations, reviewing PRs)
3. Whether this is a common enough task that a skill likely exists

### Step 2: Check the Leaderboard First

Before running a CLI search, check the [skills.sh leaderboard](https://skills.sh/) to see if a well-known skill already exists for the domain. The leaderboard ranks skills by total installs, surfacing the most popular and battle-tested options.

For example, top skills for web development include:
- `vercel-labs/agent-skills` — React, Next.js, web design (100K+ installs each)
- `anthropics/skills` — Frontend design, document processing (100K+ installs)

### Step 3: Search for Skills

If the leaderboard doesn't cover the user's need, run the find command:

```bash
npx skills find [query]
```

For example:

- User asks "how do I make my React app faster?" → `npx skills find react performance`
- User asks "can you help me with PR reviews?" → `npx skills find pr review`
- User asks "I need to create a changelog" → `npx skills find changelog`

### Step 4: Verify Quality Before Recommending

**Do not recommend a skill based solely on search results.** Always verify:

1. **Install count** — Prefer skills with 1K+ installs. Be cautious with anything under 100.
2. **Source reputation** — Official sources (`vercel-labs`, `anthropics`, `microsoft`) are more trustworthy than unknown authors.
3. **GitHub stars** — Check the source repository. A skill from a repo with <100 stars should be treated with skepticism.

### Step 5: Present Options to the User

When you find relevant skills, present them to the user with:

1. The skill name and what it does
2. The install count and source
3. The install command they can run
4. A link to learn more at skills.sh

Example response:

```
I found a skill that might help! The "react-best-practices" skill provides
React and Next.js performance optimization guidelines from Vercel Engineering.
(185K installs)

To install it:
npx skills add vercel-labs/agent-skills@react-best-practices

Learn more: https://skills.sh/vercel-labs/agent-skills/react-best-practices
```

### Step 6: Offer to Install

If the user wants to proceed, you can install the skill for them:

```bash
npx skills add <owner/repo@skill> -g -y
```

The `-g` flag installs globally (user-level) and `-y` skips confirmation prompts.
Then **`skill_import`** (`auto_enable` true), or the user enables it in Settings.

## GitHub Download Fallback (Mainland China)

`npx skills add` and related commands fetch skill packages from GitHub.
If the network cannot reach `github.com` (timeout, `ETIMEDOUT`,
`ECONNRESET`, `Failed to connect`, or hung clone/download), **do not stop
after one failure**. Switch to a domestic GitHub proxy and retry.

### Step 1: Route Git through a proxy (preferred)

Configure Git to rewrite GitHub HTTPS URLs, then run the same install
command again. Try mirrors **in order** until one succeeds.

**Common mirrors** (third-party; availability changes — try the next if one
fails or times out):

| Mirror | Git `insteadOf` prefix (append `https://github.com/`) |
| ------ | ----------------------------------------------------- |
| ghproxy.net (default) | `https://ghproxy.net/` |
| gh-proxy.com | `https://gh-proxy.com/` |
| mirror.ghproxy.com | `https://mirror.ghproxy.com/` |
| ghps.cc | `https://ghps.cc/` |
| ghproxy.link | `https://ghproxy.link/` |
| moeyy.xyz | `https://github.moeyy.xyz/` |

Example with the default mirror:

```bash
git config --global url."https://ghproxy.net/https://github.com/".insteadOf "https://github.com/"
npx skills add <owner/repo@skill> -g -y
```

Switch mirror — unset the old rule, then set the new prefix:

```bash
git config --global --unset url.https://ghproxy.net/https://github.com/.insteadOf
git config --global url."https://gh-proxy.com/https://github.com/".insteadOf "https://github.com/"
npx skills add <owner/repo@skill> -g -y
```

Start with **ghproxy.net**; only rotate mirrors after a clear failure.

To remove proxy routing when done:

```bash
git config --global --unset url.https://ghproxy.net/https://github.com/.insteadOf
# repeat --unset for any other mirror prefix you configured
```

### Step 2: Manual download via proxy (when CLI still fails)

1. Build the original GitHub URL for the skill repo or archive, for example:
   `https://github.com/owner/repo/archive/refs/heads/main.zip`
2. Prefix the **full** URL with a mirror base (same list as above), for
   example:
   - `https://ghproxy.net/https://github.com/owner/repo/archive/refs/heads/main.zip`
   - `https://gh-proxy.com/https://github.com/owner/repo/archive/refs/heads/main.zip`
   - `https://mirror.ghproxy.com/https://github.com/owner/repo/archive/refs/heads/main.zip`
3. If one mirror fails, try the next row in the table before giving up.
4. Download and extract the skill folder (or the subdirectory named in
   `owner/repo@skill`).
5. Install with **`skill_import`** (zip or local directory, `auto_enable` true).

### Step 3: Tell the user what changed

Briefly explain that GitHub was unreachable directly and the install used
a China-accessible proxy. If proxy install also fails, offer to help manually
or proceed without the skill.

**Do not** assume every user is in mainland China — only apply this section
after a GitHub connectivity or timeout error, or when the user says GitHub
is blocked or slow.

## Common Skill Categories

When searching, consider these common categories:

| Category        | Example Queries                          |
| --------------- | ---------------------------------------- |
| Web Development | react, nextjs, typescript, css, tailwind |
| Testing         | testing, jest, playwright, e2e           |
| DevOps          | deploy, docker, kubernetes, ci-cd        |
| Documentation   | docs, readme, changelog, api-docs        |
| Code Quality    | review, lint, refactor, best-practices   |
| Design          | ui, ux, design-system, accessibility     |
| Productivity    | workflow, automation, git                |

## Tips for Effective Searches

1. **Use specific keywords**: "react testing" is better than just "testing"
2. **Try alternative terms**: If "deploy" doesn't work, try "deployment" or "ci-cd"
3. **Check popular sources**: Many skills come from `vercel-labs/agent-skills` or `ComposioHQ/awesome-claude-skills`

## When No Skills Are Found

If no relevant skills exist:

1. Acknowledge that no existing skill was found
2. Offer to help with the task directly using your general capabilities
3. Suggest the user could create their own skill with `npx skills init`

Example:

```
I searched for skills related to "xyz" but didn't find any matches.
I can still help you with this task directly! Would you like me to proceed?

If this is something you do often, you could create your own skill:
npx skills init my-xyz-skill
```
