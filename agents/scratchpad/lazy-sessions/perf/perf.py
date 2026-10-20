#!/usr/bin/env python3
"""Noninteractive real-data PTY startup, idle CPU, keyboard and mouse probe."""
import argparse, codecs, fcntl, json, os, pathlib, pty, select, signal, struct, subprocess, termios, time
import pyte
p=argparse.ArgumentParser(); p.add_argument('binary'); p.add_argument('label'); p.add_argument('--sample',action='store_true'); a=p.parse_args()
out=pathlib.Path(__file__).resolve().parent; binary=str(pathlib.Path(a.binary).resolve())
screen=pyte.Screen(200,50); stream=pyte.Stream(screen); decoder=codecs.getincrementaldecoder('utf-8')('replace')
raw=bytearray(); replied=False; frame=None; samples=[]; actions=[]; sampler=None
start=time.monotonic(); deadline=start+120; pid,fd=pty.fork()
if pid==0:
 os.environ['TERM']='xterm-256color'; os.execv(binary,[binary])
fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',50,200,0,0))
def read_once(wait):
 global replied,frame
 if not select.select([fd],[],[],max(0,wait))[0]: return
 chunk=os.read(fd,1048576)
 if not chunk: raise RuntimeError('PTY closed')
 raw.extend(chunk); stream.feed(decoder.decode(chunk))
 if not replied and b'\x1b[c' in raw:
  os.write(fd,b'\x1b]11;rgb:1111/1111/1111\x1b\\\x1b[?62;22;52c'); replied=True
 if frame is None and b'Active' in raw and b'Inactive' in raw: frame=time.monotonic()-start

def wait_for(predicate,timeout=90):
 until=min(deadline,time.monotonic()+timeout)
 while time.monotonic()<until:
  if predicate(): return
  read_once(min(.02,until-time.monotonic()))
 raise TimeoutError('Screen assertion timed out')

def drain(seconds):
 until=min(deadline,time.monotonic()+seconds)
 while time.monotonic()<until: read_once(min(.02,until-time.monotonic()))
 if time.monotonic()>=deadline: raise TimeoutError("Overall 120 second deadline")

def display():
 return [''.join(screen.buffer[y][x].data or '' for x in range(200)) for y in range(50)]
def shot(name): (out/(a.label+'-'+name+'.txt')).write_text('\n'.join(display())+'\n')
def region(x0,x1):
 return tuple(tuple((screen.buffer[y][x].data,screen.buffer[y][x].fg,screen.buffer[y][x].bg,screen.buffer[y][x].reverse) for x in range(x0,x1)) for y in range(2,30))
def action(name,data,predicate):
 begin=time.monotonic(); os.write(fd,data)
 wait_for(predicate)
 elapsed=time.monotonic()-begin
 actions.append({'action':name,'seconds':elapsed}); checkpoint(); drain(.05); shot(name)
def checkpoint():
 (out/(a.label+'.partial.json')).write_text(json.dumps({'startup_seconds':frame,'idle_cpu':samples,'actions':actions},indent=2)+'\n')
def click(x,y): return f'\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m'.encode()
try:
 wait_for(lambda:frame is not None,180)
 wait_for(lambda:'help' in display()[-1],90)
 drain(.1); full=time.monotonic()-start; shot('startup')
 idle=time.monotonic()
 if a.sample:
  sampler=subprocess.Popen(['/usr/bin/sample',str(pid),'3','1','-file',str(out/(a.label+'.idle.sample.txt'))],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 for at in [2,4,6,8,10]:
  drain(max(0,idle+at-time.monotonic()))
  value=subprocess.check_output(['ps','-p',str(pid),'-o','%cpu=,time='],text=True).strip()
  samples.append({'at_seconds':time.monotonic()-idle,'ps':value}); checkpoint()
 # The terminal cursor is parked on the selected task row after each rendered frame.
 before=(screen.cursor.x,screen.cursor.y); snapshot=region(50,102)
 action('key-down',b'\x1b[B',lambda:(screen.cursor.x,screen.cursor.y)!=before and region(50,102)!=snapshot)
 drain(.1)
 task_rows=[y for y in range(2,30) if any(c in display()[y][:50] for c in ['●','○','◐','◉','◌','◍','◒','◑','◔','◕','⏳'])]
 target=next(y for y in task_rows if y!=screen.cursor.y)
 snapshot=region(50,102)
 action('click-task',click(15,target+1),lambda:region(50,102)!=snapshot)
 drain(.2)
 event_rows=[y for y in range(3,28) if display()[y][104:195].strip() and '─' not in display()[y][104:195]]
 target=event_rows[min(2,len(event_rows)-1)]
 snapshot=region(50,200)
 action('click-event',click(125,target+1),lambda:region(50,200)!=snapshot)
 result={'passed':True,'binary':binary,'startup_seconds':frame,'full_frame_seconds':full,'idle_cpu':samples,'actions':actions,'pid':pid,'real_home':os.path.expanduser('~')}
except Exception as e:
 result={'passed':False,'error':repr(e),'startup_seconds':frame,'idle_cpu':samples,'actions':actions,'pid':pid}; shot('failure')
finally:
 (out/(a.label+'.perf.ansi')).write_bytes(raw)
 os.close(fd); os.kill(pid,signal.SIGKILL); os.waitpid(pid,0)
 if sampler: sampler.wait(timeout=10)
(out/(a.label+'.perf.json')).write_text(json.dumps(result,indent=2)+'\n'); print(json.dumps(result)); raise SystemExit(0 if result['passed'] else 1)
