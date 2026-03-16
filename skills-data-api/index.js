const express = require("express");
const { MongoClient } = require("mongodb");
const multer = require("multer");
const fs = require("fs");
const path = require("path");

const MONGO_URI =
  process.env.MONGO_PUBLIC_URL ||
  process.env.MONGO_URI ||
  "mongodb://mongo:xPXhguLfGwQFtcXLAgpihPdrwUtOBcYN@yamabiko.proxy.rlwy.net:52355/";
const DB_NAME = process.env.DB_NAME || "clawmacdo";
const PORT = process.env.PORT || 3100;
const SKILLS_DIR = process.env.SKILLS_DIR || path.join(__dirname, "skills");

let db;

// Multer config: accept files with field name "files", store in memory
const upload = multer({ storage: multer.memoryStorage(), limits: { fileSize: 1024 * 1024 } });

async function connectDB() {
  const client = new MongoClient(MONGO_URI);
  await client.connect();
  db = client.db(DB_NAME);
  console.log(`Connected to MongoDB (${DB_NAME})`);
  return client;
}

const app = express();

// --- GET /api/categories ---
// List all categories with skill counts
app.get("/api/categories", async (_req, res) => {
  try {
    const categories = await db
      .collection("categories")
      .find({}, { projection: { _id: 0 } })
      .sort({ category: 1 })
      .toArray();
    res.json({ count: categories.length, categories });
  } catch (err) {
    res.status(500).json({ error: err.message });
  }
});

// --- GET /api/categories/:slug ---
// Get all skills in a category
app.get("/api/categories/:slug", async (req, res) => {
  try {
    const page = Math.max(1, parseInt(req.query.page) || 1);
    const limit = Math.min(100, Math.max(1, parseInt(req.query.limit) || 50));
    const skip = (page - 1) * limit;

    const filter = { category_slug: req.params.slug };
    const [skills, total] = await Promise.all([
      db
        .collection("skills")
        .find(filter, { projection: { _id: 0 } })
        .sort({ slug: 1 })
        .skip(skip)
        .limit(limit)
        .toArray(),
      db.collection("skills").countDocuments(filter),
    ]);

    if (total === 0) {
      return res.status(404).json({ error: "Category not found" });
    }
    const total_pages = Math.ceil(total / limit);
    res.json({ category_slug: req.params.slug, total, page, limit, total_pages, skills });
  } catch (err) {
    res.status(500).json({ error: err.message });
  }
});

// --- GET /api/categories/:slug/files ---
// List skills in a category, verifying SKILL.md presence on the data volume.
// Paginated (default 50 per page).
app.get("/api/categories/:slug/files", async (req, res) => {
  try {
    const page = Math.max(1, parseInt(req.query.page) || 1);
    const limit = Math.min(100, Math.max(1, parseInt(req.query.limit) || 50));
    const skip = (page - 1) * limit;

    const filter = { category_slug: req.params.slug };
    const [skills, total] = await Promise.all([
      db
        .collection("skills")
        .find(filter, { projection: { _id: 0, slug: 1, description: 1, has_skill_md: 1 } })
        .sort({ slug: 1 })
        .skip(skip)
        .limit(limit)
        .toArray(),
      db.collection("skills").countDocuments(filter),
    ]);

    if (total === 0) {
      return res.status(404).json({ error: "Category not found" });
    }

    // Check actual file presence on disk
    const results = skills.map((s) => {
      const mdPath = path.join(SKILLS_DIR, s.slug, "SKILL.md");
      return {
        slug: s.slug,
        description: s.description,
        has_skill_md: s.has_skill_md,
        file_on_disk: fs.existsSync(mdPath),
      };
    });

    const total_pages = Math.ceil(total / limit);
    const on_disk = results.filter((r) => r.file_on_disk).length;
    res.json({
      category_slug: req.params.slug,
      total,
      page,
      limit,
      total_pages,
      on_disk_this_page: on_disk,
      skills: results,
    });
  } catch (err) {
    res.status(500).json({ error: err.message });
  }
});

