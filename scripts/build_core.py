#!/usr/bin/env python3
"""Regenerate the small, hand-reviewed bootstrap catalog. No network or execution."""
import json
from pathlib import Path

# An option is name|aliases; arity; description. This is deliberately a subset.
SPECS = [
("git add", "Stage selected files or all working tree changes in the Git index", "--all|-A:none:Stage additions modifications and deletions;--update|-u:none:Stage modified and removed tracked files;--patch|-p:none:Interactively stage selected changes;--force|-f:none:Allow ignored files"),
("git commit", "Create a Git commit recording staged index changes with a message", "--message|-m:one:Use this commit message;--all|-a:none:Stage modified and deleted tracked files before committing;--amend:none:Replace the last commit;--no-edit:none:Reuse the previous commit message;--only|-o:none:Commit only specified paths;--dry-run:none:Preview what would be committed"),
("git status", "Inspect staged unstaged and untracked working tree changes", "--short|-s:none:Short status output;--branch|-b:none:Include branch information;--porcelain:optional:Stable machine readable status;--untracked-files|-u:optional:Control untracked file reporting"),
("git diff", "Compare changes in files working tree index and commits", "--cached|--staged:none:Compare staged changes with the current commit;--stat:none:Summarize changed file counts;--name-only:none:Print changed filenames;--check:none:Check whitespace errors;--exit-code:none:Exit one when differences exist"),
("git log", "Inspect commit history filter dates paths messages authors and branches", "--oneline:none:Abbreviated one line per commit;--since:one:Show commits after a date;--until:one:Show commits before a date;--author:one:Filter commit authors;--grep:one:Filter commit messages;--all:none:Include all refs;--graph:none:Show commit graph;--max-count|-n:one:Limit commits;--format:one:Customize log output;--follow:none:Follow file history across renames"),
("git switch", "Switch branches or create a new Git branch", "--create|-c:one:Create and switch to a new branch;--detach:none:Detach HEAD at a commit"),
("git branch", "List create inspect and delete branches", "--all|-a:none:List local and remote branches;--delete|-d:none:Delete a fully merged branch;-D:none:Force delete a branch;--move|-m:none:Rename a branch;--show-current:none:Print the current branch name"),
("git fetch", "Download repository refs and objects without merging", "--prune|-p:none:Remove stale remote tracking refs;--all:none:Fetch configured remotes;--tags:none:Fetch tags;--dry-run:none:Preview remote fetch"),
("git pull", "Fetch remote changes and integrate into the current branch", "--ff-only:none:Only permit a fast forward;--rebase:optional:Rebase instead of merging;--autostash:none:Temporarily stash local changes"),
("git push", "Publish commits branches and tags to a remote repository", "--set-upstream|-u:none:Set upstream tracking;--tags:none:Push tags;--force-with-lease:optional:Require an expected remote ref before force updating;--dry-run|-n:none:Preview remote updates;--delete|-d:none:Delete remote refs"),
("git restore", "Restore working tree or index files from a selected source", "--source|-s:one:Choose the source commit;--staged|-S:none:Restore the index;--worktree|-W:none:Restore working tree files;--patch|-p:none:Interactively choose changes"),
("git reset", "Move HEAD or reset index entries with different working tree behavior", "--soft:none:Keep the index and working tree unchanged;--mixed:none:Reset index but keep working tree changes;--hard:none:Discard tracked working tree and index changes"),
("git stash push", "Temporarily save uncommitted working tree and index changes", "--include-untracked|-u:none:Include untracked files;--message|-m:one:Describe the stash;--keep-index|-k:none:Keep staged changes in the index"),
("git stash pop", "Apply and remove a saved stash", "--index:none:Try to restore staged state"),
("git tag", "Create list verify and delete repository tags", "--annotate|-a:none:Create an annotated tag;--message|-m:one:Tag annotation message;--list|-l:none:List tags;--delete|-d:none:Delete tags"),
("git show", "Inspect a commit object or file at a revision", "--stat:none:Show changed file statistics;--format:one:Format commit metadata;--name-only:none:Print changed filenames"),
("git rev-parse", "Resolve revisions and repository paths for scripts", "--show-toplevel:none:Print repository root;--verify:none:Verify a single revision;--short:optional:Abbreviate an object name"),
("git init", "Initialize a repository", "--initial-branch|-b:one:Set initial branch name;--bare:none:Create a bare repository"),
("git clone", "Clone a remote repository into a directory", "--depth:one:Limit fetched commit history;--branch|-b:one:Select a branch;--recurse-submodules:optional:Initialize submodules"),
("git clean", "Remove untracked files and directories from the working tree", "--dry-run|-n:none:Preview deletion;-d:none:Include directories;--force|-f:none:Confirm removal;-x:none:Include ignored files"),
("git worktree add", "Create a linked working tree for another branch", "-b:one:Create a new branch;--detach:none:Detach the new worktree"),
("docker compose up", "Create start rebuild compose application service containers", "--build:none:Build images before startup;--detach|-d:none:Run containers in the background;--no-deps:none:Do not start dependencies;--force-recreate:none:Recreate containers;--wait:none:Wait for running or healthy services;--remove-orphans:none:Remove containers for undefined services"),
("docker compose build", "Build compose service images", "--no-cache:none:Disable build cache;--pull:none:Refresh base images;--build-arg:one:Set a build argument"),
("docker compose logs", "Display and follow compose service container logs", "--follow|-f:none:Follow live output;--tail:one:Limit recent lines;--since:one:Show logs after a time;--timestamps|-t:none:Include timestamps;--no-color:none:Disable ANSI colors"),
("docker compose ps", "List compose containers and current service status", "--all|-a:none:Include stopped containers;--format:one:Choose output format;--services:none:Print service names;--quiet|-q:none:Print container identifiers"),
("docker compose restart", "Restart existing compose service containers", "--no-deps:none:Do not restart dependencies;--timeout|-t:one:Set shutdown timeout"),
("docker compose down", "Stop and remove compose application containers and networks", "--volumes|-v:none:Remove named and anonymous volumes;--remove-orphans:none:Remove orphaned containers;--timeout|-t:one:Set shutdown timeout"),
("docker compose config", "Validate and render resolved compose configuration", "--quiet|-q:none:Validate without printing;--services:none:List defined services;--format:one:Choose output format"),
("docker compose exec", "Run an explicit command inside a compose service container", "-T:none:Disable pseudo terminal allocation;--user|-u:one:Select user;--workdir|-w:one:Choose working directory;--env|-e:one:Set environment variable"),
("docker compose pull", "Download images referenced by compose services", "--ignore-pull-failures:none:Continue after pull failures;--quiet|-q:none:Reduce progress output"),
("docker ps", "List running or stopped Docker containers", "--all|-a:none:Include stopped containers;--filter|-f:one:Filter containers;--format:one:Format output;--quiet|-q:none:Print identifiers only"),
("docker images", "List local container images", "--filter|-f:one:Filter images;--format:one:Format output;--quiet|-q:none:Print image IDs"),
("docker logs", "Read container stdout and stderr logs", "--follow|-f:none:Follow live logs;--tail:one:Limit recent lines;--since:one:Filter by start time;--timestamps|-t:none:Include timestamps"),
("docker inspect", "Inspect container image network and volume metadata", "--format|-f:one:Format selected properties;--type:one:Restrict object type"),
("docker stats", "Show container CPU memory network and block I/O usage", "--no-stream:none:Return a single snapshot;--format:one:Format statistics;--all|-a:none:Include all containers"),
("docker build", "Build a container image from a Dockerfile", "--tag|-t:one:Name and optionally tag the image;--file|-f:one:Choose a Dockerfile;--no-cache:none:Disable layer cache;--build-arg:one:Provide a build argument;--pull:none:Refresh base images"),
("docker run", "Create and start a container from an image", "--rm:none:Remove container after exit;--detach|-d:none:Run in background;--name:one:Name the container;--publish|-p:one:Publish a container port;--volume|-v:one:Mount a volume;--env|-e:one:Set environment variable;--read-only:none:Use a read only root filesystem;--network:one:Select a network"),
("docker container prune", "Remove stopped Docker containers", "--force|-f:none:Skip confirmation;--filter:one:Filter containers to remove"),
("docker system df", "Inspect Docker disk usage", "--verbose|-v:none:Show detailed storage usage"),
("docker image prune", "Remove unused Docker images", "--all|-a:none:Remove all unused images;--force|-f:none:Skip confirmation;--filter:one:Filter candidate images"),
("podman ps", "List running or stopped Podman containers", "--all|-a:none:Include stopped containers;--format:one:Format output;--quiet|-q:none:Print identifiers"),
("rg", "Recursively search file contents for text or regular expressions", "--glob|-g:one:Include or exclude matching file globs;--type|-t:one:Restrict to a file type;--ignore-case|-i:none:Ignore letter case;--line-number|-n:none:Show line numbers;--files-with-matches|-l:none:Print matching filenames;--hidden:none:Search hidden files;--no-ignore:none:Do not respect ignore files;--fixed-strings|-F:none:Use literal text;--count|-c:none:Count matching lines;--files:none:List files that would be searched;--null|-0:none:Separate filenames with NUL;--context|-C:one:Print surrounding context"),
("grep", "Find matching lines in input or files", "--recursive|-r:none:Search directories recursively;--ignore-case|-i:none:Ignore case;--line-number|-n:none:Show line numbers;--invert-match|-v:none:Select nonmatching lines;--extended-regexp|-E:none:Use extended regular expressions;--fixed-strings|-F:none:Use fixed text;--files-with-matches|-l:none:Print matching filenames;--count|-c:none:Count matching lines"),
("find", "Search filesystem paths by name type size permissions and modification time", "-name:one:Match a filename pattern;-iname:one:Match filename ignoring case;-type:one:Filter filesystem entry type;-mtime:one:Filter modification age in 24 hour units;-mmin:one:Filter modification age in minutes;-size:one:Filter by size;-maxdepth:one:Limit traversal depth;-mindepth:one:Start matching below a depth;-path:one:Match an entire path;-prune:none:Do not descend into a directory;-print:none:Print a matching path;-print0:none:Print paths terminated by NUL;-delete:none:Delete matching paths;-empty:none:Select empty files or directories;-perm:one:Filter file mode bits;-o:none:Logical OR;-a:none:Logical AND"),
("fd", "Find filesystem entries by name with sensible ignore rules", "--type|-t:one:Filter entry type;--extension|-e:one:Filter file extension;--hidden|-H:none:Include hidden entries;--no-ignore|-I:none:Ignore ignore files;--exclude|-E:one:Exclude a pattern;--max-depth|-d:one:Limit traversal depth;--print0|-0:none:Separate paths with NUL"),
("rsync", "Synchronize copy files and directories preserving selected metadata", "--archive|-a:none:Preserve common metadata and recurse;--recursive|-r:none:Recurse into directories;--exclude:one:Exclude matching file patterns;--include:one:Include matching file patterns;--dry-run|-n:none:Preview changes without transferring;--delete:none:Remove receiver files absent at sender;--progress:none:Report transfer progress;--times|-t:none:Preserve modification times;--perms|-p:none:Preserve permissions;--checksum|-c:none:Use content checksums;--itemize-changes|-i:none:Describe changed attributes;--compress|-z:none:Compress transfer data"),
("tar", "Create list and extract tar archives with compression and exclusions", "--create|-c:none:Create an archive;--extract|-x:none:Extract archive members;--list|-t:none:List archive members;--file|-f:one:Read or write this archive file;--gzip|-z:none:Use gzip compression;--xz|-J:none:Use xz compression;--zstd:none:Use zstandard compression;--directory|-C:one:Change directory before processing;--exclude:one:Exclude matching member names;--verbose|-v:none:Print processed names"),
("gzip", "Compress or decompress gzip files", "--decompress|-d:none:Decompress;--keep|-k:none:Keep the original input;--stdout|-c:none:Write to standard output;--test|-t:none:Test compressed data integrity;-9:none:Use maximum compression"),
("zstd", "Compress decompress and test Zstandard data", "--decompress|-d:none:Decompress input;--stdout|-c:none:Write standard output;--keep|-k:none:Keep source files;--test|-t:none:Verify compressed data;-o:one:Select output file;-T:one:Select worker count"),
("ls", "List directory entries including hidden files and metadata", "--all|-a:none:Include hidden entries;--human-readable|-h:none:Human readable sizes;-l:none:Long listing format;-S:none:Sort by size;-t:none:Sort by modification time;--reverse|-r:none:Reverse sort order;--recursive|-R:none:List subdirectories"),
("mkdir", "Create directories", "--parents|-p:none:Create missing parents;--mode|-m:one:Set permissions;--verbose|-v:none:Report created directories"),
("cp", "Copy files and directories", "--recursive|-R|-r:none:Copy directories recursively;--archive|-a:none:Preserve attributes recursively;--no-clobber|-n:none:Do not overwrite existing paths;--preserve:optional:Preserve selected attributes;--verbose|-v:none:Print copied paths"),
("mv", "Move or rename files and directories", "--no-clobber|-n:none:Do not replace existing files;--interactive|-i:none:Prompt before overwrite;--verbose|-v:none:Describe moves"),
("rm", "Remove files or directories", "--recursive|-r|-R:none:Remove directories recursively;--force|-f:none:Do not prompt for missing entries;--interactive:optional:Prompt before removals;-i:none:Prompt before each removal;--verbose|-v:none:Describe deletions"),
("du", "Estimate disk usage of files and directories", "--human-readable|-h:none:Human readable sizes;--summarize|-s:none:Print a total for each argument;--max-depth|-d:one:Limit reported depth;--one-file-system|-x:none:Stay on one filesystem"),
("df", "Show filesystem space usage", "--human-readable|-h:none:Human readable units;--print-type|-T:none:Include filesystem type;--inodes|-i:none:Report inode usage"),
("sort", "Sort text lines", "--numeric-sort|-n:none:Compare numeric values;--human-numeric-sort|-h:none:Compare human readable sizes;--reverse|-r:none:Reverse ordering;--unique|-u:none:Output only unique lines;--key|-k:one:Select sort key;--field-separator|-t:one:Choose field separator;--zero-terminated|-z:none:Use NUL terminated records"),
("uniq", "Filter adjacent repeated lines", "--count|-c:none:Prefix occurrence counts;--repeated|-d:none:Print duplicates only;--unique|-u:none:Print nonrepeated lines"),
("head", "Show the first lines or bytes of input", "--lines|-n:one:Set line count;--bytes|-c:one:Set byte count"),
("tail", "Show final input lines or follow appended output", "--lines|-n:one:Set line count;--follow:optional:Follow growing output;-f:none:Follow growing output;-F:none:Follow by filename and retry"),
("wc", "Count input lines words characters and bytes", "--lines|-l:none:Count lines;--words|-w:none:Count words;--bytes|-c:none:Count bytes;--chars|-m:none:Count characters"),
("sha256sum", "Compute or verify SHA256 checksums of files", "--check|-c:none:Verify checksums from a file;--binary|-b:none:Read in binary mode;--status:none:Use exit status without output"),
("chmod", "Change file permission mode bits", "--recursive|-R:none:Apply recursively;--reference:one:Copy mode from another file;--verbose|-v:none:Describe changes"),
("touch", "Create empty files or update file timestamps", "--no-create|-c:none:Do not create missing files;--reference|-r:one:Copy timestamps from a reference;--date|-d:one:Use a specified date"),
("ln", "Create hard or symbolic filesystem links", "--symbolic|-s:none:Create symbolic links;--force|-f:none:Replace existing destinations;--no-dereference|-n:none:Treat symlink destination as a file"),
("readlink", "Read or canonicalize symbolic link targets", "--canonicalize|-f:none:Resolve existing path components;--canonicalize-existing|-e:none:Require every component to exist"),
("ss", "Inspect network sockets listening ports and owning processes", "--listening|-l:none:Show listening sockets;--tcp|-t:none:Show TCP sockets;--udp|-u:none:Show UDP sockets;--numeric|-n:none:Avoid resolving names;--processes|-p:none:Show owning processes;--all|-a:none:Include listening and non listening sockets"),
("lsof", "List open files sockets and processes using ports", "-i:optional:Filter network connections;-P:none:Keep numeric port numbers;-n:none:Avoid host name resolution;-t:none:Print process IDs only;-p:one:Select a process"),
("ps", "Show running process metadata", "-e:none:Select all processes;-f:none:Full format;-o:one:Choose output columns;--sort:one:Sort by selected properties;-p:one:Select process IDs"),
("kill", "Send a signal to a process", "--signal|-s:one:Select a signal;--list|-l:optional:List or translate signals"),
("systemctl status", "Inspect systemd service health and recent status", "--no-pager:none:Do not start a pager;--full|-l:none:Do not ellipsize output"),
("systemctl restart", "Restart a systemd unit", "--user:none:Use the user service manager;--no-block:none:Do not wait for completion"),
("systemctl is-active", "Test whether a systemd unit is active", "--quiet|-q:none:Return status without text"),
("systemctl list-units", "List loaded systemd units", "--type:one:Filter unit types;--state:one:Filter unit state;--all|-a:none:Include inactive units;--no-pager:none:Disable paging"),
("journalctl", "Query and follow systemd journal logs", "--unit|-u:one:Filter by systemd unit;--follow|-f:none:Follow new entries;--since|-S:one:Filter by starting time;--until|-U:one:Filter by ending time;--lines|-n:one:Limit recent entries;--boot|-b:optional:Filter by boot;--priority|-p:one:Filter by priority;--no-pager:none:Disable paging;--output|-o:one:Choose output format"),
("curl", "Transfer data using URLs and inspect HTTP responses", "--fail|-f:none:Report HTTP error responses as failures;--silent|-s:none:Silence progress output;--show-error|-S:none:Show errors with silent mode;--location|-L:none:Follow redirects;--head|-I:none:Request headers only;--output|-o:one:Write body to a file;--request|-X:one:Select request method;--header|-H:one:Add a request header;--data|-d:one:Send request data;--connect-timeout:one:Limit connection setup time;--max-time|-m:one:Limit total transfer time;--retry:one:Set retry attempts"),
("jq", "Filter transform validate and format JSON input", "--raw-output|-r:none:Print string values without JSON quoting;--compact-output|-c:none:Use compact JSON;--slurp|-s:none:Read all inputs into an array;--exit-status|-e:none:Reflect the result in exit status;--arg:two:Bind a string variable using name and value;--sort-keys|-S:none:Sort object keys;--null-input|-n:none:Run without reading input"),
("cargo build", "Compile Rust package targets", "--release:none:Use release optimizations;--package|-p:one:Select workspace package;--bin:one:Select binary target;--features:one:Enable features;--no-default-features:none:Disable default features;--all-features:none:Enable all features;--locked:none:Require an unchanged lockfile;--offline:none:Prevent network access;--target:one:Select target triple;--jobs|-j:one:Set parallel jobs"),
("cargo test", "Run Rust unit integration and documentation tests", "--release:none:Use release build;--package|-p:one:Select package;--lib:none:Run library tests;--test:one:Select integration test;--all-features:none:Enable all features;--locked:none:Require unchanged lockfile;--offline:none:Prevent network access"),
("cargo clippy", "Lint Rust code using Clippy", "--all-targets:none:Check all targets;--all-features:none:Enable all features;--no-default-features:none:Disable default features"),
("cargo fmt", "Format Rust source code", "--check:none:Check formatting without changing files;--all:none:Format all workspace packages"),
("cargo clean", "Remove Rust build artifacts", "--release:none:Remove release artifacts;--package|-p:one:Select a package;--dry-run:none:Describe without removing"),
("cargo tree", "Inspect Rust dependency graph", "--invert|-i:one:Show reverse dependencies;--duplicates|-d:none:Show duplicated dependencies;--edges|-e:one:Select dependency edge kinds"),
("go build", "Compile Go packages and dependencies", "-o:one:Choose output file;-race:none:Enable race detection;-trimpath:none:Remove filesystem paths from artifacts;-tags:one:Select build tags"),
("go test", "Test Go packages", "-race:none:Enable race detection;-cover:none:Measure code coverage;-run:one:Select tests;-count:one:Set test repetition count;-v:none:Print verbose test output"),
("go fmt", "Format Go packages", ""),
("make", "Build selected Makefile targets", "--jobs|-j:optional:Set parallel recipe count;--dry-run|-n:none:Print recipes without running;--file|-f:one:Choose a Makefile;--directory|-C:one:Change working directory"),
("npm ci", "Install Node dependencies exactly from a lockfile", "--ignore-scripts:none:Do not run lifecycle scripts;--omit:one:Omit selected dependency types;--offline:none:Require cached packages"),
("npm test", "Run the package test script", "--ignore-scripts:none:Suppress lifecycle hooks"),
("npm run", "Run a named package script", "--if-present:none:Ignore a missing script;--workspace|-w:one:Select a workspace"),
("python", "Run Python code or modules", "-m:one:Run a module as a script;-c:one:Execute supplied Python code;-I:none:Use isolated mode;-B:none:Do not write bytecode files"),
("pytest", "Run Python test cases", "-q:none:Reduce output verbosity;-v:none:Increase output verbosity;-k:one:Select tests by expression;-x:none:Stop at the first failure;--maxfail:one:Limit failures;--disable-warnings:none:Hide warning summaries"),
("ruff check", "Lint Python code with Ruff", "--fix:none:Apply supported fixes;--select:one:Select lint rule codes;--output-format:one:Choose diagnostic format"),
("ruff format", "Format Python code with Ruff", "--check:none:Report formatting changes without writing;--diff:none:Print formatting differences"),
("ffmpeg", "Convert transcode resize and remux audio and video files", "-i:one:Read input media;-c:one:Set codec for streams;-c:v:one:Set video codec;-c:a:one:Set audio codec;-vf:one:Apply video filters;-af:one:Apply audio filters;-an:none:Disable audio;-vn:none:Disable video;-ss:one:Seek to a time;-t:one:Limit duration;-y:none:Overwrite output without confirmation;-n:none:Never overwrite output;-map:one:Choose input streams;-movflags:one:Set MP4 muxer flags"),
("ffprobe", "Inspect multimedia stream and container metadata", "-v:one:Set logging verbosity;-show_streams:none:Show stream metadata;-show_format:none:Show container metadata;-of:one:Select output format;-select_streams:one:Select streams;-show_entries:one:Select metadata fields"),
("kubectl get", "List Kubernetes resources", "--namespace|-n:one:Select namespace;--all-namespaces|-A:none:Query all namespaces;--output|-o:one:Choose output format;--selector|-l:one:Filter labels;--watch|-w:none:Watch resource changes"),
("kubectl describe", "Inspect Kubernetes resource details and events", "--namespace|-n:one:Select namespace;--selector|-l:one:Select label matches"),
("kubectl logs", "Read Kubernetes pod or workload logs", "--namespace|-n:one:Select namespace;--follow|-f:none:Follow logs;--container|-c:one:Choose a container;--previous|-p:none:Use a previous container instance;--tail:one:Limit recent lines;--since:one:Filter recent duration"),
("kubectl apply", "Apply a Kubernetes resource configuration", "--filename|-f:one:Read a resource file;--dry-run:one:Select client or server dry run;--namespace|-n:one:Select namespace;--server-side:none:Use server side apply"),
("kubectl diff", "Compare Kubernetes desired configuration with live resources", "--filename|-f:one:Read configuration from file;--namespace|-n:one:Select namespace"),
("kubectl rollout status", "Wait for Kubernetes rollout completion", "--namespace|-n:one:Select namespace;--timeout:one:Set wait timeout"),
("kubectl rollout restart", "Restart a Kubernetes workload rollout", "--namespace|-n:one:Select namespace"),
("kubectl config current-context", "Show the currently selected Kubernetes context", ""),
("kubectl config use-context", "Switch the current Kubernetes context", ""),
("kubectl port-forward", "Forward a local port to a Kubernetes pod or service", "--namespace|-n:one:Select namespace;--address:one:Choose bind addresses"),
("terraform init", "Initialize a Terraform working directory", "-upgrade:none:Update providers and modules;-backend:one:Enable or disable backend setup;-input:one:Enable or disable interactive prompts"),
("terraform fmt", "Format Terraform configuration", "-check:none:Check formatting without writing;-recursive:none:Process nested directories;-diff:none:Show formatting differences"),
("terraform validate", "Validate Terraform configuration syntax and internal consistency", "-json:none:Return JSON diagnostics"),
("terraform plan", "Preview proposed Terraform infrastructure changes", "-out:one:Save an execution plan;-var-file:one:Read input variables;-detailed-exitcode:none:Distinguish errors no changes and changes;-input:one:Control interactive prompts"),
("terraform show", "Inspect Terraform state or a saved execution plan", "-json:none:Return machine readable JSON"),
("terraform apply", "Apply a saved Terraform plan or proposed infrastructure changes", "-auto-approve:none:Skip approval prompts;-input:one:Control interactive prompts"),
("aws s3 cp", "Copy a file or object to from or between S3 locations", "--recursive:none:Copy multiple objects recursively;--exclude:one:Exclude matching paths;--include:one:Include matching paths;--dryrun:none:Preview transfers;--only-show-errors:none:Hide successful progress;--profile:one:Use a named AWS profile"),
("aws s3 sync", "Synchronize local directories and S3 prefixes", "--delete:none:Remove destination entries absent at source;--exclude:one:Exclude matching paths;--include:one:Include matching paths;--dryrun:none:Preview synchronization;--exact-timestamps:none:Require exact timestamp matches"),
("aws sts get-caller-identity", "Inspect the AWS account and identity for current credentials", "--profile:one:Select credentials profile;--output:one:Select output format;--query:one:Filter output using JMESPath"),
("openssl dgst", "Compute message digests and verify digital signatures", "-sha256:none:Use SHA256 digest;-out:one:Write digest to a file;-verify:one:Verify with a public key;-signature:one:Read a signature file"),
("openssl x509", "Inspect convert and validate certificate metadata", "-in:one:Read a certificate file;-noout:none:Do not print encoded certificate;-dates:none:Print validity dates;-subject:none:Print certificate subject;-issuer:none:Print certificate issuer;-fingerprint:none:Print fingerprint;-sha256:none:Select SHA256"),
]

