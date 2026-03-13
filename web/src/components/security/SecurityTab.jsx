import React, {useState} from 'react';
export default function SecurityTab(){
  const [job, setJob]=useState(null);
  const start=async()=>{
    const r=await fetch('/api/security/scan',{method:'POST',headers:{'content-type':'application/json'}});
    const j=await r.json(); setJob(j);
  };
  const status=async()=>{ if(!job) return; const r=await fetch(`/api/security/scan/${job.jobId}/status`); const s=await r.json(); setJob(s); };
  return (
    <div>
      <h2>Security Scan</h2>
      <button onClick={start}>Start Scan</button>
      <button onClick={status} disabled={!job}>Check Status</button>
      {job && <pre>{JSON.stringify(job,null,2)}</pre>}
    </div>
  )
}
