# GitHub CLI (`gh`) — Near-Exhaustive Cheat Sheet

## Setup & Authentication

| Command | Description | is_pure_description |
|---|---|---|
| `gh auth login` | Authenticate with GitHub (interactive) | false |
| `gh auth login --with-token < token.txt` | Authenticate via token from stdin | false |
| `gh auth login -h github.example.com` | Authenticate with GitHub Enterprise | false |
| `gh auth logout` | Log out of a GitHub account | false |
| `gh auth status` | Check authentication status | true |
| `gh auth token` | Print current auth token | true |
| `gh auth refresh -s <scopes>` | Refresh auth with additional scopes | false |
| `gh auth setup-git` | Configure git to use gh as credential helper | false |
| `gh auth switch` | Switch between authenticated accounts | false |

## Configuration

| Command | Description | is_pure_description |
|---|---|---|
| `gh config set editor vim` | Set default editor | false |
| `gh config set pager less` | Set default pager | false |
| `gh config set browser firefox` | Set default browser | false |
| `gh config set git_protocol ssh` | Set git protocol (ssh or https) | false |
| `gh config set prompt disabled` | Disable interactive prompts | false |
| `gh config get editor` | Get a config value | true |
| `gh config list` | List all config values | true |
| `gh config clear-cache` | Clear CLI cache | false |

## Repositories

| Command | Description | is_pure_description |
|---|---|---|
| `gh repo clone owner/repo` | Clone a repository | false |
| `gh repo clone owner/repo -- --depth=1` | Shallow clone | false |
| `gh repo create` | Create a new repo (interactive) | false |
| `gh repo create name --public` | Create a public repo | false |
| `gh repo create name --private` | Create a private repo | false |
| `gh repo create name --internal` | Create an internal repo (orgs) | false |
| `gh repo create name --template owner/tpl` | Create from template | false |
| `gh repo create name --clone` | Create and clone locally | false |
| `gh repo create --source .` | Create remote from existing local repo | false |
| `gh repo fork owner/repo` | Fork a repository | false |
| `gh repo fork owner/repo --clone` | Fork and clone locally | false |
| `gh repo fork owner/repo --remote` | Fork and add remote | false |
| `gh repo view` | View current repo details | true |
| `gh repo view owner/repo` | View a specific repo | true |
| `gh repo view --web` | Open repo in browser | true |
| `gh repo view --json name,description` | View repo as JSON | true |
| `gh repo list` | List your repos | true |
| `gh repo list <owner>` | List repos for a user/org | true |
| `gh repo list --language python` | Filter by language | true |
| `gh repo list --topic cli` | Filter by topic | true |
| `gh repo list --source` | Exclude forks | true |
| `gh repo list --fork` | Only forks | true |
| `gh repo list --archived` | Only archived repos | true |
| `gh repo list --no-archived` | Exclude archived repos | true |
| `gh repo list --limit 100` | Increase result limit | true |
| `gh repo list --json name,url --jq '.[].url'` | JSON + jq filtering | true |
| `gh repo edit` | Edit current repo settings (interactive) | false |
| `gh repo edit --description "..."` | Set repo description | false |
| `gh repo edit --visibility public` | Change repo visibility | false |
| `gh repo edit --default-branch main` | Set default branch | false |
| `gh repo edit --enable-issues=false` | Disable issues | false |
| `gh repo edit --enable-wiki=false` | Disable wiki | false |
| `gh repo edit --enable-projects=false` | Disable projects | false |
| `gh repo edit --add-topic cli,tool` | Add topics | false |
| `gh repo edit --remove-topic tool` | Remove topics | false |
| `gh repo edit --template` | Mark as template repo | false |
| `gh repo edit --enable-auto-merge` | Enable auto-merge | false |
| `gh repo edit --delete-branch-on-merge` | Auto-delete head branches after merge | false |
| `gh repo edit --allow-forking` | Allow forking (org repos) | false |
| `gh repo rename <new-name>` | Rename current repo | false |
| `gh repo delete <repo> --yes` | Delete a repository | false |
| `gh repo archive <repo>` | Archive a repository | false |
| `gh repo unarchive <repo>` | Unarchive a repository | false |
| `gh repo sync` | Sync fork with upstream | false |
| `gh repo sync --branch main` | Sync a specific branch | false |
| `gh repo sync --source owner/repo` | Sync from a specific source | false |
| `gh repo set-default` | Set default remote repo (interactive) | false |
| `gh repo set-default owner/repo` | Set default remote repo | false |
| `gh repo deploy-key add key.pub` | Add a deploy key | false |
| `gh repo deploy-key add key.pub --allow-write` | Add writable deploy key | false |
| `gh repo deploy-key list` | List deploy keys | true |
| `gh repo deploy-key delete <key-id>` | Delete a deploy key | false |

## Pull Requests