def reference(command):
    root = command.split()[0]
    if root == "git": return "https://git-scm.com/docs/" + command.replace(" ", "-")
    if root == "docker": return "https://docs.docker.com/reference/cli/" + command.replace(" ", "/") + "/"
    if root == "cargo": return "https://doc.rust-lang.org/cargo/commands/" + command.replace(" ", "-") + ".html"
    if root == "kubectl": return "https://kubernetes.io/docs/reference/kubectl/"
    return {
        "rg": "https://github.com/BurntSushi/ripgrep/blob/master/crates/core/flags/defs.rs",
        "fd": "https://github.com/sharkdp/fd", "rsync": "https://download.samba.org/pub/rsync/rsync.1",
        "curl": "https://curl.se/docs/manpage.html", "jq": "https://jqlang.org/manual/",
        "ffmpeg": "https://ffmpeg.org/ffmpeg.html", "ffprobe": "https://ffmpeg.org/ffprobe.html",
        "terraform": "https://developer.hashicorp.com/terraform/cli/commands",
        "aws": "https://docs.aws.amazon.com/cli/latest/reference/", "go": "https://pkg.go.dev/cmd/go",
        "npm": "https://docs.npmjs.com/cli/", "python": "https://docs.python.org/3/using/cmdline.html",
        "pytest": "https://docs.pytest.org/en/stable/reference/reference.html",
        "ruff": "https://docs.astral.sh/ruff/configuration/", "openssl": "https://docs.openssl.org/master/man1/",
        "systemctl": "https://www.freedesktop.org/software/systemd/man/latest/systemctl.html",
        "journalctl": "https://www.freedesktop.org/software/systemd/man/latest/journalctl.html",
        "find": "https://www.gnu.org/software/findutils/manual/html_mono/find.html",
        "tar": "https://www.gnu.org/software/tar/manual/tar.html",
    }.get(root, "https://man7.org/linux/man-pages/man1/" + root + ".1.html")

