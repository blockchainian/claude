import collections,json,re,statistics
from pathlib import Path
out=Path(__file__).resolve().parent
keys={'refresh':['App7refresh','Hub7refresh'],'tasks':['5Tasks7summary','5Tasks15refresh_summary'],'summary_records':['15summary_records'],'read_summary':['12read_summary'],'latest_activity':['latest_activity'],'events':['6events','19events_with_context'],'clone_subtree':['clone_subtree'],'parse':['from_trait']}
def sample(name):
    lines=(out/(name+'.sample.txt')).read_text().splitlines();start=lines.index('Call graph:');path=[];counts=collections.Counter();main=False;total=0
    for line in lines[start+1:]:
        if line.startswith('Total number in stack'):break
        if 'Thread_' in line and re.match(r'^    \d+',line):
            main='com.apple.main-thread' in line;path=[]
            if main:total=int(line.split()[0])
            continue
        if not main:continue
        m=re.match(r'^([ +!:|]*)(\d+) (.+?)  \(in ([^)]*)\)',line)
        if not m:continue
        depth,n,sym=len(m[1]),int(m[2]),m[3]
        while path and path[-1][0]>=depth:path.pop()
        tags={key for key,patterns in keys.items() if any(pattern in sym for pattern in patterns)}
        for tag in tags:
            if not any(tag in tags for _,tags in path):counts[tag]+=n
        path.append((depth,tags))
    return {'main_samples':total,**counts}
result={}
for name in ['before-idle','after-fixed-idle','delivery-before-idle','delivery-after-idle']:
    if (out/(name+'.json')).exists():
        data=json.loads((out/(name+'.json')).read_text());result[name]={k:data[k] for k in ['first_content_seconds','idle_cpu_percent']};result[name]['ps_peak']=max(row['cpu'] for row in data['ps']);result[name]['ps_count']=len(data['ps']);result[name]['sample']=sample(name)
for label in ['before','after-fixed','paired-before','paired-after','delivery-before','delivery-after']:
    values=[json.loads(path.read_text())['first_content_seconds'] for path in sorted(out.glob(label+'-startup-*.json'))]
    if values:result[label]={'first_frames':values,'median':statistics.median(v for v in values if v is not None)}
(out/'metrics.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
