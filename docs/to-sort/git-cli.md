# Git CLI — Near-Exhaustive Cheat Sheet

## Cache Invalidation Matrix

When a **mutating** git command (row) is executed, which **read-only** GitResult panels (columns) should be invalidated?

P6 (GitPanel) is **NOT** included — the `.git/` filesystem watcher handles it.

| Mutating cmd ↓ \ RO panel → | `git log` | `git diff` | `git show` | `git status` | `git branch` | `git stash list` | `git stash show` | `git tag` | `git config` | `git remote` | `git blame` | `git shortlog` | `git rev-list` | `git rev-parse` | `git ls-files` | `git ls-tree` | `git for-each-ref` | `git grep` | `git describe` | `git reflog` | `git cat-file` | `git format-patch` |
|-----|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **`git commit`** | ✓ | ✓ | ✓ | ✓ | | | | | | | ✓ | ✓ | ✓ | ✓ | | ✓ | ✓ | | ✓ | ✓ | ✓ | ✓ |
| **`git add`** | | ✓ | | ✓ | | | | | | | | | | | ✓ | | | | | | | |
| **`git restore`** | | ✓ | | ✓ | | | | | | | | | | | ✓ | | | ✓ | | | | |
| **`git rm`** | | ✓ | | ✓ | | | | | | | | | | | ✓ | | | ✓ | | | | |
| **`git mv`** | | ✓ | | ✓ | | | | | | | ✓ | | | | ✓ | | | ✓ | | | | |
| **`git checkout`** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **`git switch`** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **`git merge`** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **`git rebase`** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **`git cherry-pick`** | ✓ | ✓ | ✓ | ✓ | | | | | | | ✓ | ✓ | ✓ | ✓ | | ✓ | ✓ | | ✓ | ✓ | ✓ | ✓ |
| **`git reset`** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **`git revert`** | ✓ | ✓ | ✓ | ✓ | | | | | | | ✓ | ✓ | ✓ | ✓ | | ✓ | ✓ | | ✓ | ✓ | ✓ | ✓ |
| **`git push`** | ✓ | | | | | | | | | | | | | | | | | | | | | |
| **`git pull`** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **`git fetch`** | ✓ | | | | ✓ | | | ✓ | | | | | | | | | ✓ | | | | | |
| **`git stash push`** | | ✓ | | ✓ | | ✓ | ✓ | | | | | | | | ✓ | | | ✓ | | | | |
| **`git stash pop`** | | ✓ | | ✓ | | ✓ | ✓ | | | | | | | | ✓ | | | ✓ | | | | |
| **`git stash drop`** | | | | | | ✓ | ✓ | | | | | | | | | | | | | | | |
| **`git stash clear`** | | | | | | ✓ | ✓ | | | | | | | | | | | | | | | |
| **`git branch -d/-D`** | | | | | ✓ | | | | | | | | | | | | ✓ | | | | | |
| **`git branch <new>`** | | | | | ✓ | | | | | | | | | | | | ✓ | | | | | |
| **`git branch -m`** | | | | | ✓ | | | | | | | | | | | | ✓ | | | ✓ | | |
| **`git tag <name>`** | | | | | | | | ✓ | | | | | | | | | ✓ | | ✓ | | | |
| **`git tag -d`** | | | | | | | | ✓ | | | | | | | | | ✓ | | ✓ | | | |
| **`git config set`** | | | | | | | | | ✓ | | | | | | | | | | | | | |
| **`git remote add/rm`** | | | | | | | | | | ✓ | | | | | | | | | | | | |
| **`git remote set-url`** | | | | | | | | | | ✓ | | | | | | | | | | | | |
| **`git clean`** | | ✓ | | ✓ | | | | | | | | | | | ✓ | | | ✓ | | | | |
| **`git am`** | ✓ | ✓ | ✓ | ✓ | | | | | | | ✓ | ✓ | ✓ | ✓ | | ✓ | ✓ | | ✓ | ✓ | ✓ | ✓ |

### Simplification: Group by invalidation pattern

To avoid 30+ individual rules, commands can be grouped by their invalidation pattern:

1. **NUCLEAR** (invalidate ALL GitResult panels): `checkout`, `switch`, `merge`, `rebase`, `reset`, `pull` — these change HEAD/branch or rewrite history
2. **COMMIT-LIKE** (invalidate log, diff, show, status, blame, shortlog, rev-list, rev-parse, ls-tree, for-each-ref, describe, reflog, cat-file, format-patch): `commit`, `cherry-pick`, `revert`, `am`
3. **STAGING** (invalidate diff, status, ls-files, grep): `add`, `restore`, `rm`, `mv`, `clean`, `stash push`, `stash pop`
4. **STASH-ONLY** (invalidate stash list/show): `stash drop`, `stash clear`
5. **PUSH** (invalidate log only): `push`
6. **FETCH** (invalidate log, branch, tag, for-each-ref): `fetch`
7. **BRANCH-MGMT** (invalidate branch, for-each-ref): `branch -d/-D/<new>/-m/-c`
8. **TAG-MGMT** (invalidate tag, for-each-ref, describe): `tag <create>`, `tag -d`
9. **CONFIG** (invalidate config): `config <set>`
10. **REMOTE** (invalidate remote): `remote add/rm/set-url/rename`
11. **UNKNOWN** → blanket invalidation (safe default)

## Setup & Configuration

| Command | Description | is_pure_description |
|---|---|---|
| `git config --global user.name "Name"` | Set global username | false |
| `git config --global user.email "email"` | Set global email | false |
| `git config --local user.name "Name"` | Set repo-level username | false |
| `git config --global core.editor vim` | Set default editor | false |
| `git config --global core.autocrlf true` | Auto-convert line endings (Windows) | false |
| `git config --global core.autocrlf input` | Convert CRLF→LF on commit (Mac/Linux) | false |
| `git config --global init.defaultBranch main` | Set default branch name | false |
| `git config --global pull.rebase true` | Rebase on pull by default | false |
| `git config --global push.default current` | Push current branch by default | false |
| `git config --global push.autoSetupRemote true` | Auto set upstream on push | false |
| `git config --global merge.conflictstyle diff3` | 3-way conflict markers | false |
| `git config --global rerere.enabled true` | Remember conflict resolutions | false |
| `git config --global fetch.prune true` | Auto-prune on fetch | false |
| `git config --global diff.algorithm histogram` | Use histogram diff algorithm | false |
| `git config --global alias.co checkout` | Create an alias | false |
| `git config --global alias.st "status -sb"` | Create complex alias | false |
| `git config --global credential.helper cache` | Cache credentials in memory | false |
| `git config --global credential.helper store` | Store credentials on disk (plaintext) | false |
| `git config --global credential.helper osxkeychain` | macOS keychain | false |
| `git config --global http.proxy http://proxy:8080` | Set HTTP proxy | false |
| `git config --global core.excludesfile ~/.gitignore_global` | Set global gitignore | false |
| `git config --global core.pager "less -F -X"` | Set pager | false |
| `git config --global color.ui auto` | Enable colored output | false |
| `git config --global log.date iso` | Set default date format | false |
| `git config --global commit.gpgsign true` | Sign all commits with GPG | false |
| `git config --global tag.gpgsign true` | Sign all tags with GPG | false |
| `git config --global gpg.format ssh` | Use SSH for signing | false |
| `git config --global user.signingkey ~/.ssh/id_ed25519.pub` | Set signing key | false |
| `git config --list` | List all config values | true |
| `git config --list --show-origin` | List config with file locations | true |
| `git config --list --show-scope` | List config with scope (system/global/local) | true |
| `git config --global --edit` | Open global config in editor | false |
| `git config --get user.name` | Get a specific config value | true |
| `git config --get-regexp alias` | List all aliases | true |
| `git config --unset user.name` | Remove a config key | false |
| `git config --remove-section alias` | Remove entire config section | false |