| Command | Description | is_pure_description |
|---|---|---|
| `gh pr create` | Create a PR (interactive) | false |
| `gh pr create --fill` | Create PR, auto-fill title/body from commits | false |
| `gh pr create --title "..." --body "..."` | Create PR with title/body | false |
| `gh pr create --draft` | Create a draft PR | false |
| `gh pr create --base main` | Target a specific base branch | false |
| `gh pr create --head feature-branch` | Specify head branch | false |
| `gh pr create --reviewer user1,user2` | Request reviewers | false |
| `gh pr create --assignee @me` | Assign to yourself | false |
| `gh pr create --label "bug,priority"` | Add labels | false |
| `gh pr create --milestone "v1.0"` | Set milestone | false |
| `gh pr create --project "Board"` | Add to project | false |
| `gh pr create --web` | Open PR creation in browser | false |
| `gh pr create --no-maintainer-edit` | Disallow maintainer edits | false |
| `gh pr create --template <file>` | Use a PR template | false |
| `gh pr list` | List open PRs | true |
| `gh pr list --state all` | List all PRs | true |
| `gh pr list --state closed` | List closed PRs | true |
| `gh pr list --state merged` | List merged PRs | true |
| `gh pr list --author @me` | PRs by you | true |
| `gh pr list --assignee @me` | PRs assigned to you | true |
| `gh pr list --label "bug"` | Filter by label | true |
| `gh pr list --base main` | Filter by base branch | true |
| `gh pr list --head feature` | Filter by head branch | true |
| `gh pr list --draft` | Only draft PRs | true |
| `gh pr list --search "review:required"` | Advanced search filter | true |
| `gh pr list --limit 50` | Increase result limit | true |
| `gh pr list --json number,title,author` | Output as JSON | true |
| `gh pr view <number>` | View PR details | true |
| `gh pr view --web` | Open PR in browser | true |
| `gh pr view --json additions,deletions` | View PR stats as JSON | true |
| `gh pr view --comments` | View PR with comments | true |
| `gh pr status` | Show PR status for current branch | true |
| `gh pr checkout <number>` | Check out a PR branch locally | false |
| `gh pr checkout <number> --detach` | Check out detached HEAD | false |
| `gh pr checkout <number> --force` | Force checkout | false |
| `gh pr checkout <number> -b <branch>` | Check out into custom branch name | false |
| `gh pr diff <number>` | View PR diff | true |
| `gh pr diff <number> --patch` | View diff in patch format | true |
| `gh pr diff <number> --name-only` | List changed files only | true |
| `gh pr checks <number>` | View CI/check status | true |
| `gh pr checks <number> --watch` | Watch checks in real time | true |
| `gh pr checks <number> --required` | Show only required checks | true |
| `gh pr review <number>` | Start a review (interactive) | false |
| `gh pr review <number> --approve` | Approve a PR | false |
| `gh pr review <number> --approve --body "LGTM"` | Approve with comment | false |
| `gh pr review <number> --request-changes --body "..."` | Request changes | false |
| `gh pr review <number> --comment --body "..."` | Leave a review comment | false |
| `gh pr comment <number> --body "..."` | Add a comment to PR | false |
| `gh pr comment <number> --body-file file.md` | Comment from file | false |
| `gh pr comment <number> --edit-last --body "..."` | Edit your last comment | false |
| `gh pr comment <number> --web` | Comment via browser | false |
| `gh pr edit <number>` | Edit PR (interactive) | false |
| `gh pr edit <number> --title "New title"` | Change PR title | false |
| `gh pr edit <number> --body "..."` | Change PR body | false |
| `gh pr edit <number> --base main` | Change base branch | false |
| `gh pr edit <number> --add-label "bug"` | Add label | false |
| `gh pr edit <number> --remove-label "wip"` | Remove label | false |
| `gh pr edit <number> --add-reviewer user1` | Add reviewer | false |
| `gh pr edit <number> --remove-reviewer user1` | Remove reviewer | false |
| `gh pr edit <number> --add-assignee @me` | Add assignee | false |
| `gh pr edit <number> --add-project "Board"` | Add to project | false |
| `gh pr edit <number> --milestone "v1.0"` | Set milestone | false |
| `gh pr merge <number>` | Merge a PR (interactive) | false |
| `gh pr merge --merge` | Merge commit | false |
| `gh pr merge --squash` | Squash merge | false |
| `gh pr merge --rebase` | Rebase merge | false |
| `gh pr merge --auto` | Enable auto-merge when checks pass | false |
| `gh pr merge --auto --squash` | Auto-merge with squash | false |
| `gh pr merge --disable-auto` | Disable auto-merge | false |
| `gh pr merge --delete-branch` | Delete branch after merge | false |
| `gh pr merge --admin` | Merge bypassing protections (admin) | false |
| `gh pr merge --match-head-commit <sha>` | Merge only if HEAD matches SHA | false |
| `gh pr merge --subject "..."` | Custom merge commit subject | false |
| `gh pr merge --body "..."` | Custom merge commit body | false |
| `gh pr close <number>` | Close a PR | false |
| `gh pr close <number> --delete-branch` | Close and delete branch | false |
| `gh pr close <number> --comment "..."` | Close with comment | false |
| `gh pr reopen <number>` | Reopen a PR | false |
| `gh pr ready <number>` | Mark draft PR as ready for review | false |
| `gh pr lock <number>` | Lock PR conversation | false |
| `gh pr lock <number> --reason "resolved"` | Lock with reason | false |
| `gh pr unlock <number>` | Unlock PR conversation | false |
| `gh pr update-branch <number>` | Update PR branch from base | false |
| `gh pr update-branch --rebase` | Update branch via rebase | false |

