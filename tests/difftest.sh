#!/bin/bash
# Differential tests: pdfq vs a reference pdfgrep binary.
# Usage: PDFGREP=/path/to/pdfgrep bash tests/difftest.sh
# Compares stdout, stderr and exit code (program name normalized).
set -u
PDFGREP="${PDFGREP:-pdfgrep}"
PDFQ="$(cd "$(dirname "$0")/.." && pwd)/target/release/pdfq"
FIX="$(cd "$(dirname "$0")" && pwd)/fixtures"
cd "$FIX" || exit 1

if ! command -v "$PDFGREP" >/dev/null 2>&1 && [ ! -x "$PDFGREP" ]; then
    echo "pdfgrep not found; set PDFGREP=/path/to/pdfgrep" >&2
    exit 2
fi
if [ ! -x "$PDFQ" ]; then
    echo "pdfq not built; run: cargo build --release" >&2
    exit 2
fi

pass=0; fail=0

norm() {
    # Normalize program name/path in output for comparison.
    sed "s|pdfgrep:|PROG:|g; s|$PDFGREP|PROG|g; s|pdfq:|PROG:|g; s|$PDFQ|PROG|g; s|pdfq|PROG|g"
}

run() {
    local desc="$1"; shift
    local go qo gec qec ge qe
    go=$("$PDFGREP" "$@" 2>/tmp/dt_ge); gec=$?
    qo=$("$PDFQ" "$@" 2>/tmp/dt_qe); qec=$?
    go=$(printf '%s' "$go" | norm)
    qo=$(printf '%s' "$qo" | norm)
    ge=$(norm </tmp/dt_ge)
    qe=$(norm </tmp/dt_qe)
    if [ "$go" == "$qo" ] && [ "$ge" == "$qe" ] && [ "$gec" == "$qec" ]; then
        pass=$((pass+1))
    else
        fail=$((fail+1))
        echo "FAIL: $desc"
        echo "  args: $*"
        [ "$go" != "$qo" ] && { echo "  stdout diff:"; diff <(printf '%s\n' "$go") <(printf '%s\n' "$qo") | sed 's/^/    /'; }
        [ "$ge" != "$qe" ] && { echo "  stderr diff:"; diff <(printf '%s\n' "$ge") <(printf '%s\n' "$qe") | sed 's/^/    /'; }
        [ "$gec" != "$qec" ] && echo "  exit: pdfgrep=$gec pdfq=$qec"
    fi
}

run_sorted() {
    local desc="$1"; shift
    local go qo gec qec ge qe
    go=$("$PDFGREP" "$@" 2>/tmp/dt_ge); gec=$?
    go=$(printf '%s' "$go" | norm | sort)
    qo=$("$PDFQ" "$@" 2>/tmp/dt_qe); qec=$?
    qo=$(printf '%s' "$qo" | norm | sort)
    ge=$(norm </tmp/dt_ge)
    qe=$(norm </tmp/dt_qe)
    if [ "$go" == "$qo" ] && [ "$ge" == "$qe" ] && [ "$gec" == "$qec" ]; then
        pass=$((pass+1))
    else
        fail=$((fail+1))
        echo "FAIL(sorted): $desc"
        echo "  args: $*"
        diff <(printf '%s\n' "$go") <(printf '%s\n' "$qo") | sed 's/^/    /'
        [ "$ge" != "$qe" ] && echo "  stderr: g=[$ge] q=[$qe]"
        [ "$gec" != "$qec" ] && echo "  exit: g=$gec q=$qec"
    fi
}

# basic matching
run "basic -n"            -n --color=never -e foo t.pdf twopage.pdf
run "basic no -n"         --color=never -e foo t.pdf
run "pattern positional"  --color=never foo t.pdf
run "no match"            --color=never -e zzzz t.pdf
run "no match files"      --color=never -e zzzz t.pdf twopage.pdf
run "multiple -e"         -n --color=never -e foo -e bar t.pdf twopage.pdf
run "regex dot"           -n --color=never -e 'f.o' t.pdf
run "regex anchors"       -n --color=never -e '^page' t.pdf twopage.pdf
run "regex dollar"        -n --color=never -e 'foo$' t.pdf
run "regex dollar bar"    -n --color=never -e 'bar$' twopage.pdf t.pdf

