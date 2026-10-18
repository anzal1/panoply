#!/usr/bin/env node
// Launcher for the Claude Desktop bundle. Claude Desktop unpacks bundle files without the execute
// permission, so a `binary` server cannot start (EACCES). Node ships inside Claude Desktop, so the bundle
// is a `node` server whose only job is to set the permission on the real program and run it as
// `panoply mcp`. The program talks to Claude over this process's stdin and stdout, which are handed
// over unchanged. The exit code and the stop signals are passed on.
'use strict';
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

const exe = path.join(__dirname, process.platform === 'win32' ? 'panoply.exe' : 'panoply');
try {
  fs.chmodSync(exe, 0o755);
} catch (e) {
  // Windows has no execute bit. Anywhere else a failure here means the start below fails too, with a clearer line.
  if (process.platform !== 'win32') console.error(`panoply: could not make ${exe} executable: ${e.message}`);
}

const child = spawn(exe, ['mcp', ...process.argv.slice(2)], { stdio: 'inherit', env: process.env });
const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'];
for (const sig of signals) process.on(sig, () => child.kill(sig));
child.on('error', (e) => {
  console.error(`panoply: could not start ${exe}: ${e.message}`);
  process.exit(1);
});
child.on('exit', (code, signal) => {
  if (signal) {
    for (const sig of signals) process.removeAllListeners(sig);
    process.kill(process.pid, signal);
  } else {
    process.exit(code === null ? 1 : code);
  }
});