// --- GET /api/skills ---
// List / search skills. ?q=keyword searches description (text index).
// Supports pagination with ?page=1&limit=50
app.get("/api/skills", async (req, res) => {
  try {
    const page = Math.max(1, parseInt(req.query.page) || 1);
    const limit = Math.min(100, Math.max(1, parseInt(req.query.limit) || 50));
    const skip = (page - 1) * limit;
    const q = (req.query.q || "").trim();

    let filter = {};
    let sort = { slug: 1 };

    if (q) {
      filter = { $text: { $search: q } };
      sort = { score: { $meta: "textScore" }, slug: 1 };
    }

    const projection = q
      ? { _id: 0, score: { $meta: "textScore" } }
      : { _id: 0 };

    const [skills, total] = await Promise.all([
      db
        .collection("skills")
        .find(filter, { projection })
        .sort(sort)
        .skip(skip)
        .limit(limit)
        .toArray(),
      db.collection("skills").countDocuments(filter),
    ]);

    const total_pages = Math.ceil(total / limit);
    res.json({ total, page, limit, total_pages, query: q || undefined, skills });
  } catch (err) {
    res.status(500).json({ error: err.message });
  }
});

// --- GET /api/skills/:slug/download ---
// Download the raw SKILL.md file for a given slug from the data volume
app.get("/api/skills/:slug/download", (_req, res) => {
  const mdPath = path.join(SKILLS_DIR, _req.params.slug, "SKILL.md");
  if (!fs.existsSync(mdPath)) {
    return res.status(404).json({ error: "SKILL.md not found for this slug" });
  }
  res.setHeader("Content-Type", "text/markdown; charset=utf-8");
  res.setHeader(
    "Content-Disposition",
    `attachment; filename="${_req.params.slug}-SKILL.md"`
  );
  fs.createReadStream(mdPath).pipe(res);
});

// --- GET /api/skills/:slug ---
// Get skill(s) by slug (with SKILL.md content if available on disk).
// A slug may appear in multiple categories; returns all matches.
app.get("/api/skills/:slug", async (req, res) => {
  try {
    const skills = await db
      .collection("skills")
      .find({ slug: req.params.slug }, { projection: { _id: 0 } })
      .toArray();

    if (skills.length === 0) {
      return res.status(404).json({ error: "Skill not found" });
    }

    // Try to read local SKILL.md
    const mdPath = path.join(SKILLS_DIR, req.params.slug, "SKILL.md");
    let skill_md = null;
    if (fs.existsSync(mdPath)) {
      skill_md = fs.readFileSync(mdPath, "utf-8");
    }

    if (skills.length === 1) {
      return res.json({ ...skills[0], skill_md });
    }
    res.json({ slug: req.params.slug, skill_md, entries: skills });
  } catch (err) {
    res.status(500).json({ error: err.message });
  }
});

// --- POST /api/skills/upload ---
// Batch upload SKILL.md files. Each file's field name is "files".
// The slug is derived from the original filename path: <slug>/SKILL.md
// or from a JSON body field "slugs" (array matching file order).
// Expects multipart/form-data with:
//   files: one or more SKILL.md files (originalname should be "<slug>/SKILL.md" or "SKILL.md")
//   slugs: (optional) JSON array of slug names matching the files order
app.post("/api/skills/upload", upload.array("files", 500), async (req, res) => {
  try {
    if (!req.files || req.files.length === 0) {
      return res.status(400).json({ error: "No files uploaded" });
    }

    let slugs = [];
    if (req.body && req.body.slugs) {
      try {
        slugs = JSON.parse(req.body.slugs);
      } catch (_) {
        slugs = [];
      }
    }

    const results = [];
    for (let i = 0; i < req.files.length; i++) {
      const file = req.files[i];

      // Determine slug: from slugs array, or from originalname path
      let slug = slugs[i] || null;
      if (!slug) {
        // originalname might be "my-skill/SKILL.md" or just "SKILL.md"
        const parts = file.originalname.replace(/\\/g, "/").split("/");
        slug = parts.length >= 2 ? parts[parts.length - 2] : null;
      }

      if (!slug || slug === "SKILL.md") {
        results.push({
          file: file.originalname,
          status: "skipped",
          reason: "Could not determine slug",
        });
        continue;
      }

      // Write file to SKILLS_DIR/<slug>/SKILL.md
      const skillDir = path.join(SKILLS_DIR, slug);
      fs.mkdirSync(skillDir, { recursive: true });
      fs.writeFileSync(path.join(skillDir, "SKILL.md"), file.buffer);

      // Update MongoDB has_skill_md flag
      await db
        .collection("skills")
        .updateMany({ slug }, { $set: { has_skill_md: true } });

      results.push({ slug, status: "uploaded", size: file.buffer.length });
    }

    const uploaded = results.filter((r) => r.status === "uploaded").length;
    const skipped = results.filter((r) => r.status === "skipped").length;
    res.json({ uploaded, skipped, total: req.files.length, results });
  } catch (err) {
    res.status(500).json({ error: err.message });
  }
});