# counts
run "-c one file"         --color=never -c -e foo t.pdf
run "-c two files"        --color=never -c -e foo t.pdf twopage.pdf
run "-c no match"         --color=never -c -e zzzz t.pdf
run "-p pagecount"        --color=never -p -e foo t.pdf twopage.pdf

# filenames / quiet
run "-o"                  -n -o --color=never -e 'foo+' t.pdf
run "-l one"              --color=never -l -e foo t.pdf twopage.pdf
run "-l none"             --color=never -l -e zzzz t.pdf twopage.pdf
run "-L one"              --color=never -L -e foo t.pdf twopage.pdf
run "-L none"             --color=never -L -e zzzz t.pdf twopage.pdf
run "-q match"            --color=never -q -e foo t.pdf
run "-q nomatch"          --color=never -q -e zzzz t.pdf

# max-count
run "-m 1"                -n --color=never -m 1 -e foo t.pdf twopage.pdf
run "-m 2"                -n --color=never -m 2 -e foo t.pdf twopage.pdf

# context
run "-A1"                 -n --color=never -A1 -e two t.pdf
run "-A1 gap"             -n --color=never -A1 -e '^foo bar' -e 'page two content foo' t.pdf
run "-B1"                 -n --color=never -B1 -e 'page two' t.pdf
run "-C1"                 -n --color=never -C1 -e two t.pdf twopage.pdf
run "-C1 gap"             -n --color=never -C1 -e '^foo bar' -e 'page two content foo' t.pdf
run "-C0 gap"             -n --color=never -C0 -e '^foo bar' -e 'page two content foo' t.pdf
run "-C2 first line"      -n --color=never -C2 -e 'foo bar' t.pdf
run "-C2 last line"       -n --color=never -C2 -e 'page two content foo' t.pdf
run "-C1 across pages"    -n --color=never -C1 -e foo -e bar twopage.pdf

# fixed / case
run "-F exact"            -n --color=never -F -e 'line two' t.pdf
run "-i"                  -n --color=never -i -e 'FOO BAR' t.pdf
run "-F -i"               -n --color=never -F -i -e 'FOO BAR' t.pdf

# pattern file
run "-f empty"            -n --color=never -f empty.pats t.pdf

# colors
run "colors default"      -e foo t.pdf
run "colors always"       --color=always -e foo t.pdf
run "colors never"        --color=never -e foo t.pdf

# page range
run "page-range 1"        -n --color=never --page-range 1 -e foo t.pdf twopage.pdf
run "page-range 2"        -n --color=never --page-range 2 -e foo t.pdf twopage.pdf
run "page-range empty"    -n --color=never --page-range 2-1 -e foo t.pdf
run "page-range bad"      -n --color=never --page-range 'x' -e foo t.pdf

# usage
run "no args"
run "only pattern"        --color=never foo
run "-h suppress name"    -n --color=never -h -e foo t.pdf twopage.pdf

# recursion (sorted: walk order intentionally differs from readdir)
run_sorted "-r dir"       -n --color=never -r -e foo .

# errors
run "missing file"        --color=never -e foo nope.pdf
run "corrupt pdf"         --color=never -e foo corrupt.pdf
run "corrupt with match"  --color=never -e foo corrupt.pdf t.pdf
run "-q corrupt"          --color=never -q -e foo corrupt.pdf

# PCRE
run "-P"                  -n --color=never -P -e 'f.o' t.pdf
run "-P lookahead"        -n --color=never -P -e 'foo(?= bar)' t.pdf

# -Z via od (bash strips NULs from substitution)
zo=$("$PDFGREP" -n --color=never -Z -e foo t.pdf twopage.pdf | od -An -tx1 | tr -d ' \n')
zq=$("$PDFQ" -n --color=never -Z -e foo t.pdf twopage.pdf | od -An -tx1 | tr -d ' \n')
if [ "$zo" == "$zq" ]; then pass=$((pass+1)); else
    fail=$((fail+1)); echo "FAIL: -Z bytes"; echo "  g=$zo"; echo "  q=$zq"
fi

echo
echo "passed=$pass failed=$fail"
[ "$fail" -eq 0 ]