## Issues

| Command | Description | is_pure_description |
|---|---|---|
| `gh issue create` | Create an issue (interactive) | false |
| `gh issue create --title "..." --body "..."` | Create with title/body | false |
| `gh issue create --body-file issue.md` | Create from file | false |
| `gh issue create --label "bug,urgent"` | Create with labels | false |
| `gh issue create --assignee @me,user2` | Create with assignees | false |
| `gh issue create --milestone "v1.0"` | Set milestone | false |
| `gh issue create --project "Board"` | Add to project | false |
| `gh issue create --template "bug_report.md"` | Use issue template | false |
| `gh issue create --web` | Open creation in browser | false |
| `gh issue list` | List open issues | true |
| `gh issue list --state all` | List all issues | true |
| `gh issue list --state closed` | List closed issues | true |
| `gh issue list --label "bug"` | Filter by label | true |
| `gh issue list --assignee @me` | Issues assigned to you | true |
| `gh issue list --author @me` | Issues created by you | true |
| `gh issue list --mention @me` | Issues mentioning you | true |
| `gh issue list --milestone "v1.0"` | Filter by milestone | true |
| `gh issue list --search "is:open sort:created-asc"` | Advanced search filter | true |
| `gh issue list --limit 100` | Increase result limit | true |
| `gh issue list --json number,title,labels` | Output as JSON | true |
| `gh issue view <number>` | View issue details | true |
| `gh issue view <number> --web` | Open in browser | true |
| `gh issue view <number> --comments` | View with comments | true |
| `gh issue view <number> --json body` | View body as JSON | true |
| `gh issue status` | Show issue status (assigned, mentioned, created) | true |
| `gh issue comment <number> --body "..."` | Add a comment | false |
| `gh issue comment <number> --body-file file.md` | Comment from file | false |
| `gh issue comment <number> --edit-last --body "..."` | Edit your last comment | false |
| `gh issue comment <number> --web` | Comment via browser | false |
| `gh issue edit <number>` | Edit issue (interactive) | false |
| `gh issue edit <number> --title "New title"` | Change title | false |
| `gh issue edit <number> --body "..."` | Change body | false |
| `gh issue edit <number> --add-label "bug"` | Add label | false |
| `gh issue edit <number> --remove-label "wip"` | Remove label | false |
| `gh issue edit <number> --add-assignee user1` | Add assignee | false |
| `gh issue edit <number> --remove-assignee user1` | Remove assignee | false |
| `gh issue edit <number> --add-project "Board"` | Add to project | false |
| `gh issue edit <number> --milestone "v1.0"` | Set milestone | false |
| `gh issue close <number>` | Close an issue | false |
| `gh issue close <number> --reason "completed"` | Close with reason | false |
| `gh issue close <number> --reason "not planned"` | Close as not planned | false |
| `gh issue close <number> --comment "..."` | Close with comment | false |
| `gh issue reopen <number>` | Reopen an issue | false |
| `gh issue delete <number>` | Delete an issue | false |
| `gh issue transfer <number> owner/repo` | Transfer to another repo | false |
| `gh issue pin <number>` | Pin an issue | false |
| `gh issue unpin <number>` | Unpin an issue | false |
| `gh issue lock <number>` | Lock issue conversation | false |
| `gh issue lock <number> --reason "resolved"` | Lock with reason | false |
| `gh issue unlock <number>` | Unlock issue conversation | false |
| `gh issue develop <number>` | Create a branch for an issue | false |
| `gh issue develop <number> --name "fix-123"` | Branch with custom name | false |
| `gh issue develop <number> --checkout` | Create branch and check out | false |

## Labels

| Command | Description | is_pure_description |
|---|---|---|
| `gh label create "bug"` | Create a label | false |
| `gh label create "bug" --color FF0000` | Create label with color | false |
| `gh label create "bug" --description "..."` | Create label with description | false |
| `gh label list` | List all labels | true |
| `gh label list --search "bug"` | Search labels | true |
| `gh label list --json name,color` | List labels as JSON | true |
| `gh label edit "bug" --name "defect"` | Rename a label | false |
| `gh label edit "bug" --color 00FF00` | Change label color | false |
| `gh label edit "bug" --description "..."` | Change label description | false |
| `gh label delete "bug" --yes` | Delete a label | false |
| `gh label clone owner/repo` | Clone labels from another repo | false |
| `gh label clone owner/repo --overwrite` | Clone labels, overwrite existing | false |