## Creating & Cloning Repositories

| Command | Description | is_pure_description |
|---|---|---|
| `git init` | Initialize a new repo in current directory | false |
| `git init <dir>` | Initialize a new repo in specified directory | false |
| `git init --bare` | Initialize a bare repository | false |
| `git init --initial-branch main` | Init with specific default branch | false |
| `git init --template <dir>` | Init with template directory | false |
| `git clone <url>` | Clone a remote repository | false |
| `git clone <url> <dir>` | Clone into specific directory | false |
| `git clone --depth 1 <url>` | Shallow clone (latest commit only) | false |
| `git clone --depth 10 <url>` | Shallow clone (last 10 commits) | false |
| `git clone --shallow-since="2024-01-01" <url>` | Shallow clone since date | false |
| `git clone --single-branch <url>` | Clone only default branch | false |
| `git clone --branch <branch> <url>` | Clone specific branch | false |
| `git clone --no-checkout <url>` | Clone without checking out files | false |
| `git clone --bare <url>` | Clone as bare repository | false |
| `git clone --mirror <url>` | Mirror clone (all refs) | false |
| `git clone --recurse-submodules <url>` | Clone with submodules | false |
| `git clone --shallow-submodules <url>` | Clone with shallow submodules | false |
| `git clone --filter=blob:none <url>` | Blobless (partial) clone | false |
| `git clone --filter=tree:0 <url>` | Treeless (partial) clone | false |
| `git clone --sparse <url>` | Clone for sparse-checkout | false |
| `git clone --jobs 4 <url>` | Parallel submodule clone | false |
| `git clone --origin upstream <url>` | Clone with custom remote name | false |

## Staging & Snapshotting

| Command | Description | is_pure_description |
|---|---|---|
| `git status` | Show working tree status | true |
| `git status -s` | Short format status | true |
| `git status -sb` | Short format with branch info | true |
| `git status --ignored` | Show ignored files | true |
| `git status --porcelain` | Machine-readable status | true |
| `git status --untracked-files=all` | Show all untracked files (including in subdirs) | true |
| `git add <file>` | Stage a specific file | false |
| `git add <dir>` | Stage all changes in directory | false |
| `git add .` | Stage all changes in current directory | false |
| `git add -A` | Stage all changes (entire repo) | false |
| `git add -p` | Interactively stage hunks | false |
| `git add -i` | Interactive staging mode | false |
| `git add -u` | Stage modified/deleted files (not new) | false |
| `git add -N <file>` | Mark file as intent-to-add | false |
| `git add --chmod=+x <file>` | Stage and mark as executable | false |
| `git add *.py` | Stage by glob pattern | false |
| `git rm <file>` | Remove file from working tree and index | false |
| `git rm --cached <file>` | Unstage file (keep on disk) | false |
| `git rm -r --cached <dir>` | Recursively unstage directory | false |
| `git rm -f <file>` | Force remove modified file | false |
| `git mv <old> <new>` | Move/rename a file | false |
| `git restore <file>` | Discard working tree changes | false |
| `git restore .` | Discard all working tree changes | false |
| `git restore --source <commit> <file>` | Restore file from specific commit | false |
| `git restore --staged <file>` | Unstage a file (keep changes) | false |
| `git restore --staged --worktree <file>` | Unstage and discard changes | false |
| `git restore -p` | Interactively restore hunks | false |
| `git clean -n` | Dry-run: show what would be removed | true |
| `git clean -f` | Remove untracked files | false |
| `git clean -fd` | Remove untracked files and directories | false |
| `git clean -fX` | Remove only ignored files | false |
| `git clean -fx` | Remove all untracked + ignored files | false |
| `git clean -fi` | Interactive clean | false |

## Committing

| Command | Description | is_pure_description |
|---|---|---|
| `git commit` | Commit staged changes (opens editor) | false |
| `git commit -m "message"` | Commit with message | false |
| `git commit -m "title" -m "body"` | Commit with title and body | false |
| `git commit -a` | Stage all tracked changes and commit | false |
| `git commit -am "message"` | Stage all + commit with message | false |
| `git commit --amend` | Amend last commit (opens editor) | false |
| `git commit --amend -m "new msg"` | Amend with new message | false |
| `git commit --amend --no-edit` | Amend without changing message | false |
| `git commit --amend --author="Name <email>"` | Change author of last commit | false |
| `git commit --amend --date="..."` | Change date of last commit | false |
| `git commit --amend --reset-author` | Reset author to current config | false |
| `git commit --allow-empty` | Create commit with no changes | false |
| `git commit --allow-empty-message` | Commit with empty message | false |
| `git commit --fixup <commit>` | Create fixup commit for later autosquash | false |
| `git commit --squash <commit>` | Create squash commit for later autosquash | false |
| `git commit -S` | Sign commit with GPG | false |
| `git commit --signoff` | Add Signed-off-by line | false |
| `git commit -v` | Show diff in commit editor | false |
| `git commit --dry-run` | Show what would be committed | true |
| `git commit --no-verify` | Skip pre-commit and commit-msg hooks | false |
| `git commit -p` | Interactively select hunks to commit | false |
| `git commit --trailer "key: value"` | Add trailer to commit message | false |
| `git commit -C <commit>` | Reuse message from another commit | false |
| `git commit --date="2024-01-01T12:00:00"` | Set author date | false |

## Branching

