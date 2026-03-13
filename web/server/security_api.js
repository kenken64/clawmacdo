const express = require('express');
const { spawn } = require('child_process');
const app = express();
app.use(express.json());
const jobs = {};
let idCounter = 1;
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