## Milestones

| Command | Description | is_pure_description |
|---|---|---|
| `gh api repos/{owner}/{repo}/milestones` | List milestones (via API) | true |
| `gh api repos/{owner}/{repo}/milestones -f title="v1.0"` | Create milestone (via API) | false |

## GitHub Actions / Workflows

| Command | Description | is_pure_description |
|---|---|---|
| `gh workflow list` | List workflows | true |
| `gh workflow list --all` | Include disabled workflows | true |
| `gh workflow list --json name,state` | List as JSON | true |
| `gh workflow view <name-or-id>` | View workflow details | true |
| `gh workflow view <name-or-id> --yaml` | View workflow YAML | true |
| `gh workflow view <name-or-id> --web` | Open in browser | true |
| `gh workflow run <workflow>` | Trigger a workflow manually | false |
| `gh workflow run <workflow> --ref main` | Trigger on specific ref | false |
| `gh workflow run <workflow> -f key=value` | Trigger with input parameters | false |
| `gh workflow run <workflow> --json` | Trigger with JSON stdin inputs | false |
| `gh workflow enable <workflow>` | Enable a workflow | false |
| `gh workflow disable <workflow>` | Disable a workflow | false |
| `gh run list` | List recent workflow runs | true |
| `gh run list --workflow <name>` | Filter by workflow | true |
| `gh run list --branch main` | Filter by branch | true |
| `gh run list --user @me` | Filter by triggering user | true |
| `gh run list --status failure` | Filter by status | true |
| `gh run list --event push` | Filter by event type | true |
| `gh run list --created ">2024-01-01"` | Filter by creation date | true |
| `gh run list --json databaseId,status` | List as JSON | true |
| `gh run list --limit 50` | Increase result limit | true |
| `gh run view <run-id>` | View run details | true |
| `gh run view <run-id> --web` | Open run in browser | true |
| `gh run view <run-id> --log` | View full run logs | true |
| `gh run view <run-id> --log-failed` | View only failed step logs | true |
| `gh run view <run-id> --exit-status` | Exit non-zero if run failed | true |
| `gh run view <run-id> --json jobs` | View jobs as JSON | true |
| `gh run view <run-id> --verbose` | Verbose output | true |
| `gh run watch <run-id>` | Watch a run in real time | true |
| `gh run watch <run-id> --exit-status` | Watch and exit non-zero on failure | true |
| `gh run rerun <run-id>` | Re-run all jobs | false |
| `gh run rerun <run-id> --failed` | Re-run only failed jobs | false |
| `gh run rerun <run-id> --debug` | Re-run with debug logging | false |
| `gh run rerun <run-id> --job <job-id>` | Re-run a specific job | false |
| `gh run cancel <run-id>` | Cancel a running workflow | false |
| `gh run download <run-id>` | Download all artifacts | true |
| `gh run download <run-id> --name "artifact"` | Download specific artifact | true |
| `gh run download <run-id> --dir ./out` | Download to specific directory | true |
| `gh run download <run-id> --pattern "*.zip"` | Download matching pattern | true |
| `gh run delete <run-id>` | Delete a workflow run | false |

## Actions Cache

| Command | Description | is_pure_description |
|---|---|---|
| `gh cache list` | List action caches | true |
| `gh cache list --sort size` | Sort by size | true |
| `gh cache list --order asc` | Ascending order | true |
| `gh cache list --key "npm-"` | Filter by key prefix | true |
| `gh cache list --json key,sizeInBytes` | List as JSON | true |
| `gh cache delete <cache-id>` | Delete a specific cache | false |
| `gh cache delete --all` | Delete all caches | false |

## Secrets

| Command | Description | is_pure_description |
|---|---|---|
| `gh secret set NAME` | Set a secret (interactive) | false |
| `gh secret set NAME --body "value"` | Set a secret with value | false |
| `gh secret set NAME < secret.txt` | Set secret from file | false |
| `gh secret set NAME --env production` | Set environment secret | false |
| `gh secret set NAME --org my-org` | Set org-level secret | false |
| `gh secret set NAME --org my-org --visibility all` | Org secret visible to all repos | false |
| `gh secret set NAME --org my-org --repos "r1,r2"` | Org secret for specific repos | false |
| `gh secret list` | List repo secrets | true |
| `gh secret list --env production` | List environment secrets | true |
| `gh secret list --org my-org` | List org secrets | true |
| `gh secret list --json name,updatedAt` | List as JSON | true |
| `gh secret delete NAME` | Delete a secret | false |
| `gh secret delete NAME --env production` | Delete environment secret | false |
| `gh secret delete NAME --org my-org` | Delete org secret | false |

## Variables

