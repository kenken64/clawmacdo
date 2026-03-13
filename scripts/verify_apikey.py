import sys,sqlite3,hashlib
key=sys.argv[1]
db=sys.argv[2]
conn=sqlite3.connect(db)
c=conn.cursor()
for row in c.execute('select hash,salt,revoked from api_keys'):
    h=row[0]
    s=row[1]
    revoked=row[2]
    if revoked:
        continue
    calc=hashlib.pbkdf2_hmac('sha256', key.encode(), s, 200000)
    if calc==h:
        print('OK')
        sys.exit(0)
print('NO')
sys.exit(2)