| Command | Description | is_pure_description |
|---|---|---|
| `git branch` | List local branches | true |
| `git branch -r` | List remote branches | true |
| `git branch -a` | List all branches (local + remote) | true |
| `git branch -v` | List branches with last commit | true |
| `git branch -vv` | List branches with tracking info | true |
| `git branch --merged` | List branches merged into current | true |
| `git branch --no-merged` | List unmerged branches | true |
| `git branch --merged main` | Branches merged into main | true |
| `git branch --contains <commit>` | Branches containing commit | true |
| `git branch --no-contains <commit>` | Branches not containing commit | true |
| `git branch --sort=-committerdate` | Sort by most recent commit | true |
| `git branch --format '%(refname:short) %(upstream:short)'` | Custom format | true |
| `git branch --show-current` | Print current branch name | true |
| `git branch <name>` | Create a branch | false |
| `git branch <name> <start>` | Create branch from specific point | false |
| `git branch -d <name>` | Delete branch (safe) | false |
| `git branch -D <name>` | Force delete branch | false |
| `git branch -m <old> <new>` | Rename a branch | false |
| `git branch -M <old> <new>` | Force rename a branch | false |
| `git branch -m <new>` | Rename current branch | false |
| `git branch -c <old> <new>` | Copy a branch | false |
| `git branch -u origin/main` | Set upstream for current branch | false |
| `git branch --unset-upstream` | Remove upstream tracking | false |
| `git branch -u origin/main <branch>` | Set upstream for specific branch | false |
| `git switch <branch>` | Switch to a branch | false |
| `git switch -c <branch>` | Create and switch to new branch | false |
| `git switch -c <branch> origin/main` | Create branch from remote | false |
| `git switch -C <branch>` | Force create/reset and switch | false |
| `git switch -` | Switch to previous branch | false |
| `git switch --detach <commit>` | Switch to detached HEAD | false |
| `git switch --orphan <branch>` | Create orphan branch (no history) | false |
| `git checkout <branch>` | Switch to a branch (legacy) | false |
| `git checkout -b <branch>` | Create and switch (legacy) | false |
| `git checkout -b <branch> origin/main` | Create from remote (legacy) | false |
| `git checkout -B <branch>` | Force create/reset and switch (legacy) | false |
| `git checkout --orphan <branch>` | Create orphan branch (legacy) | false |
| `git checkout -` | Switch to previous branch (legacy) | false |
| `git checkout -- <file>` | Restore file (legacy, use `restore`) | false |

## Merging

| Command | Description | is_pure_description |
|---|---|---|
| `git merge <branch>` | Merge branch into current | false |
| `git merge --no-ff <branch>` | Merge with merge commit (no fast-forward) | false |
| `git merge --ff-only <branch>` | Merge only if fast-forward possible | false |
| `git merge --squash <branch>` | Squash merge (stage only, no commit) | false |
| `git merge --strategy=ours <branch>` | Merge keeping our changes | false |
| `git merge -X theirs <branch>` | Auto-resolve conflicts with theirs | false |
| `git merge -X ours <branch>` | Auto-resolve conflicts with ours | false |
| `git merge --no-commit <branch>` | Merge but don't auto-commit | false |
| `git merge --edit` | Edit merge commit message | false |
| `git merge --signoff <branch>` | Merge with Signed-off-by | false |
| `git merge --abort` | Abort an in-progress merge | false |
| `git merge --continue` | Continue after resolving conflicts | false |
| `git merge --allow-unrelated-histories <branch>` | Merge repos with no common ancestor | false |
| `git mergetool` | Open merge conflict resolution tool | false |
| `git mergetool --tool=vimdiff` | Use specific merge tool | false |

## Rebasing

| Command | Description | is_pure_description |
|---|---|---|
| `git rebase <branch>` | Rebase current branch onto branch | false |
| `git rebase main` | Rebase onto main | false |
| `git rebase origin/main` | Rebase onto remote main | false |
| `git rebase -i HEAD~3` | Interactive rebase last 3 commits | false |
| `git rebase -i --root` | Interactive rebase entire history | false |
| `git rebase -i --autosquash HEAD~5` | Auto-arrange fixup/squash commits | false |
| `git rebase --onto main A B` | Rebase range A..B onto main | false |
| `git rebase --onto main HEAD~3` | Move last 3 commits onto main | false |
| `git rebase --keep-base -i HEAD~3` | Rebase in-place (no reroot) | false |
| `git rebase -X theirs <branch>` | Auto-resolve conflicts with theirs | false |
| `git rebase -X ours <branch>` | Auto-resolve conflicts with ours | false |
| `git rebase --exec "make test" main` | Run command after each commit | false |
| `git rebase --committer-date-is-author-date <b>` | Preserve original dates | false |
| `git rebase --abort` | Abort an in-progress rebase | false |
| `git rebase --continue` | Continue after resolving conflict | false |
| `git rebase --skip` | Skip current commit and continue | false |
| `git rebase --edit-todo` | Edit the rebase todo list mid-rebase | false |
| `git rebase --update-refs` | Auto-update stacked branches | false |

## Cherry-Picking

| Command | Description | is_pure_description |
|---|---|---|
| `git cherry-pick <commit>` | Apply a specific commit | false |
| `git cherry-pick <c1> <c2> <c3>` | Apply multiple commits | false |
| `git cherry-pick <c1>..<c2>` | Apply range of commits (exclusive start) | false |
| `git cherry-pick <c1>^..<c2>` | Apply range of commits (inclusive) | false |
| `git cherry-pick --no-commit <commit>` | Apply without committing | false |
| `git cherry-pick -x <commit>` | Append "(cherry picked from ...)" | false |
| `git cherry-pick -m 1 <merge-commit>` | Cherry-pick a merge commit | false |
| `git cherry-pick --edit <commit>` | Edit message before committing | false |
| `git cherry-pick --signoff <commit>` | Add Signed-off-by | false |
| `git cherry-pick --abort` | Abort in-progress cherry-pick | false |
| `git cherry-pick --continue` | Continue after resolving conflict | false |
| `git cherry-pick --skip` | Skip current commit | false |

## Reverting

| Command | Description | is_pure_description |
|---|---|---|
| `git revert <commit>` | Create a commit that undoes a commit | false |
| `git revert <c1> <c2>` | Revert multiple commits | false |
| `git revert <c1>..<c2>` | Revert a range | false |
| `git revert --no-commit <commit>` | Revert without committing | false |
| `git revert --no-edit <commit>` | Revert without editing message | false |
| `git revert -m 1 <merge-commit>` | Revert a merge commit | false |
| `git revert --abort` | Abort in-progress revert | false |
| `git revert --continue` | Continue after resolving conflict | false |

## Resetting

| Command | Description | is_pure_description |
|---|---|---|
| `git reset HEAD <file>` | Unstage a file | false |
| `git reset` | Unstage all files | false |
| `git reset --soft HEAD~1` | Undo last commit, keep staged | false |
| `git reset --mixed HEAD~1` | Undo last commit, keep unstaged (default) | false |
| `git reset --hard HEAD~1` | Undo last commit, discard all changes | false |
| `git reset --soft <commit>` | Move HEAD, keep all changes staged | false |
| `git reset --mixed <commit>` | Move HEAD, keep changes unstaged | false |
| `git reset --hard <commit>` | Move HEAD, discard everything | false |
| `git reset --hard origin/main` | Reset to match remote exactly | false |
| `git reset --hard ORIG_HEAD` | Undo last reset/merge/rebase | false |
| `git reset --merge` | Reset a failed merge | false |
| `git reset --keep <commit>` | Reset but keep local uncommitted changes | false |
| `git reset -p` | Interactively unstage hunks | false |

