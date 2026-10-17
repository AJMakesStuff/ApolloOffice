# ApolloOffice

An office of small Claude workers that turns GitHub issues into verified pull requests. Add any GitHub repository, and ApolloOffice picks up its bug reports (or hunts for bugs itself), reproduces them, writes a fix with a regression test, proves the test fails without the fix and passes with it, runs the project's tests, reviews the change and opens a pull request. You watch the workers in a small pixel office in your browser.

It runs on your own computer, with your own Claude Code login and your own GitHub account. Nothing goes through a server of ours.

## How it works

One fixed pipeline, each step a role with its own model:

| Role | Default model | Job |
|---|---|---|
| Scout | Haiku | Reads new issues and picks clear, reproducible bugs |
| Guard | Haiku | Checks every text from strangers for prompt injection before anyone else sees it |
| Vet | Haiku | Applies the selection rules, rejects claimed, duplicate or unwanted issues |
| Hunter | Sonnet | Optional: searches the code for one real bug and proves it with a failing test |
| Repro | Haiku, then Sonnet | Reproduces the bug on the default branch |
| Fixer | Sonnet (Opus for complex bugs) | Writes a regression test commit and a fix commit |
| CI | – | Runs your test command |
| Reviewer | Haiku | Checks the diff and the PR text |
| Submit | – | Pushes the branch (to a fork if needed) and opens the PR |
| Watcher | Haiku | Follows your open PRs: merges, review requests, merge conflicts |

The proof is checked by ApolloOffice itself, not by a model: the regression test must fail on the test commit and pass on the fix commit, and the test command must pass (failures that also happen on the default branch are tolerated and named in the PR). Without proof nothing is submitted.

By default a finished pull request waits for your approval in the browser, with diff, proof and an editable PR text. You can switch a repository to submit automatically.

## Requirements

- `git`, and on Windows Git for Windows (for `bash`)
- [GitHub CLI](https://cli.github.com), logged in (`gh auth login`)
- [Claude Code](https://claude.com/claude-code), logged in
- Docker (recommended): the workers build and run the repositories' code inside a container. Without Docker they run it directly on your machine, which you have to confirm.

## Start

Download the binary for your system from the releases and run it in a terminal:

```
apollo-office
```

It prints a one-time login link. Open it, then follow the setup: add a machine (this computer, or any machine you reach with `ssh`), add a repository and check its test command.

```
apollo-office login-link      new login link (10 minutes)
apollo-office autostart on    start at login (Windows, macOS, Linux)
apollo-office status
```

Options: `--port` (default 47300), `--data <folder>`.

## AI disclosure

Every commit carries `Co-Authored-By` trailers for the Claude models that worked on it, and every PR text says it was generated with Claude Code. You can turn this off per repository, but many projects require it.

## License

AGPL-3.0
