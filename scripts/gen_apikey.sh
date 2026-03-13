#!/usr/bin/env bash
set -euo pipefail
name=${1:-"default"}
db=${2:-/root/.openclaw/workspace/clawmacdo/keys.db}
mkdir -p $(dirname "$db")
key=$(openssl rand -hex 32)
# create table if not exists and insert hashed key using python pbkdf2
python3 - <<PY
import sqlite3,hashlib,os
key='$key'
name='$name'
db='$db'
salt=os.urandom(16)
hash=hashlib.pbkdf2_hmac('sha256', key.encode(), salt, 200000)
conn=sqlite3.connect(db)
c=conn.cursor()
c.execute('''CREATE TABLE IF NOT EXISTS api_keys (id INTEGER PRIMARY KEY, name TEXT, hash BLOB, salt BLOB, created_at TEXT, revoked INTEGER DEFAULT 0)''')
c.execute('INSERT INTO api_keys (name,hash,salt,created_at) VALUES (?,?,?,datetime("now"))', (name, hash, salt))
conn.commit()
conn.close()
print(key)
PY

echo "Generated API key for '$name' and stored hash in $db. The clear key is printed above — store it securely."
