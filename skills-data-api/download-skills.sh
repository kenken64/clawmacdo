#!/usr/bin/env bash
# Download every SKILL.md for each skill slug into skills-data/skills/<slug>/SKILL.md
#
# Usage: ./skills-data/download-skills.sh
# Output: skills-data/skills/<slug>/SKILL.md for each skill

set -euo pipefail

REPO="VoltAgent/awesome-openclaw-skills"
DIR="categories"
SKILLS_DIR="skills-data/skills"
TMP_DIR="$(mktemp -d)"
URL_LIST="$TMP_DIR/urls.txt"
UPDATED_JSON="skills-data/skills.json"
MAX_PARALLEL=20

trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$SKILLS_DIR"

echo "=== Phase 1: Extracting skill URLs from category files ==="

FILES=$(gh api "repos/$REPO/contents/$DIR" --jq '.[] | select(.name | endswith(".md")) | .name')

> "$URL_LIST"
total_count=0

for file in $FILES; do
  raw_url="https://raw.githubusercontent.com/$REPO/main/$DIR/$file"
  local_file="$TMP_DIR/$file"
  curl -sL "$raw_url" -o "$local_file"

  category=$(grep -m1 '^# ' "$local_file" | sed 's/^# //')
  category_slug="${file%.md}"

  # Extract slug and URL from each skill line
  while IFS= read -r line; do
    # Get slug
    slug=$(echo "$line" | sed -n 's/^- \[\([^]]*\)\].*/\1/p')
    [ -z "$slug" ] && continue

    # Get URL from (...)
    url=$(echo "$line" | sed -n 's/^- \[[^]]*\](\([^)]*\)).*/\1/p')
    [ -z "$url" ] && continue

    # Get description
    desc=$(echo "$line" | sed -n 's/^- \[[^]]*\]([^)]*) - \(.*\)/\1/p')
    desc="${desc%.}"

    # Convert GitHub tree URL to raw SKILL.md URL
    # Pattern: https://github.com/openclaw/skills/tree/main/skills/author/slug/SKILL.md
    # -> https://raw.githubusercontent.com/openclaw/skills/main/skills/author/slug/SKILL.md
    if echo "$url" | grep -q 'SKILL.md$'; then
      raw_skill_url=$(echo "$url" | sed 's|github.com/\([^/]*/[^/]*\)/tree/|raw.githubusercontent.com/\1/|')
    else
      # URL points to directory, append /SKILL.md
      raw_skill_url=$(echo "$url" | sed 's|github.com/\([^/]*/[^/]*\)/tree/|raw.githubusercontent.com/\1/|')
      raw_skill_url="${raw_skill_url%/}/SKILL.md"
    fi

    echo "$slug|$raw_skill_url|$category_slug|$desc" >> "$URL_LIST"
    total_count=$((total_count + 1))
  done < <(grep '^- \[' "$local_file")

  echo "  $category: extracted URLs"
done

echo ""
echo "=== Phase 2: Downloading $total_count SKILL.md files (max $MAX_PARALLEL parallel) ==="

downloaded=0
failed=0
skipped=0

download_skill() {
  local slug="$1"
  local url="$2"
  local dest="$SKILLS_DIR/$slug"

  # Skip if already downloaded
  if [ -f "$dest/SKILL.md" ] && [ -s "$dest/SKILL.md" ]; then
    return 2  # skipped
  fi

  mkdir -p "$dest"
  if curl -sL --fail --max-time 15 "$url" -o "$dest/SKILL.md" 2>/dev/null; then
    # Verify it's not an HTML error page
    if head -1 "$dest/SKILL.md" | grep -qi '<!DOCTYPE\|<html\|404'; then
      rm -f "$dest/SKILL.md"
      rmdir "$dest" 2>/dev/null || true
      return 1  # failed
    fi
    return 0  # success
  else
    rm -f "$dest/SKILL.md"
    rmdir "$dest" 2>/dev/null || true
    return 1  # failed
  fi
}

export -f download_skill
export SKILLS_DIR

# Process in batches for progress reporting
batch_size=100
line_num=0

while IFS='|' read -r slug url category_slug desc; do
  line_num=$((line_num + 1))

  # Run download in background, limit parallelism
  (
    if download_skill "$slug" "$url"; then
      echo "OK:$slug"
    elif [ $? -eq 2 ]; then
      echo "SKIP:$slug"
    else
      echo "FAIL:$slug"
    fi
  ) &

  # Throttle: wait if too many background jobs
  if (( line_num % MAX_PARALLEL == 0 )); then
    wait
  fi

  # Progress report every batch_size
  if (( line_num % batch_size == 0 )); then
    echo "  Progress: $line_num / $total_count"
  fi

done < "$URL_LIST"

# Wait for remaining jobs
wait

echo ""
echo "=== Phase 3: Building updated skills.json with URLs ==="

# Rebuild JSON with url field included
python3 - "$URL_LIST" "$SKILLS_DIR" "$UPDATED_JSON" <<'PYEOF'
import json, sys, os

url_file = sys.argv[1]
skills_dir = sys.argv[2]
out_file = sys.argv[3]

# Parse URL list into structured data
categories = {}
for line in open(url_file):
    parts = line.strip().split('|', 3)
    if len(parts) < 4:
        continue
    slug, url, cat_slug, desc = parts
    if cat_slug not in categories:
        categories[cat_slug] = {'skills': []}
    has_file = os.path.isfile(os.path.join(skills_dir, slug, 'SKILL.md'))
    categories[cat_slug]['skills'].append({
        'slug': slug,
        'description': desc,
        'url': url,
        'has_skill_md': has_file
    })

# Load existing JSON to preserve category names and order
existing = json.load(open(out_file))
cat_names = {c['slug']: c['category'] for c in existing}

# Build output preserving original order
output = []
for cat in existing:
    cs = cat['slug']
    if cs in categories:
        skills = categories[cs]['skills']
    else:
        skills = [{'slug': s['slug'], 'description': s['description'], 'url': '', 'has_skill_md': False} for s in cat['skills']]
    output.append({
        'category': cat['category'],
        'slug': cs,
        'skill_count': len(skills),
        'skills': skills
    })

with open(out_file, 'w') as f:
    json.dump(output, f, indent=2, ensure_ascii=False)

downloaded = sum(1 for c in output for s in c['skills'] if s['has_skill_md'])
total = sum(len(c['skills']) for c in output)
print(f'Updated {out_file}: {len(output)} categories, {total} skills, {downloaded} with SKILL.md')
PYEOF

echo ""
echo "=== Done ==="
actual_files=$(find "$SKILLS_DIR" -name "SKILL.md" | wc -l | tr -d ' ')
echo "Downloaded SKILL.md files: $actual_files"
echo "Location: $SKILLS_DIR/<slug>/SKILL.md"
