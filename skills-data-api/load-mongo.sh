#!/usr/bin/env bash
# load-mongo.sh — Load skills.json into MongoDB
# Usage: bash load-mongo.sh [MONGO_URI] [DB_NAME]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SKILLS_JSON="$SCRIPT_DIR/skills.json"

MONGO_URI="${1:-${MONGO_URI:-mongodb://mongo:xPXhguLfGwQFtcXLAgpihPdrwUtOBcYN@yamabiko.proxy.rlwy.net:52355/}}"
DB_NAME="${2:-${DB_NAME:-clawmacdo}}"

if [ ! -f "$SKILLS_JSON" ]; then
  echo "ERROR: $SKILLS_JSON not found"
  exit 1
fi

command -v node >/dev/null 2>&1 || { echo "ERROR: node is required"; exit 1; }

echo "Loading skills data into MongoDB..."
echo "  URI: ${MONGO_URI%%@*}@***"
echo "  DB:  $DB_NAME"
echo "  Source: $SKILLS_JSON"

node --no-warnings -e "
const { MongoClient } = require('mongodb');
const fs = require('fs');

async function load() {
  const uri = process.argv[1];
  const dbName = process.argv[2];
  const jsonPath = process.argv[3];

  const data = JSON.parse(fs.readFileSync(jsonPath, 'utf-8'));

  const client = new MongoClient(uri);
  await client.connect();
  const db = client.db(dbName);

  // --- Load categories ---
  const categories = data.map(c => ({
    category: c.category,
    slug: c.slug,
    skill_count: c.skill_count
  }));

  await db.collection('categories').deleteMany({});
  if (categories.length > 0) {
    await db.collection('categories').insertMany(categories);
  }
  console.log('  Inserted ' + categories.length + ' categories');

  // --- Flatten and deduplicate skills ---
  const seen = new Set();
  const skills = [];
  for (const cat of data) {
    for (const skill of cat.skills) {
      const key = skill.slug + '::' + cat.slug;
      if (seen.has(key)) continue;
      seen.add(key);
      skills.push({
        slug: skill.slug,
        description: skill.description,
        url: skill.url || null,
        has_skill_md: skill.has_skill_md || false,
        category: cat.category,
        category_slug: cat.slug
      });
    }
  }

  await db.collection('skills').deleteMany({});
  // Insert in batches of 1000
  const BATCH = 1000;
  for (let i = 0; i < skills.length; i += BATCH) {
    const batch = skills.slice(i, i + BATCH);
    await db.collection('skills').insertMany(batch);
    process.stdout.write('  Inserted ' + Math.min(i + BATCH, skills.length) + '/' + skills.length + ' skills\r');
  }
  console.log('');

  // --- Create indexes ---
  // Drop old unique slug index if it exists (slugs can repeat across categories)
  try { await db.collection('skills').dropIndex('slug_1'); } catch (_) {}
  await db.collection('skills').createIndex({ slug: 1, category_slug: 1 }, { unique: true });
  await db.collection('skills').createIndex({ category_slug: 1 });
  await db.collection('skills').createIndex(
    { description: 'text', slug: 'text' },
    { weights: { description: 10, slug: 5 }, name: 'skills_text_search' }
  );
  await db.collection('categories').createIndex({ slug: 1 }, { unique: true });
  console.log('  Indexes created');

  await client.close();
  console.log('Done! Loaded ' + categories.length + ' categories and ' + skills.length + ' skills.');
}

load().catch(err => { console.error('ERROR:', err.message); process.exit(1); });
" "$MONGO_URI" "$DB_NAME" "$SKILLS_JSON"
