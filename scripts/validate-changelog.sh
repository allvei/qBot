#!/bin/bash

# Changelog Jargon Validation Script
# This script checks CHANGELOG.md for technical jargon that should be avoided

set -e

CHANGELOG_FILE="CHANGELOG.md"

# Colors for output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

echo "🔍 Checking changelog for technical jargon..."

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
    ["role"]="permission"
    ["id"]="identifier"
    ["hash"]="code"
    ["token"]="key"
    ["api"]="interface"
    ["module"]="component"
)

# Track if any issues were found
issues_found=false

echo -e "\n📋 Checking for forbidden technical terms..."

# Check each forbidden term (case insensitive)
for term in "${!forbidden_terms[@]}"; do
    if grep -i "\b$term\b" "$CHANGELOG_FILE" > /dev/null 2>&1; then
        echo -e "${RED}❌ Found forbidden term: '$term'${NC}"
        echo -e "   ${YELLOW}Suggested alternative: '${forbidden_terms[$term]}'${NC}"
        
        # Show the lines where the term appears
        grep -n -i "\b$term\b" "$CHANGELOG_FILE" | sed 's/^/   /'
        echo ""
        issues_found=true
    fi
done

# Check for other common technical patterns
echo -e "\n🔧 Checking for other technical patterns..."

# Check for programming-related terms
programming_terms=("class" "struct" "enum" "interface" "abstract" "inherit" "extend" "override")
for term in "${programming_terms[@]}"; do
    if grep -i "\b$term\b" "$CHANGELOG_FILE" > /dev/null 2>&1; then
        echo -e "${RED}❌ Found programming term: '$term'${NC}"
        echo -e "   ${YELLOW}Consider using more user-friendly language${NC}"
        grep -n -i "\b$term\b" "$CHANGELOG_FILE" | sed 's/^/   /'
        echo ""
        issues_found=true
    fi
done

# Check for file/extension patterns
if grep -E "\.(rs|js|py|sql|json|yaml|yml|toml)" "$CHANGELOG_FILE" > /dev/null 2>&1; then
    echo -e "${RED}❌ Found file extensions in changelog${NC}"
    echo -e "   ${YELLOW}File extensions should not appear in user-facing changelog entries${NC}"
    grep -n -E "\.(rs|js|py|sql|json|yaml|yml|toml)" "$CHANGELOG_FILE" | sed 's/^/   /'
    echo ""
    issues_found=true
fi

# Check for commit message patterns
if grep -E "^(feat|fix|refactor|perf|break|chore|docs|style|test):" "$CHANGELOG_FILE" > /dev/null 2>&1; then
    echo -e "${RED}❌ Found commit message prefixes in changelog${NC}"
    echo -e "   ${YELLOW}These should be converted to user-friendly descriptions${NC}"
    grep -n -E "^(feat|fix|refactor|perf|break|chore|docs|style|test):" "$CHANGELOG_FILE" | sed 's/^/   /'
    echo ""
    issues_found=true
fi

# Final result
if [ "$issues_found" = true ]; then
    echo -e "${RED}❌ Changelog validation failed!${NC}"
    echo -e "${YELLOW}Please review and fix the issues above before committing.${NC}"
    echo -e "\n💡 Tip: Use the changelog workflow at /.windsurf/workflows/changelog.md for guidance"
    exit 1
else
    echo -e "${GREEN}✅ Changelog validation passed!${NC}"
    echo -e "${GREEN}No technical jargon found. The changelog is user-friendly!${NC}"
    exit 0
fi