// ══════════════════════════════════════════════════════════════════════════
// User-specific (customer) SKILL.md endpoints
// Volume path: /skills/user/<deploymentId>/SKILL.md
// ══════════════════════════════════════════════════════════════════════════

const USER_SKILLS_DIR = path.join(SKILLS_DIR, "user");
const USER_SKILLS_API_KEY = process.env.USER_SKILLS_API_KEY || "";

// Middleware: require API key for user-skills endpoints
function requireUserSkillsApiKey(req, res, next) {
  if (!USER_SKILLS_API_KEY) {
    return res.status(503).json({ error: "USER_SKILLS_API_KEY not configured on server" });
  }
  const provided = req.headers["x-api-key"] || "";
  if (provided !== USER_SKILLS_API_KEY) {
    return res.status(401).json({ error: "Invalid or missing API key" });
  }
  next();
}

// --- POST /api/user-skills/:deploymentId ---
// Upload a customer-specific SKILL.md for a deployment.
// Accepts multipart/form-data with field "file" (single SKILL.md).
app.post(
  "/api/user-skills/:deploymentId",
  requireUserSkillsApiKey,
  upload.single("file"),
  async (req, res) => {
    try {
      const { deploymentId } = req.params;
      if (!deploymentId || deploymentId.includes("..") || deploymentId.includes("/")) {
        return res.status(400).json({ error: "Invalid deployment ID" });
      }

      if (!req.file) {
        return res.status(400).json({ error: "No file uploaded. Use field name 'file'." });
      }

      const destDir = path.join(USER_SKILLS_DIR, deploymentId);
      const destFile = path.join(destDir, "SKILL.md");

      // Backup existing file if present
      let backedUp = false;
      if (fs.existsSync(destFile)) {
        const ts = new Date().toISOString().replace(/[:.]/g, "-");
        const backupFile = path.join(destDir, `SKILL.md.backup-${ts}`);
        fs.copyFileSync(destFile, backupFile);
        backedUp = true;
      }

      fs.mkdirSync(destDir, { recursive: true });
      fs.writeFileSync(destFile, req.file.buffer);

      res.json({
        ok: true,
        deployment_id: deploymentId,
        size: req.file.buffer.length,
        backed_up: backedUp,
        path: `/skills/user/${deploymentId}/SKILL.md`,
      });
    } catch (err) {
      res.status(500).json({ error: err.message });
    }
  }
);

// --- GET /api/user-skills/:deploymentId ---
// Download the customer-specific SKILL.md for a deployment.
app.get("/api/user-skills/:deploymentId", requireUserSkillsApiKey, (req, res) => {
  const { deploymentId } = req.params;
  if (!deploymentId || deploymentId.includes("..") || deploymentId.includes("/")) {
    return res.status(400).json({ error: "Invalid deployment ID" });
  }

  const mdPath = path.join(USER_SKILLS_DIR, deploymentId, "SKILL.md");
  if (!fs.existsSync(mdPath)) {
    return res.status(404).json({
      error: "No SKILL.md found for this deployment",
      deployment_id: deploymentId,
    });
  }

  res.setHeader("Content-Type", "text/markdown; charset=utf-8");
  res.setHeader(
    "Content-Disposition",
    `attachment; filename="${deploymentId}-SKILL.md"`
  );
  fs.createReadStream(mdPath).pipe(res);
});

