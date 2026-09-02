# Number of results to show
top := "30"
# Time window for "recent" queries
since := "3 days ago"

# Most frequently changed .rs files recently
analyze-hot-files:
    @echo "★ Most frequently changed files (since {{since}})"
    @git log --since="{{since}}" --name-only --pretty=format: -- 'crates/**/*.rs' \
        | grep -v '^$$' | sort | uniq -c | sort -rn | head -n {{top}}

# Files with the most churn (lines added + removed) recently
analyze-churn:
    #!/usr/bin/env python3
    import subprocess, collections
    out = subprocess.check_output([
        "git", "log", "--since={{since}}", "--numstat", "--pretty=format:", "--", "crates/**/*.rs"
    ], text=True)
    add = collections.defaultdict(int)
    rem = collections.defaultdict(int)
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[0] != "-":
            add[parts[2]] += int(parts[0])
            rem[parts[2]] += int(parts[1])
    print("★ Highest churn files (since {{since}})")
    print(" total   add   del  file")
    ranked = sorted(add, key=lambda f: add[f] + rem[f], reverse=True)[:{{top}}]
    for f in ranked:
        t = add[f] + rem[f]
        print(f"{t:5d} {add[f]:5d} {rem[f]:5d}  {f}")

# Commits per crate to see which crates are hottest
analyze-hot-crates:
    @echo "★ Hottest crates (since {{since}})"
    @git log --since="{{since}}" --name-only --pretty=format: -- 'crates/' \
        | grep -v '^$$' | sed 's|crates/\([^/]*\)/.*|\1|' | sort | uniq -c | sort -rn

# Recently created files (net-new code to review)
analyze-new-files:
    @echo "★ Newly added files (since {{since}})"
    @git log --since="{{since}}" --diff-filter=A --name-only --pretty=format: -- 'crates/**/*.rs' \
        | grep -v '^$$' | sort -u

# Long-term hotspots (most commits all-time)
analyze-hotspots:
    @echo "★ All-time most changed files"
    @git log --name-only --pretty=format: -- 'crates/**/*.rs' \
        | grep -v '^$$' | sort | uniq -c | sort -rn | head -n {{top}}

# Run all analyses in sequence
analyze: analyze-hot-files analyze-churn analyze-hot-crates analyze-new-files analyze-hotspots