## Stashing

| Command | Description | is_pure_description |
|---|---|---|
| `git stash` | Stash working tree changes | false |
| `git stash push` | Same as `git stash` | false |
| `git stash push -m "description"` | Stash with description | false |
| `git stash push <file1> <file2>` | Stash specific files | false |
| `git stash push -p` | Interactively select hunks to stash | false |
| `git stash push --keep-index` | Stash but keep staged changes | false |
| `git stash push --include-untracked` / `-u` | Include untracked files | false |
| `git stash push --all` / `-a` | Include untracked + ignored files | false |
| `git stash push --staged` | Stash only staged changes | false |
| `git stash list` | List all stashes | true |
| `git stash show` | Show stash diff summary | true |
| `git stash show -p` | Show stash diff (full patch) | true |
| `git stash show stash@{2}` | Show specific stash | true |
| `git stash show stash@{2} -p` | Show specific stash (full patch) | true |
| `git stash pop` | Apply latest stash and remove it | false |
| `git stash pop stash@{2}` | Apply specific stash and remove it | false |
| `git stash apply` | Apply latest stash, keep in list | false |
| `git stash apply stash@{2}` | Apply specific stash, keep in list | false |
| `git stash branch <branch>` | Create branch from stash | false |
| `git stash branch <branch> stash@{2}` | Create branch from specific stash | false |
| `git stash drop` | Remove latest stash | false |
| `git stash drop stash@{2}` | Remove specific stash | false |
| `git stash clear` | Remove all stashes | false |
| `git stash create` | Create stash entry without storing | false |
| `git stash store <commit>` | Store a stash entry manually | false |

## Tagging

| Command | Description | is_pure_description |
|---|---|---|
| `git tag` | List all tags | true |
| `git tag -l "v1.*"` | List tags matching pattern | true |
| `git tag -l --sort=-version:refname` | List tags sorted by version | true |
| `git tag -l --sort=-creatordate` | List tags sorted by date | true |
| `git tag -n` | List tags with first line of annotation | true |
| `git tag -n5` | List tags with 5 lines of annotation | true |
| `git tag --contains <commit>` | Tags containing a commit | true |
| `git tag --no-contains <commit>` | Tags not containing a commit | true |
| `git tag --points-at HEAD` | Tags pointing at HEAD | true |
| `git tag <name>` | Create lightweight tag | false |
| `git tag <name> <commit>` | Tag a specific commit | false |
| `git tag -a <name> -m "message"` | Create annotated tag | false |
| `git tag -a <name> <commit> -m "msg"` | Annotated tag on specific commit | false |
| `git tag -s <name> -m "message"` | Create GPG-signed tag | false |
| `git tag -f <name>` | Force-update a tag | false |
| `git tag -d <name>` | Delete a local tag | false |
| `git tag -v <name>` | Verify a signed tag | true |
| `git push origin <tag>` | Push a tag to remote | false |
| `git push origin --tags` | Push all tags to remote | false |
| `git push origin --follow-tags` | Push commits + annotated tags | false |
| `git push origin :refs/tags/<tag>` | Delete a remote tag | false |
| `git push origin --delete <tag>` | Delete a remote tag (alternative) | false |

## Viewing History & Logs

| Command | Description | is_pure_description |
|---|---|---|
| `git log` | Show commit history | true |
| `git log --oneline` | Compact one-line log | true |
| `git log --oneline --graph` | Log with ASCII graph | true |
| `git log --oneline --graph --all` | Graph of all branches | true |
| `git log --oneline --graph --decorate` | Graph with branch/tag names | true |
| `git log -n 10` | Last 10 commits | true |
| `git log -p` | Log with diffs (patches) | true |
| `git log --stat` | Log with file change stats | true |
| `git log --shortstat` | Log with summary stats only | true |
| `git log --name-only` | Log with changed filenames | true |
| `git log --name-status` | Log with filenames + status (A/M/D) | true |
| `git log --diff-filter=D` | Commits that deleted files | true |
| `git log --follow <file>` | Log for a file (follows renames) | true |
| `git log -- <file>` | Log for a specific file | true |
| `git log -- <dir>` | Log for a specific directory | true |
| `git log -S "string"` | Commits that add/remove string (pickaxe) | true |
| `git log -G "regex"` | Commits matching regex in diff | true |
| `git log --grep="pattern"` | Search commit messages | true |
| `git log --grep="fix" --grep="bug" --all-match` | Messages matching all patterns | true |
| `git log --author="name"` | Filter by author | true |
| `git log --committer="name"` | Filter by committer | true |
| `git log --after="2024-01-01"` | Commits after date | true |
| `git log --before="2024-12-31"` | Commits before date | true |
| `git log --since="2 weeks ago"` | Relative date filter | true |
| `git log --until="yesterday"` | Relative date filter | true |
| `git log --merges` | Only merge commits | true |
| `git log --no-merges` | Exclude merge commits | true |
| `git log --first-parent` | Follow only first parent (mainline) | true |
| `git log --ancestry-path <c1>..<c2>` | Show direct path between commits | true |
| `git log main..feature` | Commits in feature not in main | true |
| `git log main...feature` | Commits in either but not both | true |
| `git log --left-right main...feature` | Show which side each commit is on | true |
| `git log --format="%H %an %s"` | Custom format | true |
| `git log --format="%h %ad %s" --date=short` | Short hash + short date | true |
| `git log --pretty=fuller` | Full detail (author + committer) | true |
| `git log --pretty=raw` | Raw commit objects | true |
| `git log --abbrev-commit` | Short commit hashes | true |
| `git log --reverse` | Oldest first | true |
| `git log --topo-order` | Topological order | true |
| `git log --date-order` | Strict date order | true |
| `git log --simplify-by-decoration` | Only tagged/branched commits | true |
| `git log --all --source` | Show which ref led to each commit | true |
| `git log --walk-reflogs` | Show reflog entries | true |

## Viewing & Comparing

