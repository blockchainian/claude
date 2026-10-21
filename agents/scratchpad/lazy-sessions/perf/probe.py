#!/usr/bin/env python3
"""Real HOME PTY probe: exec -> first frame containing Active and Inactive."""
import argparse, fcntl, json, os, pathlib, pty, select, signal, struct, subprocess, termios, time
p=argparse.ArgumentParser(); p.add_argument('binary'); p.add_argument('label'); p.add_argument('--sample',action='store_true'); a=p.parse_args()
out=pathlib.Path(__file__).resolve().parent
binary=str(pathlib.Path(a.binary).resolve())
start=time.monotonic(); pid,fd=pty.fork()
if pid==0:
    os.environ['TERM']='xterm-256color'
    os.execv(binary,[binary])
fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',50,200,0,0))
raw=bytearray(); frame=None; sample=None; replied=False; deadline=start+180
try:
    while time.monotonic()<deadline:
        if a.sample and sample is None and time.monotonic()-start>3:
            sample=subprocess.Popen(['/usr/bin/sample',str(pid),'5','1','-file',str(out/(a.label+'.sample.txt'))],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if not select.select([fd],[],[],0.02)[0]: continue
        try: chunk=os.read(fd,1048576)
        except OSError: break
        if not chunk: break
        raw.extend(chunk)
        if not replied and b'\x1b[c' in raw:
            os.write(fd,b'\x1b]11;rgb:1111/1111/1111\x1b\\\x1b[?62;22;52c'); replied=True
        if b'Active' in raw and b'Inactive' in raw:
            frame=time.monotonic()-start; break
finally:
    (out/(a.label+'.ansi')).write_bytes(raw)
    os.close(fd)
    os.kill(pid,signal.SIGKILL)
    os.waitpid(pid,0)
    if sample: sample.wait(timeout=15)
result={'binary':binary,'label':a.label,'first_content_seconds':frame,'bytes':len(raw),'terminal':'200x50 PTY; OSC11/DA1 replies','real_home':os.path.expanduser('~'),'overrides':{k:os.environ.get(k) for k in ['CLAUDE_CONFIG_DIR','CODEX_HOME','CODEX_MANAGER_STATE_DIR']},'passed':frame is not None}
(out/(a.label+'.json')).write_text(json.dumps(result,indent=2)+'\n'); print(json.dumps(result))
raise SystemExit(0 if frame is not None else 1)
