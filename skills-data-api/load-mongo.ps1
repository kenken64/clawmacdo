#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$MongoUri = $(if ($env:MONGO_URI) { $env:MONGO_URI } else { 'mongodb://mongo:xPXhguLfGwQFtcXLAgpihPdrwUtOBcYN@yamabiko.proxy.rlwy.net:52355/' }),
    [string]$DbName = $(if ($env:DB_NAME) { $env:DB_NAME } else { 'clawmacdo' })
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = $PSScriptRoot
$skillsJson = Join-Path $scriptDir 'skills.json'
if (-not (Test-Path -LiteralPath $skillsJson)) {
    throw "ERROR: $skillsJson not found"
}
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    throw 'ERROR: node is required'
}

$maskedUri = if ($MongoUri -match '@') { ($MongoUri -replace '@.*$', '@***') } else { $MongoUri }
Write-Host 'Loading skills data into MongoDB...'
Write-Host "  URI: $maskedUri"
Write-Host "  DB:  $DbName"
Write-Host "  Source: $skillsJson"

$script = @'
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

  const categories = data.map(c => ({ category: c.category, slug: c.slug, skill_count: c.skill_count }));
  await db.collection('categories').deleteMany({});
  if (categories.length > 0) {
    await db.collection('categories').insertMany(categories);
  }
  console.log('  Inserted ' + categories.length + ' categories');

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
  const BATCH = 1000;
  for (let i = 0; i < skills.length; i += BATCH) {
    const batch = skills.slice(i, i + BATCH);
    await db.collection('skills').insertMany(batch);
    process.stdout.write('  Inserted ' + Math.min(i + BATCH, skills.length) + '/' + skills.length + ' skills\r');
  }
  console.log('');

  try { await db.collection('skills').dropIndex('slug_1'); } catch (_) {}
  await db.collection('skills').createIndex({ slug: 1, category_slug: 1 }, { unique: true });
  await db.collection('skills').createIndex({ category_slug: 1 });
  await db.collection('skills').createIndex({ description: 'text', slug: 'text' }, { weights: { description: 10, slug: 5 }, name: 'skills_text_search' });
  await db.collection('categories').createIndex({ slug: 1 }, { unique: true });
  console.log('  Indexes created');

  await client.close();
  console.log('Done! Loaded ' + categories.length + ' categories and ' + skills.length + ' skills.');
}

load().catch(err => { console.error('ERROR:', err.message); process.exit(1); });
'@

& node --no-warnings -e $script $MongoUri $DbName $skillsJson
if ($LASTEXITCODE -ne 0) {
    throw 'Node MongoDB load script failed.'
}