| Command | Description | is_pure_description |
|---|---|---|
| `gh variable set NAME --body "value"` | Set a variable | false |
| `gh variable set NAME --env production` | Set environment variable | false |
| `gh variable set NAME --org my-org` | Set org-level variable | false |
| `gh variable list` | List repo variables | true |
| `gh variable list --env production` | List environment variables | true |
| `gh variable list --org my-org` | List org variables | true |
| `gh variable list --json name,value` | List as JSON | true |
| `gh variable get NAME` | Get a variable's value | true |
| `gh variable delete NAME` | Delete a variable | false |
| `gh variable delete NAME --env production` | Delete environment variable | false |

## Environments

| Command | Description | is_pure_description |
|---|---|---|
| `gh api repos/{owner}/{repo}/environments` | List environments (via API) | true |

## Gists

| Command | Description | is_pure_description |
|---|---|---|
| `gh gist create <file>` | Create a gist from file | false |
| `gh gist create <file1> <file2>` | Create gist with multiple files | false |
| `gh gist create --public <file>` | Create a public gist | false |
| `gh gist create -` | Create gist from stdin | false |
| `gh gist create --desc "description" <file>` | Create with description | false |
| `gh gist create --filename "name.py" -` | Create from stdin with filename | false |
| `gh gist create --web <file>` | Create and open in browser | false |
| `gh gist list` | List your gists | true |
| `gh gist list --public` | List public gists | true |
| `gh gist list --secret` | List secret gists | true |
| `gh gist list --limit 50` | Increase result limit | true |
| `gh gist view <id>` | View a gist | true |
| `gh gist view <id> --raw` | View raw content | true |
| `gh gist view <id> --filename "file.py"` | View specific file in gist | true |
| `gh gist view <id> --web` | Open gist in browser | true |
| `gh gist edit <id>` | Edit a gist (opens editor) | false |
| `gh gist edit <id> --add <file>` | Add a file to gist | false |
| `gh gist edit <id> --remove "file.py"` | Remove file from gist | false |
| `gh gist edit <id> --filename "f.py" -` | Update file from stdin | false |
| `gh gist edit <id> --desc "new desc"` | Update gist description | false |
| `gh gist clone <id>` | Clone a gist locally | false |
| `gh gist rename <id> <old> <new>` | Rename a file in gist | false |
| `gh gist delete <id>` | Delete a gist | false |

## Releases

| Command | Description | is_pure_description |
|---|---|---|
| `gh release create <tag>` | Create a release (interactive) | false |
| `gh release create <tag> --title "..."` | Create with title | false |
| `gh release create <tag> --notes "..."` | Create with release notes | false |
| `gh release create <tag> --notes-file CHANGELOG.md` | Notes from file | false |
| `gh release create <tag> --generate-notes` | Auto-generate release notes | false |
| `gh release create <tag> --target main` | Target specific commitish | false |
| `gh release create <tag> --draft` | Create as draft | false |
| `gh release create <tag> --prerelease` | Mark as pre-release | false |
| `gh release create <tag> --latest` | Mark as latest | false |
| `gh release create <tag> --discussion-category "..."` | Create discussion for release | false |
| `gh release create <tag> ./file.zip` | Upload asset with release | false |
| `gh release create <tag> ./a.zip ./b.tar.gz` | Upload multiple assets | false |
| `gh release list` | List releases | true |
| `gh release list --exclude-drafts` | Exclude draft releases | true |
| `gh release list --exclude-pre-releases` | Exclude pre-releases | true |
| `gh release list --limit 50` | Increase result limit | true |
| `gh release list --json tagName,isDraft` | List as JSON | true |
| `gh release view <tag>` | View release details | true |
| `gh release view <tag> --web` | Open in browser | true |
| `gh release view <tag> --json assets` | View assets as JSON | true |
| `gh release download <tag>` | Download all release assets | true |
| `gh release download <tag> --pattern "*.tar.gz"` | Download matching assets | true |
| `gh release download <tag> --dir ./out` | Download to directory | true |
| `gh release download <tag> --skip-existing` | Skip already downloaded files | true |
| `gh release download <tag> --output file.zip` | Download to specific filename | true |
| `gh release download <tag> --archive tar.gz` | Download source archive | true |
| `gh release edit <tag>` | Edit a release | false |
| `gh release edit <tag> --title "..."` | Change title | false |
| `gh release edit <tag> --notes "..."` | Change notes | false |
| `gh release edit <tag> --draft=false` | Publish a draft release | false |
| `gh release edit <tag> --prerelease=false` | Remove pre-release flag | false |
| `gh release edit <tag> --latest` | Set as latest release | false |
| `gh release edit <tag> --tag <new-tag>` | Change tag | false |
| `gh release upload <tag> ./file.zip` | Upload asset to existing release | false |
| `gh release upload <tag> ./file.zip --clobber` | Overwrite existing asset | false |
| `gh release delete <tag>` | Delete a release | false |
| `gh release delete <tag> --yes` | Delete without confirmation | false |
| `gh release delete <tag> --cleanup-tag` | Also delete the git tag | false |