def main():
    records = []
    for command, summary, raw in SPECS:
        source = {"kind": "builtin", "reference": reference(command)}
        components = command.split()
        records.append({"id": command.replace(" ", "/") + "#command", "command": components,
                        "kind": "command", "name": components[-1], "summary": summary,
                        "arity": "unknown", "required": False, "source": source})
        for item in raw.split(";"):
            if not item: continue
            # ffmpeg stream specifiers contain ':', so split at the arity delimiter.
            import re
            match = re.fullmatch(r"(.+?):(none|one|two|optional|many|unknown):(.*)", item)
            if not match: raise ValueError(item)
            names, arity, description = match.groups()
            name, *aliases = names.split("|")
            record = {"id": command.replace(" ", "/") + "#" + name, "command": components,
                      "kind": "option", "name": name, "summary": description, "arity": arity,
                      "required": False, "source": source}
            if aliases: record["aliases"] = aliases
            records.append(record)
    records.sort(key=lambda r: r["id"])
    destination = Path(__file__).resolve().parents[1] / "data" / "core.jsonl"
    destination.write_text("".join(json.dumps(r, separators=(",", ":"), ensure_ascii=True, sort_keys=True) + "\n" for r in records))
    print(f"core: {len(records)} records, {len(SPECS)} command scopes")

if __name__ == "__main__": main()
