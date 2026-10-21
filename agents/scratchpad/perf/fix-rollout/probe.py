#!/usr/bin/env python3
"""Reuse the original startup PTY and idle sample method on an explicit release binary."""
import argparse,fcntl,json,os,pathlib,pty,select,signal,struct,subprocess,termios,time
p=argparse.ArgumentParser(); p.add_argument('binary');p.add_argument('label');p.add_argument('--idle',action='store_true');a=p.parse_args()
out=pathlib.Path(__file__).resolve().parent;binary=str(pathlib.Path(a.binary).resolve())
start=time.monotonic();pid,fd=pty.fork()
if pid==0:
    os.environ['TERM']='xterm-256color';os.execv(binary,[binary])
fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',50,200,0,0))
raw=bytearray();replied=False;frame=None;sampler=None;samples=[];next_ps=4.;deadline=start+120
try:
    while time.monotonic()<deadline:
        if select.select([fd],[],[],.02)[0]:
            try: chunk=os.read(fd,1<<20)
            except OSError: break
            if not chunk:break
            raw.extend(chunk)
            if not replied and b'\x1b[c' in raw:
                os.write(fd,b'\x1b]11;rgb:1111/1111/1111\x1b\\\x1b[?62;22;52c');replied=True
            if frame is None and b'Active' in raw and b'Inactive' in raw:
                frame=time.monotonic()-start
                if a.idle: deadline=start+frame+26; next_ps=frame+4
                if not a.idle: break
        elapsed=time.monotonic()-start
        if a.idle and frame is not None and elapsed>=next_ps:
            value=subprocess.check_output(['ps','-p',str(pid),'-o','%cpu=,time=,rss='],text=True).split()
            samples.append(dict(at=elapsed,cpu=float(value[0]),time=value[1],rss_kib=int(value[2])))
            next_ps=elapsed+.5
        if a.idle and sampler is None and frame is not None and elapsed>=frame+6:
            sampler=subprocess.Popen(['/usr/bin/sample',str(pid),'10','1','-file',str(out/(a.label+'.sample.txt'))],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
finally:
    os.close(fd)
    try:os.kill(pid,signal.SIGKILL)
    except ProcessLookupError:pass
    os.waitpid(pid,0)
    if sampler:sampler.wait(timeout=15)
(out/(a.label+'.ansi')).write_bytes(raw)
def cpu_seconds(text):
    parts=text.split(':'); return sum(float(part)*60**i for i,part in enumerate(reversed(parts)))
rate=None
if len(samples)>1:rate=100*(cpu_seconds(samples[-1]['time'])-cpu_seconds(samples[0]['time']))/(samples[-1]['at']-samples[0]['at'])
result=dict(binary=binary,label=a.label,first_content_seconds=frame,passed=frame is not None and b'agents: Codex:' not in raw,idle_cpu_percent=rate,ps=samples,real_home=os.path.expanduser('~'),terminal='200x50 PTY; OSC11/DA1 replies',tmux=False,overrides={key:os.environ.get(key) for key in ['CODEX_HOME','CLAUDE_CONFIG_DIR','CODEX_MANAGER_STATE_DIR','NO_COLOR']})
(out/(a.label+'.json')).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({key:result[key] for key in ['label','first_content_seconds','passed','idle_cpu_percent']}));raise SystemExit(0 if result['passed'] else 1)