## GitHub Projects (v2)

| Command | Description | is_pure_description |
|---|---|---|
| `gh project create --title "..."` | Create a project | false |
| `gh project create --owner @me` | Create user project | false |
| `gh project create --owner my-org` | Create org project | false |
| `gh project list` | List your projects | true |
| `gh project list --owner my-org` | List org projects | true |
| `gh project list --closed` | Include closed projects | true |
| `gh project list --json number,title` | List as JSON | true |
| `gh project view <number>` | View project details | true |
| `gh project view <number> --web` | Open project in browser | true |
| `gh project view <number> --json items` | View items as JSON | true |
| `gh project edit <number> --title "..."` | Rename project | false |
| `gh project edit <number> --description "..."` | Set description | false |
| `gh project edit <number> --visibility PUBLIC` | Change visibility | false |
| `gh project edit <number> --readme "..."` | Set project README | false |
| `gh project close <number>` | Close a project | false |
| `gh project delete <number>` | Delete a project | false |
| `gh project copy <number> --target-owner <o>` | Copy a project | false |
| `gh project mark-template <number>` | Mark as template | false |
| `gh project mark-template <number> --undo` | Unmark as template | false |
| `gh project link <number> --repo owner/repo` | Link project to repo | false |
| `gh project unlink <number> --repo owner/repo` | Unlink project from repo | false |
| `gh project field-create <number> --name "..." --data-type TEXT` | Add a field | false |
| `gh project field-create <number> --name "..." --data-type SINGLE_SELECT` | Add select field | false |
| `gh project field-list <number>` | List project fields | true |
| `gh project field-delete <number> --id <field-id>` | Delete a field | false |
| `gh project item-create <number> --title "..."` | Create a draft item | false |
| `gh project item-add <number> --url <issue-or-pr-url>` | Add issue/PR to project | false |
| `gh project item-list <number>` | List items in project | true |
| `gh project item-list <number> --json title,status` | List items as JSON | true |
| `gh project item-edit --project-id <id> --id <item-id> --field-id <f> --text "..."` | Edit item field | false |
| `gh project item-archive <number> --id <item-id>` | Archive an item | false |
| `gh project item-archive <number> --id <item-id> --undo` | Unarchive an item | false |
| `gh project item-delete <number> --id <item-id>` | Delete an item | false |

## Codespaces

| Command | Description | is_pure_description |
|---|---|---|
| `gh codespace create` | Create a codespace (interactive) | false |
| `gh codespace create --repo owner/repo` | Create for specific repo | false |
| `gh codespace create --branch feature` | Create on specific branch | false |
| `gh codespace create --machine largePremiumLinux` | Specify machine type | false |
| `gh codespace create --retention-period 72h` | Set retention period | false |
| `gh codespace create --idle-timeout 30m` | Set idle timeout | false |
| `gh codespace create --devcontainer-path .devcontainer/devcontainer.json` | Specific devcontainer | false |
| `gh cs list` | List your codespaces | true |
| `gh cs list --repo owner/repo` | List for specific repo | true |
| `gh cs list --org my-org` | List org codespaces | true |
| `gh cs list --json name,state,machineName` | List as JSON | true |
| `gh cs view` | View codespace details (interactive) | true |
| `gh cs view --json` | View details as JSON | true |
| `gh cs ssh` | SSH into a codespace (interactive) | true |
| `gh cs ssh -c <codespace-name>` | SSH into specific codespace | true |
| `gh cs ssh -- -L 8080:localhost:8080` | SSH with port forwarding | true |
| `gh cs code` | Open codespace in VS Code | true |
| `gh cs code --web` | Open codespace in browser editor | true |
| `gh cs code --insiders` | Open in VS Code Insiders | true |
| `gh cs jupyter` | Open Jupyter in a codespace | true |
| `gh cs cp local.txt remote:~/file.txt` | Copy file to codespace | false |
| `gh cs cp remote:~/file.txt ./local.txt` | Copy file from codespace | true |
| `gh cs cp -r ./dir remote:~/dir` | Copy directory to codespace | false |
| `gh cs ports` | List forwarded ports | true |
| `gh cs ports forward 8080:8080` | Forward a port | false |
| `gh cs ports visibility 8080:public` | Change port visibility | false |
| `gh cs logs` | View codespace logs | true |
| `gh cs stop` | Stop a codespace | false |
| `gh cs stop -c <codespace-name>` | Stop specific codespace | false |
| `gh cs rebuild` | Rebuild a codespace | false |
| `gh cs rebuild --full` | Full rebuild (no cache) | false |
| `gh cs edit` | Edit codespace settings | false |
| `gh cs edit --machine largePremiumLinux` | Change machine type | false |
| `gh cs delete` | Delete a codespace (interactive) | false |
| `gh cs delete -c <codespace-name>` | Delete specific codespace | false |
| `gh cs delete --all` | Delete all codespaces | false |
| `gh cs delete --days 7` | Delete codespaces older than N days | false |