| Command | Description | is_pure_description |
|---|---|---|
| `git show <commit>` | Show commit details + diff | true |
| `git show <commit>:<file>` | Show file at specific commit | true |
| `git show <tag>` | Show tag info + commit | true |
| `git show --stat <commit>` | Show commit with file stats | true |
| `git show --name-only <commit>` | Show only changed filenames | true |
| `git show --format="%H" --no-patch <commit>` | Show only commit hash | true |
| `git diff` | Working tree vs index (unstaged) | true |
| `git diff --staged` / `--cached` | Index vs HEAD (staged) | true |
| `git diff HEAD` | Working tree vs HEAD | true |
| `git diff <commit>` | Working tree vs specific commit | true |
| `git diff <c1> <c2>` | Diff between two commits | true |
| `git diff <c1>..<c2>` | Same as above | true |
| `git diff <c1>...<c2>` | Changes since common ancestor | true |
| `git diff main feature` | Diff between branches | true |
| `git diff -- <file>` | Diff for specific file | true |
| `git diff --stat` | Summary stats only | true |
| `git diff --shortstat` | Single-line summary | true |
| `git diff --name-only` | Changed filenames only | true |
| `git diff --name-status` | Filenames with status | true |
| `git diff --word-diff` | Word-level diff | true |
| `git diff --color-words` | Colored word-level diff | true |
| `git diff --no-index <f1> <f2>` | Diff two files outside git | true |
| `git diff --diff-filter=M` | Only modified files | true |
| `git diff --diff-filter=A` | Only added files | true |
| `git diff --diff-filter=D` | Only deleted files | true |
| `git diff -U5` | Show 5 lines of context | true |
| `git diff --check` | Check for whitespace errors | true |
| `git diff --patience` | Patience diff algorithm | true |
| `git diff --histogram` | Histogram diff algorithm | true |
| `git diff --minimal` | Minimal diff | true |
| `git diff --binary` | Include binary file diffs | true |
| `git diff --ignore-space-change` / `-b` | Ignore whitespace amount changes | true |
| `git diff --ignore-all-space` / `-w` | Ignore all whitespace | true |
| `git diff --ignore-blank-lines` | Ignore blank line changes | true |
| `git diff --output=diff.patch` | Write diff to file | true |
| `git difftool` | Open diff in external tool | true |
| `git difftool --tool=vimdiff` | Use specific diff tool | true |
| `git difftool --dir-diff` | Directory-level diff | true |

## Blame & Annotation

| Command | Description | is_pure_description |
|---|---|---|
| `git blame <file>` | Show line-by-line authorship | true |
| `git blame -L 10,20 <file>` | Blame specific line range | true |
| `git blame -L :funcname <file>` | Blame a function | true |
| `git blame -w <file>` | Ignore whitespace | true |
| `git blame -M <file>` | Detect lines moved within file | true |
| `git blame -C <file>` | Detect lines moved from other files | true |
| `git blame -C -C <file>` | More aggressive cross-file detection | true |
| `git blame --since="2024-01-01" <file>` | Blame since date | true |
| `git blame <commit> -- <file>` | Blame at specific commit | true |
| `git blame --reverse <c1>..<c2> -- <file>` | Reverse blame (when lines removed) | true |
| `git annotate <file>` | Similar to blame (older format) | true |

## Searching

| Command | Description | is_pure_description |
|---|---|---|
| `git grep "pattern"` | Search working tree | true |
| `git grep -n "pattern"` | Search with line numbers | true |
| `git grep -c "pattern"` | Count matches per file | true |
| `git grep -l "pattern"` | List matching filenames only | true |
| `git grep -L "pattern"` | List non-matching filenames | true |
| `git grep -i "pattern"` | Case-insensitive search | true |
| `git grep -w "pattern"` | Match whole words only | true |
| `git grep -e "p1" --and -e "p2"` | Lines matching both patterns | true |
| `git grep -e "p1" --or -e "p2"` | Lines matching either pattern | true |
| `git grep "pattern" <commit>` | Search in specific commit | true |
| `git grep "pattern" -- "*.py"` | Search only Python files | true |
| `git grep --heading --break "pattern"` | Group results by file | true |
| `git log -S "string"` | Commits adding/removing string | true |
| `git log -G "regex"` | Commits with regex in diff | true |
| `git log --all --grep="pattern"` | Search all branches' messages | true |

## Remote Operations

| Command | Description | is_pure_description |
|---|---|---|
| `git remote` | List remote names | true |
| `git remote -v` | List remotes with URLs | true |
| `git remote show <name>` | Show remote details | true |
| `git remote get-url <name>` | Get remote URL | true |
| `git remote add <name> <url>` | Add a new remote | false |
| `git remote rename <old> <new>` | Rename a remote | false |
| `git remote remove <name>` | Remove a remote | false |
| `git remote set-url <name> <url>` | Change remote URL | false |
| `git remote set-url --add <name> <url>` | Add additional push URL | false |
| `git remote set-url --push <name> <url>` | Set separate push URL | false |
| `git remote set-head <name> <branch>` | Set default branch for remote | false |
| `git remote set-head <name> --auto` | Auto-detect remote HEAD | false |
| `git remote prune <name>` | Remove stale remote-tracking branches | false |
| `git remote update` | Fetch all remotes | false |
| `git fetch` | Fetch from default remote | false |
| `git fetch <remote>` | Fetch from specific remote | false |
| `git fetch --all` | Fetch from all remotes | false |
| `git fetch --prune` | Fetch and remove stale branches | false |
| `git fetch --prune-tags` | Prune local tags not on remote | false |
| `git fetch --tags` | Fetch all tags | false |
| `git fetch --depth 1` | Fetch shallow (1 commit) | false |
| `git fetch --deepen 10` | Deepen shallow clone by 10 | false |
| `git fetch --unshallow` | Convert shallow to full clone | false |
| `git fetch --dry-run` | Show what would be fetched | true |
| `git fetch origin <branch>` | Fetch specific branch | false |
| `git fetch origin +refs/pull/*/head:refs/pull/*` | Fetch all GitHub PR refs | false |
| `git pull` | Fetch and merge | false |
| `git pull --rebase` | Fetch and rebase | false |
| `git pull --rebase=interactive` | Fetch and interactive rebase | false |
| `git pull --ff-only` | Pull only if fast-forward | false |
| `git pull --no-commit` | Pull but don't auto-commit merge | false |
| `git pull --autostash` | Auto-stash before pull | false |
| `git pull origin main` | Pull specific branch from remote | false |
| `git push` | Push current branch to upstream | false |
| `git push <remote> <branch>` | Push branch to remote | false |
| `git push -u origin <branch>` | Push and set upstream | false |
| `git push --all` | Push all branches | false |
| `git push --tags` | Push all tags | false |
| `git push --follow-tags` | Push commits + annotated tags | false |
| `git push --force` / `-f` | Force push (dangerous) | false |
| `git push --force-with-lease` | Safe force push (checks remote) | false |
| `git push --force-with-lease=<branch>` | Safe force push specific branch | false |
| `git push --force-if-includes` | Extra safety for force push | false |
| `git push --delete <remote> <branch>` | Delete remote branch | false |
| `git push origin :<branch>` | Delete remote branch (shorthand) | false |
| `git push --set-upstream origin <branch>` | Set upstream on push | false |
| `git push --dry-run` | Show what would be pushed | true |
| `git push --no-verify` | Skip pre-push hook | false |
| `git push --mirror` | Mirror push all refs | false |
| `git push origin HEAD` | Push current branch by HEAD | false |
| `git push origin main:production` | Push local main to remote production | false |
| `git ls-remote` | List references on remote | true |
| `git ls-remote --heads` | List remote branches | true |
| `git ls-remote --tags` | List remote tags | true |

