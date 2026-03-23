#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Name = 'default',
    [Parameter(Position = 1)]
    [string]$Db = '/root/.openclaw/workspace/clawmacdo/keys.db'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Python {
    param([string[]]$Arguments)

    if (Get-Command python3 -ErrorAction SilentlyContinue) {
        & python3 @Arguments
        return
    }
    if (Get-Command python -ErrorAction SilentlyContinue) {
        & python @Arguments
        return
    }
    if (Get-Command py -ErrorAction SilentlyContinue) {
        & py -3 @Arguments
        return
    }

    throw 'Python 3 is required to store API keys in SQLite.'
}

$dbDir = Split-Path -Parent $Db
if ($dbDir) {
    New-Item -ItemType Directory -Path $dbDir -Force | Out-Null
}

$bytes = New-Object byte[] 32
[System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
$key = ($bytes | ForEach-Object { $_.ToString('x2') }) -join ''

$pythonCode = @'
import hashlib
import os
import sqlite3
import sys

key = sys.argv[1]
name = sys.argv[2]
db = sys.argv[3]

salt = os.urandom(16)
digest = hashlib.pbkdf2_hmac('sha256', key.encode(), salt, 200000)

conn = sqlite3.connect(db)
cur = conn.cursor()
cur.execute("""CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY,
    name TEXT,
    hash BLOB,
    salt BLOB,
    created_at TEXT,
    revoked INTEGER DEFAULT 0
)""")
cur.execute(
    'INSERT INTO api_keys (name, hash, salt, created_at) VALUES (?, ?, ?, datetime("now"))',
    (name, digest, salt),
)
conn.commit()
conn.close()
print(key)
'@

Invoke-Python -Arguments @('-c', $pythonCode, $key, $Name, $Db)
Write-Host "Generated API key for '$Name' and stored hash in $Db. The clear key is printed above - store it securely."