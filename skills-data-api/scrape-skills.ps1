#!/usr/bin/env pwsh
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = $PSScriptRoot
$repo = 'VoltAgent/awesome-openclaw-skills'
$dir = 'categories'
$out = Join-Path $scriptDir 'skills.json'

Write-Host "Fetching category file list from $repo..."
$files = gh api "repos/$repo/contents/$dir" --jq '.[] | select(.name | endswith(".md")) | .name'
if ($LASTEXITCODE -ne 0) {
    throw 'gh api failed while listing category files.'
}

Write-Host 'Downloading and parsing markdown files...'

$categories = New-Object System.Collections.Generic.List[object]
foreach ($file in @($files -split "`r?`n" | Where-Object { $_ })) {
    $rawUrl = "https://raw.githubusercontent.com/$repo/main/$dir/$file"
    $content = (Invoke-WebRequest -Uri $rawUrl).Content
    $category = ([regex]::Match($content, '(?m)^#\s+(.+)$')).Groups[1].Value.Trim()
    $categorySlug = [System.IO.Path]::GetFileNameWithoutExtension($file)
    $skillCountMatch = [regex]::Match($content, '\*\*([0-9]+) skills?\*\*')
    $skillCount = if ($skillCountMatch.Success) { [int]$skillCountMatch.Groups[1].Value } else { 0 }

    Write-Host "  $category ($skillCount skills)"

    $skills = New-Object System.Collections.Generic.List[object]
    foreach ($line in @($content -split "`r?`n")) {
        $match = [regex]::Match($line, '^- \[([^\]]+)\]\(([^)]+)\) - (.+)$')
        if (-not $match.Success) { continue }
        $skills.Add([ordered]@{
            slug        = $match.Groups[1].Value.Trim()
            description = $match.Groups[3].Value.TrimEnd('.').Trim()
        })
    }

    $categories.Add([ordered]@{
        category    = $category
        slug        = $categorySlug
        skill_count = $skillCount
        skills      = @($skills)
    })
}

Set-Content -LiteralPath $out -Value ($categories | ConvertTo-Json -Depth 20) -Encoding utf8

try {
    Get-Content -LiteralPath $out -Raw | ConvertFrom-Json | Out-Null
    Write-Host "Valid JSON written to $out"
}
catch {
    Write-Warning 'JSON validation failed'
}

$total = ($categories | ForEach-Object { $_.skills.Count } | Measure-Object -Sum).Sum
Write-Host "Total: $($categories.Count) categories, $total skills"