## Reflog

| Command | Description | is_pure_description |
|---|---|---|
| `git reflog` | Show HEAD reflog | true |
| `git reflog show <branch>` | Show reflog for branch | true |
| `git reflog show --all` | Show all reflogs | true |
| `git reflog show --date=iso` | Reflog with ISO dates | true |
| `git reflog -n 20` | Last 20 reflog entries | true |
| `git reflog expire --expire=30.days --all` | Expire old entries | false |
| `git reflog delete HEAD@{2}` | Delete specific entry | false |

## Bisecting

| Command | Description | is_pure_description |
|---|---|---|
| `git bisect start` | Start bisecting | false |
| `git bisect bad` | Mark current commit as bad | false |
| `git bisect good <commit>` | Mark a known good commit | false |
| `git bisect bad <commit>` | Mark a known bad commit | false |
| `git bisect good` | Mark current as good | false |
| `git bisect skip` | Skip current commit | false |
| `git bisect reset` | End bisect, return to original branch | false |
| `git bisect log` | Show bisect log | true |
| `git bisect replay <logfile>` | Replay a bisect log | false |
| `git bisect visualize` | Visualize remaining suspects | true |
| `git bisect run <script>` | Auto-bisect with test script | false |
| `git bisect run make test` | Auto-bisect with make test | false |
| `git bisect terms --term-bad=broken --term-good=fixed` | Custom terminology | false |
| `git bisect start --first-parent` | Follow first parent only | false |

## Worktrees

| Command | Description | is_pure_description |
|---|---|---|
| `git worktree add <path> <branch>` | Create worktree for branch | false |
| `git worktree add <path>` | Create worktree for new branch | false |
| `git worktree add --detach <path> <commit>` | Create detached worktree | false |
| `git worktree add -b <branch> <path>` | Create new branch in worktree | false |
| `git worktree list` | List all worktrees | true |
| `git worktree list --porcelain` | Machine-readable worktree list | true |
| `git worktree move <worktree> <new-path>` | Move a worktree | false |
| `git worktree remove <worktree>` | Remove a worktree | false |
| `git worktree remove --force <worktree>` | Force remove worktree | false |
| `git worktree lock <worktree>` | Prevent pruning of worktree | false |
| `git worktree unlock <worktree>` | Allow pruning of worktree | false |
| `git worktree prune` | Clean up stale worktree refs | false |
| `git worktree repair` | Repair worktree links | false |

## Submodules

| Command | Description | is_pure_description |
|---|---|---|
| `git submodule add <url>` | Add a submodule | false |
| `git submodule add <url> <path>` | Add submodule at specific path | false |
| `git submodule add -b <branch> <url>` | Add submodule tracking a branch | false |
| `git submodule init` | Initialize submodule config | false |
| `git submodule update` | Checkout submodule commits | false |
| `git submodule update --init` | Init + update | false |
| `git submodule update --init --recursive` | Init + update recursively | false |
| `git submodule update --remote` | Update to latest remote commit | false |
| `git submodule update --remote --merge` | Update and merge | false |
| `git submodule update --remote --rebase` | Update and rebase | false |
| `git submodule status` | Show submodule status | true |
| `git submodule status --recursive` | Recursive submodule status | true |
| `git submodule summary` | Show submodule commit diff | true |
| `git submodule foreach <command>` | Run command in each submodule | false |
| `git submodule foreach --recursive <cmd>` | Run command recursively | false |
| `git submodule sync` | Sync submodule URLs from .gitmodules | false |
| `git submodule sync --recursive` | Sync recursively | false |
| `git submodule set-url <path> <url>` | Change submodule URL | false |
| `git submodule set-branch -b <br> <path>` | Change tracked branch | false |
| `git submodule deinit <path>` | Unregister a submodule | false |
| `git submodule deinit --all` | Unregister all submodules | false |
| `git submodule absorbgitdirs` | Move submodule .git dirs into parent | false |
| `git rm <submodule-path>` | Remove a submodule entirely | false |

## Sparse Checkout

| Command | Description | is_pure_description |
|---|---|---|
| `git sparse-checkout init` | Enable sparse checkout | false |
| `git sparse-checkout init --cone` | Enable cone mode (faster) | false |
| `git sparse-checkout set <dir1> <dir2>` | Set checked-out directories | false |
| `git sparse-checkout add <dir>` | Add directory to checkout | false |
| `git sparse-checkout list` | List sparse-checkout patterns | true |
| `git sparse-checkout reapply` | Reapply sparse-checkout rules | false |
| `git sparse-checkout disable` | Disable sparse checkout | false |

## Patching & Formatting

| Command | Description | is_pure_description |
|---|---|---|
| `git format-patch HEAD~3` | Create patch files for last 3 commits | true |
| `git format-patch main..feature` | Patch files for branch | true |
| `git format-patch -1 <commit>` | Patch file for single commit | true |
| `git format-patch --stdout HEAD~3 > all.patch` | Single combined patch | true |
| `git format-patch -o patches/ HEAD~3` | Output to directory | true |
| `git format-patch --cover-letter HEAD~3` | Include cover letter | true |
| `git am < patch.mbox` | Apply mailbox patch | false |
| `git am patches/*.patch` | Apply multiple patches | false |
| `git am --3way < patch.mbox` | Apply with 3-way merge | false |
| `git am --abort` | Abort patch application | false |
| `git am --continue` | Continue after resolving conflict | false |
| `git am --skip` | Skip current patch | false |
| `git apply patch.diff` | Apply diff without committing | false |
| `git apply --stat patch.diff` | Show patch stats | true |
| `git apply --check patch.diff` | Test if patch applies cleanly | true |
| `git apply --3way patch.diff` | Apply with 3-way merge | false |
| `git apply --reverse patch.diff` | Reverse-apply a patch | false |
| `git diff > changes.patch` | Create a diff patch | true |
| `git diff --binary > changes.patch` | Patch including binaries | true |

## Archiving & Bundling

| Command | Description | is_pure_description |
|---|---|---|
| `git archive HEAD --format=tar.gz -o repo.tar.gz` | Archive HEAD as tarball | true |
| `git archive HEAD --format=zip -o repo.zip` | Archive HEAD as zip | true |
| `git archive --prefix=project/ HEAD -o project.tar.gz` | Archive with directory prefix | true |
| `git archive <branch> -- <dir> -o dir.tar.gz` | Archive specific directory | true |
| `git bundle create repo.bundle --all` | Bundle entire repo | true |
| `git bundle create repo.bundle main` | Bundle specific branch | true |
| `git bundle create update.bundle main ^v1.0` | Bundle only new commits | true |
| `git bundle verify repo.bundle` | Verify bundle integrity | true |
| `git bundle list-heads repo.bundle` | List refs in bundle | true |
| `git clone repo.bundle` | Clone from bundle | false |
| `git fetch repo.bundle main` | Fetch from bundle | false |