## SSH Keys & GPG Keys

| Command | Description | is_pure_description |
|---|---|---|
| `gh ssh-key add <file>` | Add an SSH key | false |
| `gh ssh-key add <file> --title "laptop"` | Add SSH key with title | false |
| `gh ssh-key add <file> --type signing` | Add as signing key | false |
| `gh ssh-key list` | List SSH keys | true |
| `gh ssh-key list --json key,title` | List as JSON | true |
| `gh ssh-key delete <id>` | Delete an SSH key | false |
| `gh gpg-key add <file>` | Add a GPG key | false |
| `gh gpg-key list` | List GPG keys | true |
| `gh gpg-key delete <id>` | Delete a GPG key | false |

## Rulesets

| Command | Description | is_pure_description |
|---|---|---|
| `gh ruleset list` | List repo rulesets | true |
| `gh ruleset list --org my-org` | List org rulesets | true |
| `gh ruleset list --json name,enforcement` | List as JSON | true |
| `gh ruleset view <id>` | View ruleset details | true |
| `gh ruleset view <id> --web` | Open in browser | true |
| `gh ruleset check` | Check rules for current branch | true |
| `gh ruleset check --branch main` | Check rules for specific branch | true |

## Attestations

| Command | Description | is_pure_description |
|---|---|---|
| `gh attestation verify <artifact>` | Verify artifact attestation | true |
| `gh attestation verify <artifact> --owner <org>` | Verify with specific owner | true |
| `gh attestation download <artifact>` | Download attestation bundle | true |

## Searching

| Command | Description | is_pure_description |
|---|---|---|
| `gh search repos <query>` | Search repositories | true |
| `gh search repos <query> --language python` | Filter by language | true |
| `gh search repos <query> --topic cli` | Filter by topic | true |
| `gh search repos <query> --stars ">1000"` | Filter by stars | true |
| `gh search repos <query> --sort stars` | Sort by stars | true |
| `gh search repos <query> --owner <user>` | Filter by owner | true |
| `gh search repos <query> --json fullName,url` | Output as JSON | true |
| `gh search repos <query> --limit 50` | Increase result limit | true |
| `gh search issues <query>` | Search issues | true |
| `gh search issues <query> --repo owner/repo` | Scope to repo | true |
| `gh search issues <query> --state open` | Filter by state | true |
| `gh search issues <query> --label "bug"` | Filter by label | true |
| `gh search issues <query> --assignee @me` | Assigned to you | true |
| `gh search issues <query> --sort created` | Sort by created date | true |
| `gh search prs <query>` | Search pull requests | true |
| `gh search prs <query> --state merged` | Filter by state | true |
| `gh search prs <query> --review approved` | Filter by review status | true |
| `gh search prs <query> --merged-at ">2024-01-01"` | Filter by merge date | true |
| `gh search commits <query>` | Search commits | true |
| `gh search commits <query> --repo owner/repo` | Scope to repo | true |
| `gh search commits <query> --author user` | Filter by author | true |
| `gh search code <query>` | Search code | true |
| `gh search code <query> --repo owner/repo` | Scope to repo | true |
| `gh search code <query> --language go` | Filter by language | true |
| `gh search code <query> --filename "*.yml"` | Filter by filename | true |

## Browsing & Status

| Command | Description | is_pure_description |
|---|---|---|
| `gh browse` | Open current repo in browser | true |
| `gh browse <file>` | Open specific file in browser | true |
| `gh browse <file>:<line>` | Open file at specific line | true |
| `gh browse --branch main` | Open specific branch | true |
| `gh browse --settings` | Open repo settings | true |
| `gh browse --wiki` | Open repo wiki | true |
| `gh browse --projects` | Open repo projects | true |
| `gh status` | Show cross-repo dashboard (PRs, issues, notifications) | true |
| `gh status --org my-org` | Status for specific org | true |
| `gh status --exclude owner/repo` | Exclude a repo | true |

## Organizations

| Command | Description | is_pure_description |
|---|---|---|
| `gh org list` | List orgs you belong to | true |
| `gh org list --limit 50` | Increase result limit | true |

## Extensions

| Command | Description | is_pure_description |
|---|---|---|
| `gh extension install owner/gh-ext` | Install an extension | false |
| `gh extension install --pin v1.0.0 owner/gh-ext` | Install pinned version | false |
| `gh extension list` | List installed extensions | true |
| `gh extension upgrade <ext>` | Upgrade an extension | false |
| `gh extension upgrade --all` | Upgrade all extensions | false |
| `gh extension remove <ext>` | Remove an extension | false |
| `gh extension search <query>` | Search available extensions | true |
| `gh extension browse` | Browse extensions interactively | true |
| `gh extension create <name>` | Scaffold a new extension | false |
| `gh extension exec <ext> -- <args>` | Run extension with args | false |

## Aliases

