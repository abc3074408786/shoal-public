"""Read fixture process metrics, or fault-inject only a verified fixture host."""
import json,pathlib,sys,psutil
mode=sys.argv[1];root=str(pathlib.Path(sys.argv[2]).resolve()).lower();pids=json.loads(sys.argv[3])
if mode!='sample':
    assert mode in ('suspend','resume','terminate') and len(pids)==1
    process=psutil.Process(pids[0]);command=' '.join(process.cmdline()).lower()
    assert root in command and ('host' in command or 'terminal-stream-fixture.py' in command),'not a fixture host'
    getattr(process,mode)();print(json.dumps({'pid':process.pid,'action':mode}));sys.exit()
processes={}
for pid in pids:
    try:
        process=psutil.Process(pid);processes[pid]=process
        for child in process.children(recursive=True):processes[child.pid]=child
    except psutil.Error:pass
rows=[]
for process in processes.values():
    try:
        info=process.memory_info();cpu=process.cpu_times()
        rows.append({'pid':process.pid,'created':process.create_time(),'name':process.name(),'private':getattr(info,'private',info.rss),'rss':info.rss,'cpuSeconds':cpu.user+cpu.system})
    except psutil.Error:pass
print(json.dumps({'privateBytes':sum(r['private'] for r in rows),'rssBytes':sum(r['rss'] for r in rows),'cpuSeconds':sum(r['cpuSeconds'] for r in rows),'processes':rows}))
