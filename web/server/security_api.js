const express = require('express');
const { spawn, execFileSync } = require('child_process');
const app = express();
app.use(express.json());
const jobs = {};
let idCounter = 1;
const KEY_DB = process.env.KEY_DB || '/root/.openclaw/workspace/clawmacdo/keys.db';

function checkApiKey(key){
  try{
    const out = execFileSync('python3',['scripts/verify_apikey.py', key, KEY_DB],{encoding:'utf8'}).trim();
    return out==='OK';
  }catch(e){ return false; }
}

// middleware
app.use((req,res,next)=>{
  if(req.path.startsWith('/api/security/')){
    const apiKey = req.header('x-api-key') || req.header('authorization') && req.header('authorization').split(' ').pop();
    if(!apiKey || !checkApiKey(apiKey)) return res.status(401).json({error:'invalid api key'});
  }
  next();
});

app.post('/api/security/scan', (req,res)=>{
  const id = String(idCounter++);
  const ts = Date.now();
  const out = `/tmp/openclaw_security_scan_${ts}.json`;
  jobs[id] = { status:'running', out };
  const child = spawn('/bin/bash',['scripts/run_all_scans.sh'],{cwd:process.cwd()});
  child.on('exit',(code)=>{ jobs[id].status = code===0? 'done':'error'; });
  res.json({jobId:id,status:'started',out});
});
app.get('/api/security/scan/:id/status',(req,res)=>{
  const j=jobs[req.params.id]; if(!j) return res.status(404).json({error:'not found'}); res.json(j);
});
app.get('/api/security/scan/:id/result',(req,res)=>{
  const j=jobs[req.params.id]; if(!j) return res.status(404).json({error:'not found'});
  if(j.status!=='done') return res.status(400).json({error:'not ready',status:j.status});
  res.sendFile(j.out);
});
if(require.main===module){ app.listen(3001,()=>console.log('security API running on 3001')); }
module.exports=app;