| Command | Description | is_pure_description |
|---|---|---|
| `gh alias set co 'pr checkout'` | Create an alias | false |
| `gh alias set bugs 'issue list --label bug'` | Complex alias | false |
| `gh alias set --shell igrep 'gh issue list \| grep $1'` | Shell alias with args | false |
| `gh alias list` | List all aliases | true |
| `gh alias delete <alias>` | Delete an alias | false |
| `gh alias import aliases.yml` | Import aliases from file | false |

## API (Direct)

| Command | Description | is_pure_description |
|---|---|---|
| `gh api <endpoint>` | GET any REST API endpoint | true |
| `gh api <endpoint> --method POST` | POST request | false |
| `gh api <endpoint> --method PUT` | PUT request | false |
| `gh api <endpoint> --method PATCH` | PATCH request | false |
| `gh api <endpoint> --method DELETE` | DELETE request | false |
| `gh api <endpoint> -f key=value` | Send form field | false |
| `gh api <endpoint> --input data.json` | Send JSON body from file | false |
| `gh api <endpoint> --raw-field body="text"` | Send raw field (no JSON parse) | false |
| `gh api <endpoint> --paginate` | Auto-paginate results | true |
| `gh api <endpoint> --paginate --jq '.[].name'` | Paginate + filter | true |
| `gh api <endpoint> -H "Accept: application/vnd.github+json"` | Custom header | true |
| `gh api <endpoint> --hostname github.example.com` | Target GHE instance | true |
| `gh api <endpoint> --cache 1h` | Cache response | true |
| `gh api <endpoint> --silent` | Suppress output | true |
| `gh api /user` | Get authenticated user info | true |
| `gh api /user/repos --paginate` | List all your repos (paginated) | true |
| `gh api repos/{owner}/{repo}/topics` | Get repo topics | true |
| `gh api graphql -f query='{ viewer { login } }'` | GraphQL query | true |
| `gh api graphql -f query='...' -F count=10` | GraphQL with variables | true |
| `gh api graphql --paginate -f query='...'` | Paginated GraphQL | true |

## Completions & Misc

| Command | Description | is_pure_description |
|---|---|---|
| `gh completion -s bash` | Generate bash completions | true |
| `gh completion -s zsh` | Generate zsh completions | true |
| `gh completion -s fish` | Generate fish completions | true |
| `gh completion -s powershell` | Generate PowerShell completions | true |
| `gh help` | Show top-level help | true |
| `gh help <command>` | Show help for a command | true |
| `gh <command> --help` | Same as above | true |
| `gh version` | Show gh version | true |

## Useful Flags (Global)

| Flag | Description | is_pure_description |
|---|---|---|
| `--json <fields>` | Output as JSON | true |
| `--jq <expression>` | Filter JSON output (jq syntax) | true |
| `--template <string>` | Format output with Go template | true |
| `--web` / `-w` | Open in browser | true |
| `--help` / `-h` | Show help for any command | true |
| `-R owner/repo` | Target a specific repo | true |
| `--hostname <host>` | Target GitHub Enterprise instance | true |

## Quick Recipes

```bash
# Clone, branch, commit, PR in one flow
gh repo clone owner/repo && cd repo
git checkout -b my-feature
# ... make changes ...
git add . && git commit -m "feat: my change"
gh pr create --fill

# List your open PRs across all repos
gh search prs --author=@me --state=open

# Watch CI and auto-merge when green
gh pr merge --auto --squash

# Download latest release asset
gh release download --repo owner/repo --pattern '*.tar.gz'

# Quickly check CI status of current branch
gh pr checks

# Sync fork with upstream
gh repo sync

# Set secrets from .env file
while IFS='=' read -r key value; do
  echo "$value" | gh secret set "$key"
done < .env

# Bulk-close stale issues
gh issue list --label "stale" --json number --jq '.[].number' | \
  xargs -I{} gh issue close {} --comment "Closing stale issue"

# Export all issues to JSON
gh issue list --state all --limit 1000 --json number,title,state,labels,assignees > issues.json

# Find your most-starred repos
gh repo list --json name,stargazerCount --jq 'sort_by(.stargazerCount) | reverse | .[:10]'

# Create release from latest tag with auto-generated notes
LATEST_TAG=$(git describe --tags --abbrev=0)
gh release create "$LATEST_TAG" --generate-notes

# List all open PRs needing your review
gh search prs --review-requested=@me --state=open

# Delete all workflow runs for a specific workflow
gh run list --workflow "CI" --json databaseId --jq '.[].databaseId' | \
  xargs -I{} gh run delete {}

# Monitor a deploy: trigger workflow then watch
gh workflow run deploy.yml -f env=production
sleep 5
gh run list --workflow deploy.yml --limit 1 --json databaseId --jq '.[0].databaseId' | \
  xargs gh run watch

# Copy labels from one repo to another
gh label clone source-owner/source-repo -R target-owner/target-repo

# Compare two releases
gh api repos/{owner}/{repo}/compare/v1.0.0...v2.0.0 --jq '.commits[].commit.message'
```