// --- DELETE /api/user-skills/:deploymentId ---
// Delete the customer-specific SKILL.md for a deployment.
app.delete("/api/user-skills/:deploymentId", requireUserSkillsApiKey, (req, res) => {
  const { deploymentId } = req.params;
  if (!deploymentId || deploymentId.includes("..") || deploymentId.includes("/")) {
    return res.status(400).json({ error: "Invalid deployment ID" });
  }

  const mdPath = path.join(USER_SKILLS_DIR, deploymentId, "SKILL.md");
  if (!fs.existsSync(mdPath)) {
    return res.status(404).json({
      error: "No SKILL.md found for this deployment",
      deployment_id: deploymentId,
    });
  }

  // Backup before delete
  const destDir = path.join(USER_SKILLS_DIR, deploymentId);
  const ts = new Date().toISOString().replace(/[:.]/g, "-");
  const backupFile = path.join(destDir, `SKILL.md.backup-${ts}`);
  fs.copyFileSync(mdPath, backupFile);
  fs.unlinkSync(mdPath);

  res.json({ ok: true, deployment_id: deploymentId, message: "SKILL.md deleted (backup retained)" });
});

// --- GET /api/user-skills/:deploymentId/info ---
// Get metadata about the user skill (size, last modified, backups).
app.get("/api/user-skills/:deploymentId/info", requireUserSkillsApiKey, (req, res) => {
  const { deploymentId } = req.params;
  if (!deploymentId || deploymentId.includes("..") || deploymentId.includes("/")) {
    return res.status(400).json({ error: "Invalid deployment ID" });
  }

  const destDir = path.join(USER_SKILLS_DIR, deploymentId);
  const mdPath = path.join(destDir, "SKILL.md");

  if (!fs.existsSync(mdPath)) {
    return res.status(404).json({
      error: "No SKILL.md found for this deployment",
      deployment_id: deploymentId,
    });
  }

  const stat = fs.statSync(mdPath);
  const files = fs.readdirSync(destDir);
  const backups = files.filter((f) => f.startsWith("SKILL.md.backup-"));

  res.json({
    deployment_id: deploymentId,
    size: stat.size,
    last_modified: stat.mtime.toISOString(),
    backup_count: backups.length,
    backups,
  });
});

// --- Health check ---
app.get("/api/health", async (_req, res) => {
  try {
    await db.command({ ping: 1 });
    res.json({ status: "ok" });
  } catch (err) {
    res.status(503).json({ status: "unhealthy", error: err.message });
  }
});

async function main() {
  const client = await connectDB();

  app.listen(PORT, () => {
    console.log(`Skills API listening on http://localhost:${PORT}`);
    console.log(`SKILLS_DIR: ${SKILLS_DIR}`);
    console.log("Endpoints:");
    console.log("  GET  /api/categories");
    console.log("  GET  /api/categories/:slug");
    console.log("  GET  /api/categories/:slug/files");
    console.log("  GET  /api/skills?q=keyword&page=1&limit=50");
    console.log("  GET  /api/skills/:slug");
    console.log("  GET  /api/skills/:slug/download");
    console.log("  POST /api/skills/upload  (multipart: files + slugs)");
    console.log("  POST /api/user-skills/:id  (upload user SKILL.md, x-api-key)");
    console.log("  GET  /api/user-skills/:id  (download user SKILL.md, x-api-key)");
    console.log("  DELETE /api/user-skills/:id  (delete user SKILL.md, x-api-key)");
    console.log("  GET  /api/user-skills/:id/info  (metadata, x-api-key)");
    console.log("  GET  /api/health");
  });

  process.on("SIGINT", async () => {
    await client.close();
    process.exit(0);
  });
}

main().catch((err) => {
  console.error("Failed to start:", err);
  process.exit(1);
});
