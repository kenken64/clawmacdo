import React from 'react';
export default function SecurityTab(){
  return (
    <div>
      <h2>Security Scan</h2>
      <p>Start a scan for Ubuntu, macOS, or Windows hosts. (MVP)</p>
      <button onClick={()=>alert('Scan start: not yet wired')}>Start Scan</button>
    </div>
  )
}
