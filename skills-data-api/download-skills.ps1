#!/usr/bin/env pwsh
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = $PSScriptRoot
$repo = 'VoltAgent/awesome-openclaw-skills'
$dir = 'categories'
$skillsDir = Join-Path $scriptDir 'skills'
$updatedJson = Join-Path $scriptDir 'skills.json'

New-Item -ItemType Directory -Path $skillsDir -Force | Out-Null

Write-Host '=== Phase 1: Extracting skill URLs from category files ==='
$files = gh api "repos/$repo/contents/$dir" --jq '.[] | select(.name | endswith(".md")) | .name'
if ($LASTEXITCODE -ne 0) {
    throw 'gh api failed while listing category files.'
}

$entries = New-Object System.Collections.Generic.List[object]
foreach ($file in @($files -split "`r?`n" | Where-Object { $_ })) {
    $rawUrl = "https://raw.githubusercontent.com/$repo/main/$dir/$file"
    $content = (Invoke-WebRequest -Uri $rawUrl).Content
    $category = ([regex]::Match($content, '(?m)^#\s+(.+)$')).Groups[1].Value.Trim()
    $categorySlug = [System.IO.Path]::GetFileNameWithoutExtension($file)

    foreach ($line in @($content -split "`r?`n")) {
        $match = [regex]::Match($line, '^- \[([^\]]+)\]\(([^)]+)\) - (.+)$')
        if (-not $match.Success) { continue }

        $slug = $match.Groups[1].Value.Trim()
        $url = $match.Groups[2].Value.Trim()
        $description = $match.Groups[3].Value.TrimEnd('.').Trim()
        $rawSkillUrl = $url -replace 'github.com/([^/]+/[^/]+)/tree/', 'raw.githubusercontent.com/$1/'
        if ($rawSkillUrl -notmatch 'SKILL\.md$') {
            $rawSkillUrl = $rawSkillUrl.TrimEnd('/') + '/SKILL.md'
        }

        $entries.Add([pscustomobject]@{
            slug         = $slug
            url          = $rawSkillUrl
            category_slug = $categorySlug
            description  = $description
        })
    }

    Write-Host "  ${category}: extracted URLs"
}

Write-Host ''
Write-Host "=== Phase 2: Downloading $($entries.Count) SKILL.md files ==="

$downloaded = 0
$skipped = 0
$failed = 0

foreach ($entry in $entries) {
    $destinationDir = Join-Path $skillsDir $entry.slug
    $destinationFile = Join-Path $destinationDir 'SKILL.md'
    if ((Test-Path -LiteralPath $destinationFile) -and (Get-Item -LiteralPath $destinationFile).Length -gt 0) {
        $skipped += 1
        continue
    }

    New-Item -ItemType Directory -Path $destinationDir -Force | Out-Null
    try {
        Invoke-WebRequest -Uri $entry.url -OutFile $destinationFile
        $firstLine = Get-Content -LiteralPath $destinationFile -TotalCount 1 -ErrorAction SilentlyContinue
        if ($firstLine -match '<!DOCTYPE|<html|404') {
            Remove-Item -LiteralPath $destinationFile -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $destinationDir -Force -ErrorAction SilentlyContinue
            $failed += 1
        }
        else {
            $downloaded += 1
        }
    }
    catch {
        Remove-Item -LiteralPath $destinationFile -Force -ErrorAction SilentlyContinue
        $failed += 1
    }
}

Write-Host ''
Write-Host '=== Phase 3: Building updated skills.json with URLs ==='

$existing = Get-Content -LiteralPath $updatedJson -Raw | ConvertFrom-Json
$byCategory = @{}
foreach ($entry in $entries) {
    if (-not $byCategory.ContainsKey($entry.category_slug)) {
        $byCategory[$entry.category_slug] = New-Object System.Collections.Generic.List[object]
    }
    $hasFile = Test-Path -LiteralPath (Join-Path (Join-Path $skillsDir $entry.slug) 'SKILL.md')
    $byCategory[$entry.category_slug].Add([ordered]@{
        slug         = $entry.slug
        description  = $entry.description
        url          = $entry.url
        has_skill_md = $hasFile
    })
}

$output = foreach ($category in $existing) {
    $skills = if ($byCategory.ContainsKey($category.slug)) { @($byCategory[$category.slug]) } else { @($category.skills) }
    [ordered]@{
        category    = $category.category
        slug        = $category.slug
        skill_count = $skills.Count
        skills      = $skills
    }
}

Set-Content -LiteralPath $updatedJson -Value ($output | ConvertTo-Json -Depth 20) -Encoding utf8

$actualFiles = @(Get-ChildItem -LiteralPath $skillsDir -Recurse -Filter SKILL.md -ErrorAction SilentlyContinue).Count
Write-Host ''
Write-Host '=== Done ==='
Write-Host "Downloaded: $downloaded"
Write-Host "Skipped: $skipped"
Write-Host "Failed: $failed"
Write-Host "Downloaded SKILL.md files: $actualFiles"
Write-Host "Location: $skillsDir/<slug>/SKILL.md"