## Maintenance & Housekeeping

| Command | Description | is_pure_description |
|---|---|---|
| `git gc` | Garbage collect (optimize repo) | false |
| `git gc --aggressive` | Aggressive optimization | false |
| `git gc --auto` | Run gc only if needed | false |
| `git gc --prune=now` | Prune all unreachable objects | false |
| `git prune` | Remove unreachable objects | false |
| `git prune --dry-run` | Show what would be pruned | true |
| `git fsck` | Verify repo integrity | true |
| `git fsck --full` | Full integrity check | true |
| `git fsck --unreachable` | List unreachable objects | true |
| `git fsck --dangling` | List dangling objects | true |
| `git maintenance start` | Enable background maintenance | false |
| `git maintenance stop` | Disable background maintenance | false |
| `git maintenance run` | Run maintenance tasks now | false |
| `git maintenance run --task=gc` | Run specific task | false |
| `git maintenance run --task=commit-graph` | Update commit graph | false |
| `git maintenance run --task=prefetch` | Prefetch from remotes | false |
| `git pack-refs --all` | Pack all refs for performance | false |
| `git repack` | Repack objects | false |
| `git repack -a -d` | Repack all, remove loose objects | false |
| `git count-objects -v` | Count and size loose objects | true |

## Plumbing & Low-Level

| Command | Description | is_pure_description |
|---|---|---|
| `git rev-parse HEAD` | Resolve HEAD to full SHA | true |
| `git rev-parse --short HEAD` | Short SHA of HEAD | true |
| `git rev-parse --abbrev-ref HEAD` | Current branch name | true |
| `git rev-parse --show-toplevel` | Root directory of repo | true |
| `git rev-parse --git-dir` | Path to .git directory | true |
| `git rev-parse --is-inside-work-tree` | Check if in worktree | true |
| `git rev-parse --verify <ref>` | Verify a ref exists | true |
| `git rev-list HEAD` | List all commit SHAs | true |
| `git rev-list --count HEAD` | Count total commits | true |
| `git rev-list --count main..feature` | Count commits in branch | true |
| `git rev-list --all --count` | Count all commits across all refs | true |
| `git cat-file -t <sha>` | Show object type | true |
| `git cat-file -p <sha>` | Pretty-print object content | true |
| `git cat-file -s <sha>` | Show object size | true |
| `git cat-file blob <sha>` | Show blob content | true |
| `git ls-tree HEAD` | List tree at HEAD | true |
| `git ls-tree -r HEAD` | Recursively list all files at HEAD | true |
| `git ls-tree --name-only HEAD` | List filenames only | true |
| `git ls-tree -r -l HEAD` | List with sizes | true |
| `git ls-files` | List tracked files | true |
| `git ls-files -s` | List staged files with mode/sha | true |
| `git ls-files -m` | List modified files | true |
| `git ls-files -d` | List deleted files | true |
| `git ls-files -o` | List untracked files | true |
| `git ls-files -o --exclude-standard` | Untracked, respecting gitignore | true |
| `git ls-files --ignored --exclude-standard` | List ignored files | true |
| `git ls-files -u` | List unmerged files | true |
| `git hash-object <file>` | Compute SHA for file | true |
| `git hash-object -w <file>` | Write file to object store | false |
| `git update-index --assume-unchanged <file>` | Ignore local changes to file | false |
| `git update-index --no-assume-unchanged <file>` | Undo assume-unchanged | false |
| `git update-index --skip-worktree <file>` | Skip file in worktree | false |
| `git update-index --no-skip-worktree <file>` | Undo skip-worktree | false |
| `git check-ignore <file>` | Check if file is ignored | true |
| `git check-ignore -v <file>` | Show which rule ignores file | true |
| `git check-attr <attr> <file>` | Check gitattributes for file | true |
| `git for-each-ref` | List all references | true |
| `git for-each-ref refs/heads/` | List local branches as refs | true |
| `git for-each-ref --sort=-committerdate refs/heads/` | Branches by last commit | true |
| `git for-each-ref --format='%(refname:short)'` | Custom ref format | true |
| `git symbolic-ref HEAD` | Show what HEAD points to | true |
| `git symbolic-ref HEAD refs/heads/main` | Set HEAD to branch | false |
| `git update-ref -d refs/heads/<branch>` | Delete a ref | false |
| `git name-rev <sha>` | Find symbolic name for SHA | true |
| `git describe` | Describe commit using nearest tag | true |
| `git describe --tags` | Describe using any tag | true |
| `git describe --always` | Describe, fall back to short SHA | true |
| `git describe --abbrev=0` | Just the tag name | true |
| `git shortlog -sn` | Commit count per author | true |
| `git shortlog -sn --all` | Across all branches | true |
| `git shortlog -sn --since="2024-01-01"` | Since a date | true |
| `git shortlog -sne` | Include emails | true |

## Git LFS (Large File Storage)

| Command | Description | is_pure_description |
|---|---|---|
| `git lfs install` | Initialize LFS in repo | false |
| `git lfs track "*.psd"` | Track file pattern with LFS | false |
| `git lfs track "assets/**"` | Track directory with LFS | false |
| `git lfs untrack "*.psd"` | Stop tracking pattern | false |
| `git lfs track` | List tracked patterns | true |
| `git lfs ls-files` | List LFS-tracked files | true |
| `git lfs ls-files -s` | List with size info | true |
| `git lfs status` | Show LFS file status | true |
| `git lfs pull` | Download LFS objects for current ref | false |
| `git lfs fetch` | Download LFS objects without checkout | false |
| `git lfs fetch --all` | Download all LFS objects | false |
| `git lfs fetch --recent` | Download recent LFS objects | false |
| `git lfs push origin main` | Push LFS objects to remote | false |
| `git lfs push --all origin` | Push all LFS objects | false |
| `git lfs prune` | Delete old local LFS files | false |
| `git lfs prune --dry-run` | Show what would be pruned | true |
| `git lfs migrate import --include="*.zip"` | Migrate existing files to LFS | false |
| `git lfs migrate import --everything --include="*.zip"` | Migrate across all branches | false |
| `git lfs migrate info` | Show what would be migrated | true |
| `git lfs env` | Show LFS environment info | true |
| `git lfs logs last` | Show last LFS operation log | true |
| `git lfs dedup` | Deduplicate LFS files | false |

## Hooks (Reference)

