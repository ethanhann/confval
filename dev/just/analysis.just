# Number of results to show
top := "30"
# Time window for "recent" queries
since := "30 days ago"

# Most frequently changed .rs files recently
analyze-hot-files:
    @echo "=== Most frequently changed files (since {{since}}) ==="
    @git log --since="{{since}}" --name-only --pretty=format: -- 'crates/**/*.rs' \
        | grep -v '^$$' | sort | uniq -c | sort -rn | head -n {{top}}

# Files with the most churn (lines added + removed) recently
analyze-churn:
    @echo "=== Highest churn files (since {{since}}) ==="
    @echo " total   add   del  file"
    @git log --since="{{since}}" --numstat --pretty=format: -- 'crates/**/*.rs' \
        | awk 'NF==3 {add[$$3]+=$$1; del[$$3]+=$$2} END {for(f in add) printf "%5d %5d %5d  %s\n", add[f]+del[f], add[f], del[f], f}' \
        | sort -rn | head -n {{top}}

# Commits per crate to see which crates are hottest
analyze-hot-crates:
    @echo "=== Hottest crates (since {{since}}) ==="
    @git log --since="{{since}}" --name-only --pretty=format: -- 'crates/' \
        | grep -v '^$$' | sed 's|crates/\([^/]*\)/.*|\1|' | sort | uniq -c | sort -rn

# Recently created files (net-new code to review)
analyze-new-files:
    @echo "=== Newly added files (since {{since}}) ==="
    @git log --since="{{since}}" --diff-filter=A --name-only --pretty=format: -- 'crates/**/*.rs' \
        | grep -v '^$$' | sort -u

# Long-term hotspots (most commits all-time)
analyze-hotspots:
    @echo "=== All-time most changed files ==="
    @git log --name-only --pretty=format: -- 'crates/**/*.rs' \
        | grep -v '^$$' | sort | uniq -c | sort -rn | head -n {{top}}

# Run all analyses in sequence
analyze: analyze-hot-files analyze-churn analyze-hot-crates analyze-new-files analyze-hotspots
