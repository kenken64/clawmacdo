#!/usr/bin/env bash
# Scrape OpenClaw skills from GitHub and build a consolidated JSON file.
#
# Usage: ./skills-data/scrape-skills.sh
# Output: skills-data/skills.json

set -euo pipefail

REPO="VoltAgent/awesome-openclaw-skills"
DIR="categories"
OUT="skills-data/skills.json"
TMP_DIR="$(mktemp -d)"

trap 'rm -rf "$TMP_DIR"' EXIT

echo "Fetching category file list from $REPO..."
FILES=$(gh api "repos/$REPO/contents/$DIR" --jq '.[] | select(.name | endswith(".md")) | .name')

echo "Downloading and parsing markdown files..."

# Start JSON array
echo '[' > "$OUT"
FIRST_CAT=true

for file in $FILES; do
  raw_url="https://raw.githubusercontent.com/$REPO/main/$DIR/$file"
  local_file="$TMP_DIR/$file"

  curl -sL "$raw_url" -o "$local_file"

  # Extract category name from first # heading
  category=$(grep -m1 '^# ' "$local_file" | sed 's/^# //')

  # Category slug is the filename without .md
  category_slug="${file%.md}"

  # Extract skill count
  skill_count=$(grep -oE '\*\*[0-9]+ skills?\*\*' "$local_file" | grep -oE '[0-9]+' || echo "0")

  echo "  $category ($skill_count skills)"

  # Add comma separator between categories
  if [ "$FIRST_CAT" = true ]; then
    FIRST_CAT=false
  else
    echo ',' >> "$OUT"
  fi

  # Start category object
  cat >> "$OUT" <<CATEOF
  {
    "category": $(printf '%s' "$category" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))'),
    "slug": $(printf '%s' "$category_slug" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))'),
    "skill_count": $skill_count,
    "skills": [
CATEOF

  # Parse skill entries: lines matching "- [slug](url) - description"
  FIRST_SKILL=true
  while IFS= read -r line; do
    # Extract slug: text inside first [...]
    slug=$(echo "$line" | sed -n 's/^- \[\([^]]*\)\].*/\1/p')
    [ -z "$slug" ] && continue

    # Extract description: text after "] - " or "]) - "
    desc=$(echo "$line" | sed -n 's/^- \[[^]]*\]([^)]*) - \(.*\)/\1/p')
    # Trim trailing period if present
    desc="${desc%.}"

    if [ "$FIRST_SKILL" = true ]; then
      FIRST_SKILL=false
    else
      echo ',' >> "$OUT"
    fi

    # Write skill entry as JSON using python3 for safe escaping
    python3 -c "
import json, sys
obj = {'slug': sys.argv[1], 'description': sys.argv[2]}
sys.stdout.write('      ' + json.dumps(obj))
" "$slug" "$desc" >> "$OUT"

  done < <(grep '^- \[' "$local_file")

  # Close skills array and category object
  printf '\n    ]\n  }' >> "$OUT"

done

# Close root array
echo '' >> "$OUT"
echo ']' >> "$OUT"

# Validate JSON
python3 -m json.tool "$OUT" > /dev/null 2>&1 && echo "Valid JSON written to $OUT" || echo "WARNING: JSON validation failed"

# Print summary
total=$(python3 -c "import json; data=json.load(open('$OUT')); print(sum(len(c['skills']) for c in data))")
cats=$(python3 -c "import json; data=json.load(open('$OUT')); print(len(data))")
echo "Total: $cats categories, $total skills"