| Hook | Trigger | is_pure_description |
|---|---|---|
| `pre-commit` | Before commit is created | true |
| `prepare-commit-msg` | After default msg, before editor | true |
| `commit-msg` | After message entered, before commit | true |
| `post-commit` | After commit is created | true |
| `pre-rebase` | Before rebase starts | true |
| `post-rewrite` | After amend or rebase | true |
| `post-checkout` | After checkout or switch | true |
| `post-merge` | After merge completes | true |
| `pre-push` | Before push to remote | true |
| `pre-receive` | Server-side, before refs updated | true |
| `update` | Server-side, per-ref before update | true |
| `post-receive` | Server-side, after refs updated | true |
| `pre-auto-gc` | Before automatic gc | true |
| `post-rewrite` | After commit rewrite (amend/rebase) | true |
| `fsmonitor-watchman` | Filesystem monitoring integration | true |

## History Rewriting (Advanced)

| Command | Description | is_pure_description |
|---|---|---|
| `git filter-branch --tree-filter 'rm -f secrets.txt' HEAD` | Remove file from all history | false |
| `git filter-branch --env-filter '...' HEAD` | Rewrite author/committer in history | false |
| `git filter-branch --msg-filter 'sed ...' HEAD` | Rewrite commit messages | false |
| `git filter-branch --subdirectory-filter <dir>` | Extract subdirectory as root | false |
| `git filter-branch --index-filter 'git rm --cached --ignore-unmatch <f>' HEAD` | Fast file removal from history | false |
| `git filter-branch --prune-empty HEAD` | Remove empty commits after filter | false |
| `git filter-repo --path <dir> --force` | Keep only specified path (faster) | false |
| `git filter-repo --invert-paths --path <file>` | Remove file from all history | false |
| `git filter-repo --mailmap mailmap.txt` | Rewrite authors via mailmap | false |
| `git filter-repo --message-callback '...'` | Rewrite messages with callback | false |
| `git filter-repo --blob-callback '...'` | Rewrite file contents | false |
| `git filter-repo --strip-blobs-bigger-than 10M` | Remove large files from history | false |
| `git filter-repo --analyze` | Analyze repo for large files | true |
| `git replace <commit> <replacement>` | Replace a commit object | false |
| `git replace -d <commit>` | Delete a replacement | false |
| `git replace -l` | List replacements | true |
| `git notes add -m "note" <commit>` | Add a note to a commit | false |
| `git notes show <commit>` | Show note for commit | true |
| `git notes list` | List all notes | true |
| `git notes edit <commit>` | Edit a note | false |
| `git notes remove <commit>` | Remove a note | false |
| `git notes merge <ref>` | Merge notes refs | false |

## Debugging

| Command | Description | is_pure_description |
|---|---|---|
| `GIT_TRACE=1 git <command>` | Trace git execution | true |
| `GIT_TRACE_PERFORMANCE=1 git <command>` | Performance tracing | true |
| `GIT_CURL_VERBOSE=1 git <command>` | Verbose HTTP output | true |
| `GIT_SSH_COMMAND="ssh -v" git <command>` | Debug SSH connections | true |
| `GIT_TRACE_PACKET=1 git <command>` | Trace protocol packets | true |

## Special Files

| File | Purpose | is_pure_description |
|---|---|---|
| `.gitignore` | Patterns for untracked files to ignore | true |
| `.gitattributes` | Path-specific settings (LFS, diff, merge) | true |
| `.gitmodules` | Submodule URL and path mappings | true |
| `.gitkeep` | Convention: keep empty directory in git | true |
| `.mailmap` | Map author names/emails | true |
| `.git/config` | Repo-level configuration | true |
| `.git/hooks/` | Client-side hook scripts | true |
| `.git/info/exclude` | Local-only ignore rules | true |
| `.git/description` | Repo description (gitweb) | true |

## Useful Flags (Global)

| Flag | Description | is_pure_description |
|---|---|---|
| `-C <path>` | Run as if in specified directory | true |
| `--no-pager` | Disable pager | true |
| `--git-dir=<path>` | Use custom .git directory | true |
| `--work-tree=<path>` | Use custom working tree | true |
| `--bare` | Treat as bare repository | true |
| `--literal-pathspecs` | Treat pathspecs literally | true |
| `--no-optional-locks` | Skip optional locks (for scripting) | true |
| `-c <key>=<value>` | Set config for single command | false |
| `--exec-path` | Show or set git exec path | true |
| `--version` | Show git version | true |
| `--help` | Show help | true |
| `--html-path` | Show HTML docs path | true |

## Quick Recipes

```bash
# Undo last commit but keep changes staged
git reset --soft HEAD~1

# Completely undo last commit
git reset --hard HEAD~1

# Amend without changing message
git add . && git commit --amend --no-edit

# Squash last 3 commits interactively
git rebase -i HEAD~3
# (change "pick" to "squash" for commits to squash)

# Recover a deleted branch
git reflog | grep "branch-name"
git checkout -b recovered <sha>

# Find which commit introduced a bug
git bisect start
git bisect bad HEAD
git bisect good v1.0
# ... git checks out commits, you test and mark good/bad ...
git bisect reset

# Remove a file from history (use filter-repo, not filter-branch)
git filter-repo --invert-paths --path secrets.txt --force

# Show what changed between two tags
git diff v1.0 v2.0 --stat

# Find all commits that touched a function
git log -L :myFunction:src/app.py

# Show who last changed each line
git blame -w -M -C -C src/app.py

# Count lines of code by author
git log --all --format='%aN' | sort -u | while read name; do
  echo "$name:"; git log --all --author="$name" --pretty=tformat: --numstat | \
  awk '{ add += $1; del += $2 } END { printf "  +%s -%s\n", add, del }'
done

# Clean up: delete all merged local branches
git branch --merged main | grep -v "main" | xargs -r git branch -d

# Stash only staged changes
git stash push --staged -m "just the staged stuff"

# Create a patch from uncommitted changes
git diff > my-changes.patch

# Apply a patch
git apply my-changes.patch

# Move recent commits to a new branch
git branch new-feature
git reset --hard HEAD~3
git checkout new-feature

# Set up multiple push remotes
git remote set-url --add --push origin git@github.com:user/repo.git
git remote set-url --add --push origin git@gitlab.com:user/repo.git

# Find large files in history
git rev-list --objects --all | \
  git cat-file --batch-check='%(objecttype) %(objectname) %(objectsize) %(rest)' | \
  sed -n 's/^blob //p' | sort -rnk2 | head -20

# Shallow clone then get full history later
git clone --depth 1 <url>
git fetch --unshallow

# Interactive rebase with autosquash (for fixup commits)
git commit --fixup <sha>
git rebase -i --autosquash main

# Check which .gitignore rule applies
git check-ignore -v path/to/file

# Show file at specific commit without switching
git show main:src/app.py

# Compare branches: what's in feature but not main
git log main..feature --oneline

# Compare branches: what's different on both sides
git log main...feature --oneline --left-right

# Rebase preserving stacked branch updates
git rebase --update-refs main

# Worktree: work on two branches simultaneously
git worktree add ../hotfix hotfix-branch
# ... work in ../hotfix ...
git worktree remove ../hotfix
```