#!/bin/bash

# Changelog Jargon Validation Script
# This script checks the user-facing sections ("Users & Admins" and "New Commands")
# of the newest CHANGELOG.md entry for technical jargon. Developer sections and
# older entries are skipped.

set -e

CHANGELOG_FILE="CHANGELOG.md"

# Colors for output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

echo "Checking changelog for technical jargon..."

# Prints "<line number>:<text>" for lines in the user-facing sections of the newest entry
user_facing_lines() {
    awk '
        /^# v[0-9]/ { entries++; if (entries > 1) exit; next }
        /^## / { in_user = ($0 ~ /^## (Users & Admins|New Commands)/); next }
        in_user { print NR ":" $0 }
    ' "$CHANGELOG_FILE"
}

CHECKED_LINES=$(user_facing_lines)

# Prints checked lines matching the given grep arguments
find_matches() {
    printf '%s\n' "$CHECKED_LINES" | grep "$@" || true
}

# Forbidden terms and their user-friendly alternatives
declare -A forbidden_terms=(
    ["database"]="data storage"
    ["table"]="data storage"
    ["migration"]="data update"
    ["schema"]="structure"
    ["repository"]="storage"
    ["function"]="feature"
    ["method"]="feature"
    ["parameter"]="setting"
    ["implementation"]="feature"
    ["refactoring"]="improvement"
    ["performance"]="speed"
    ["optimization"]="improvement"
    ["cache"]="memory"
    ["thread"]="background"
    ["async"]="background"
    ["permission"]="access"
    ["role"]="access level"
    ["id"]="identifier"
    ["hash"]="code"
    ["token"]="key"
    ["api"]="interface"
    ["module"]="component"
)

# Track if any issues were found
issues_found=false

echo -e "\nChecking for forbidden technical terms..."

# Check each forbidden term (case insensitive)
for term in "${!forbidden_terms[@]}"; do
    matches=$(find_matches -i "\b$term\b")
    if [ -n "$matches" ]; then
        echo -e "${RED}❌ Found forbidden term: '$term'${NC}"
        echo -e "   ${YELLOW}Suggested alternative: '${forbidden_terms[$term]}'${NC}"
        echo "$matches" | sed 's/^/   /'
        echo ""
        issues_found=true
    fi
done

# Check for other common technical patterns
echo -e "\nChecking for other technical patterns..."

# Check for programming-related terms
programming_terms=("class" "struct" "enum" "abstract" "inherit" "extend" "override")
for term in "${programming_terms[@]}"; do
    matches=$(find_matches -i "\b$term\b")
    if [ -n "$matches" ]; then
        echo -e "${RED}❌ Found programming term: '$term'${NC}"
        echo -e "   ${YELLOW}Consider using more user-friendly language${NC}"
        echo "$matches" | sed 's/^/   /'
        echo ""
        issues_found=true
    fi
done

# Check for file/extension patterns
matches=$(find_matches -E "\.(rs|js|py|sql|json|yaml|yml|toml)\b")
if [ -n "$matches" ]; then
    echo -e "${RED}❌ Found file extensions in changelog${NC}"
    echo -e "   ${YELLOW}File extensions should not appear in user-facing changelog entries${NC}"
    echo "$matches" | sed 's/^/   /'
    echo ""
    issues_found=true
fi

# Check for commit message patterns
matches=$(find_matches -E "^[0-9]+:[-* ]*(feat|fix|refactor|perf|break|chore|docs|style|test)(\([^)]*\))?:")
if [ -n "$matches" ]; then
    echo -e "${RED}❌ Found commit message prefixes in changelog${NC}"
    echo -e "   ${YELLOW}These should be converted to user-friendly descriptions${NC}"
    echo "$matches" | sed 's/^/   /'
    echo ""
    issues_found=true
fi

# Final result
if [ "$issues_found" = true ]; then
    echo -e "${RED}❌ Changelog validation failed!${NC}"
    echo -e "${YELLOW}Please review and fix the issues above before committing.${NC}"
    echo -e "\nTip: Use the changelog workflow at /.windsurf/workflows/changelog.md for guidance"
    exit 1
else
    echo -e "${GREEN}✅ Changelog validation passed!${NC}"
    echo -e "${GREEN}No technical jargon found. The changelog is user-friendly!${NC}"
    exit 0
fi
