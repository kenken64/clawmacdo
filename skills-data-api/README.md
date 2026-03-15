# Skills Data API

Node.js REST API serving OpenClaw skills data from MongoDB, with keyword search, file downloads, and batch uploads. Railway-ready with Dockerfile.

## Quick Start

```bash
# Install dependencies
npm install

# Load skills data into MongoDB
bash load-mongo.sh

# Start the server
npm start
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MONGO_PUBLIC_URL` | (hardcoded fallback) | MongoDB connection string (Railway sets this) |
| `MONGO_URI` | (hardcoded fallback) | Alternative MongoDB connection string |
| `DB_NAME` | `clawmacdo` | MongoDB database name |
| `PORT` | `3100` | Server port |
| `SKILLS_DIR` | `/skills` (Docker) / `./skills` (local) | Path to SKILL.md files on disk or volume |

## API Endpoints

### Categories

#### `GET /api/categories`

List all categories with skill counts.

```bash
curl http://localhost:3100/api/categories
```

```json
{
  "count": 30,
  "categories": [
    { "category": "AI & LLMs", "slug": "ai-and-llms", "skill_count": 184 },
    ...
  ]
}
```

#### `GET /api/categories/:slug`

Get all skills in a category (paginated).

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `page` | query | `1` | Page number |
| `limit` | query | `50` | Results per page (max 100) |

```bash
curl "http://localhost:3100/api/categories/ai-and-llms?page=1&limit=10"
```

```json
{
  "category_slug": "ai-and-llms",
  "total": 184,
  "page": 1,
  "limit": 10,
  "total_pages": 19,
  "skills": [...]
}
```

#### `GET /api/categories/:slug/files`

List skills in a category, verifying SKILL.md file presence on the data volume. Useful for checking which files are actually available.

```bash
curl "http://localhost:3100/api/categories/ai-and-llms/files?page=1&limit=5"
```

```json
{
  "category_slug": "ai-and-llms",
  "total": 184,
  "page": 1,
  "limit": 5,
  "total_pages": 37,
  "on_disk_this_page": 4,
  "skills": [
    { "slug": "4claw", "description": "...", "has_skill_md": true, "file_on_disk": true },
    { "slug": "some-missing", "description": "...", "has_skill_md": false, "file_on_disk": false }
  ]
}
```

### Skills

#### `GET /api/skills`

List or search skills with keyword search on description (uses MongoDB text index). Paginated.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `q` | query | — | Search keyword (searches description + slug) |
| `page` | query | `1` | Page number |
| `limit` | query | `50` | Results per page (max 100) |

```bash
# Search for "blockchain"
curl "http://localhost:3100/api/skills?q=blockchain&limit=5"
```

```json
{
  "total": 42,
  "page": 1,
  "limit": 5,
  "total_pages": 9,
  "query": "blockchain",
  "skills": [
    {
      "slug": "argus",
      "description": "Blockchain intelligence & AI security",
      "category": "AI & LLMs",
      "category_slug": "ai-and-llms",
      "score": 6.25
    }
  ]
}
```

#### `GET /api/skills/:slug`

Get skill detail by slug. Includes the full SKILL.md content if available on disk.

```bash
curl http://localhost:3100/api/skills/aegis-security
```

```json
{
  "slug": "aegis-security",
  "description": "Blockchain security API for AI agents",
  "url": "https://raw.githubusercontent.com/...",
  "has_skill_md": true,
  "category": "AI & LLMs",
  "category_slug": "ai-and-llms",
  "skill_md": "---\nname: aegis-security\n..."
}
```

#### `GET /api/skills/:slug/download`

Download the raw SKILL.md file for a skill from the data volume.

```bash
curl -O http://localhost:3100/api/skills/aegis-security/download
```

Returns `text/markdown` with `Content-Disposition: attachment` header. Returns 404 JSON if file not found.

#### `POST /api/skills/upload`

Batch upload SKILL.md files to the data volume. Multipart form-data.

| Field | Type | Description |
|-------|------|-------------|
| `files` | file(s) | One or more SKILL.md files (max 500, 1MB each) |
| `slugs` | text | Optional JSON array of slug names matching file order |

```bash
# Upload with slug in filename path
curl -X POST http://localhost:3100/api/skills/upload \
  -F "files=@skills/my-skill/SKILL.md;filename=my-skill/SKILL.md"

# Batch upload with explicit slugs
curl -X POST http://localhost:3100/api/skills/upload \
  -F "files=@file1.md" -F "files=@file2.md" \
  -F 'slugs=["skill-one","skill-two"]'
```

```json
{
  "uploaded": 2,
  "skipped": 0,
  "total": 2,
  "results": [
    { "slug": "skill-one", "status": "uploaded", "size": 1234 },
    { "slug": "skill-two", "status": "uploaded", "size": 5678 }
  ]
}
```

### Health

#### `GET /api/health`

```bash
curl http://localhost:3100/api/health
```

```json
{ "status": "ok" }
```

## Docker / Railway

```bash
docker build -t skills-data-api .
docker run -p 3100:3100 \
  -e MONGO_PUBLIC_URL="mongodb://..." \
  -v /path/to/skills:/skills \
  skills-data-api
```

On Railway, set the `SKILLS_DIR` env var to your mounted volume path (e.g. `/skills`), and `MONGO_PUBLIC_URL` is auto-injected from the MongoDB service.

## MongoDB Load Script

```bash
# Uses default connection string
bash load-mongo.sh

# Custom connection
bash load-mongo.sh "mongodb://user:pass@host:port/" "mydb"
```

Loads `skills.json` into two collections:
- **categories** — 30 categories with slug and skill_count
- **skills** — 5,364 flattened skill documents with text search index on description + slug

## Data

- `skills.json` — Consolidated JSON with 30 categories, 5,368 skills
- `skills/` — Directory of `<slug>/SKILL.md` files (5,019 downloaded)
