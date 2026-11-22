#!/usr/bin/env python3
"""Linux local performance baseline. Run in an Orca terminal; no model calls.

Default timing: 60s warmup, 120s idle, 600s output soak, 120s recovery.
--quick is a harness check only and cannot substitute for the standard baseline.
"""
import argparse
import base64
import concurrent.futures
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shlex
import shutil
import signal
import socket
import statistics
import subprocess
import tempfile
import threading
import time
import uuid

REPO = Path(__file__).resolve().parents[1]

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def summary(values):
    ordered = sorted(values)
    return dict(samples=len(values), median_ms=statistics.median(values),
                p95_ms=ordered[math.ceil(.95 * len(values))-1], raw_ms=values)

def processes():
    result = {}
    for item in Path('/proc').iterdir():
        if not item.name.isdigit():
            continue
        try:
            stat = (item/'stat').read_text().rsplit(')', 1)[1].split()
            result[int(item.name)] = dict(ppid=int(stat[1]), ticks=int(stat[11])+int(stat[12]),
                start=stat[19], rss=int(stat[21])*os.sysconf('SC_PAGE_SIZE'),
                cmd=(item/'cmdline').read_bytes().replace(b'\0', b' ').decode(errors='replace'))
        except (OSError, ValueError, IndexError):
            pass
    return result

class Run:
    def __init__(self, output, quick):
        self.output, self.quick = output, quick
        self.root = Path(tempfile.mkdtemp(prefix='velella-perf-'))
        self.runtime = REPO/'target/release/velella-runtime'
        self.desktop = REPO/'desktop/node_modules/.bin/electron'
        self.phase = 'setup'
        self.desktop_pid = None
        self.owned = {}
        self.stop = threading.Event()
        self.rows = []
        self.results = dict(schema=1, status='running', quick=quick, cases={}, errors=[],
            started=time.strftime('%Y-%m-%dT%H:%M:%S%z'), platform=platform.platform(),
            logical_cpus=os.cpu_count(), loadavg_before=os.getloadavg(),
            cpu_model=next((line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')),None),
            memory_total=Path('/proc/meminfo').read_text().splitlines()[0],
            runtime_sha256=digest(self.runtime), source_hashes=self.fingerprints(),
            git_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip(),
            git_status=subprocess.check_output(['git','status','--short'],cwd=REPO,text=True),
            method={'cpu':'100%=one logical CPU; /proc tick deltas at 1 second intervals',
                'memory':'sum of RSS; shared pages may be counted more than once',
                'startup':'fresh app/profile and daemon; warm OS page cache; process spawn to DOM+IPC readiness',
                'terminal':'daemon socket input through complete output marker, 10ms empty-poll interval; excludes screen paint',
                'soak':'four sessions, 65536 Z bytes per second each; no real model',
                'p95':'nearest-rank; descriptive with small samples',
                'scope':'current Electron production frontend and Rust release; not Orca A/B or UI FPS'})
        (output/'metadata.json').write_text(json.dumps(self.results,indent=2))
        self.thread = threading.Thread(target=self.sample, daemon=True)
        self.thread.start()

    def fingerprints(self):
        paths = subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=REPO).split(b'\0')
        return {os.fsdecode(p):digest(REPO/os.fsdecode(p)) for p in paths if p and (REPO/os.fsdecode(p)).is_file()}

    def classify(self, procs):
        groups = {}
        for pid, value in procs.items():
            cmd = value['cmd']
            if str(self.root) in cmd and pid != os.getpid():
                groups[pid] = ('runtime' if 'velella-runtime' in cmd else
                               'desktop' if '/electron' in cmd else 'workload')
            if pid == self.desktop_pid:
                groups[pid] = 'desktop'
            if ('/orca/' in cmd or '/Orca' in cmd or 'orca-ide' in cmd or '/orcad' in cmd) and pid != os.getpid():
                groups.setdefault(pid, 'orca')
        for _ in range(12):
            changed = False
            for pid, value in procs.items():
                if pid not in groups and value['ppid'] in groups:
                    parent_group = groups[value['ppid']]
                    # Orca also owns this runner: do not count the benchmark as Orca.
                    if pid == os.getpid() or value['ppid'] == os.getpid():
                        continue
                    groups[pid] = 'workload' if parent_group == 'runtime' and 'velella-runtime' not in value['cmd'] else parent_group
                    changed = True
            if not changed:
                break
        for pid, group in groups.items():
            if group != 'orca':
                self.owned[pid] = procs[pid]['start']
        return groups

    def sample(self):
        previous, last = {}, time.monotonic()
        with (self.output/'resources.csv').open('w') as stream:
            writer = csv.DictWriter(stream, fieldnames=['elapsed_s','phase','group','cpu_percent','rss_mib','processes'])
            writer.writeheader()
            began = last
            while not self.stop.is_set():
                now, procs = time.monotonic(), processes()
                groups = self.classify(procs)
                for group in ('desktop','runtime','workload','orca'):
                    selected = [pid for pid in groups if groups[pid] == group]
                    ticks = sum(max(0,procs[p]['ticks']-previous[p]['ticks']) for p in selected
                                if p in previous and procs[p]['start'] == previous[p]['start'])
                    row = dict(elapsed_s=round(now-began,3), phase=self.phase, group=group,
                        cpu_percent=round(ticks/os.sysconf('SC_CLK_TCK')/max(now-last,.001)*100,3),
                        rss_mib=round(sum(procs[p]['rss'] for p in selected)/1048576,3),processes=len(selected))
                    writer.writerow(row)
                    self.rows.append(row)
                stream.flush()
                previous, last = procs, now
                self.stop.wait(1)

    def stage(self, value):
        self.phase = value
        print(time.strftime('%H:%M:%S'), value, flush=True)
        (self.output/'progress.json').write_text(json.dumps({'phase':value,'time':time.time()}))

    def rpc(self, command, **kwargs):
        with socket.socket(socket.AF_UNIX) as conn:
            conn.settimeout(15)
            conn.connect(str(self.data/'daemon.sock'))
            conn.sendall((json.dumps(dict(cmd=command, **kwargs))+'\n').encode())
            response = bytearray()
            while b'\n' not in response:
                chunk = conn.recv(262144)
                if not chunk: raise RuntimeError('RPC connection closed')
                response.extend(chunk)
                if len(response)>16*1024*1024: raise RuntimeError('RPC size limit')
            value = json.loads(response.split(b'\n')[0])
        if not value.get('ok'): raise RuntimeError(str(value))
        return value

    def launch(self, index):
        self.data = self.root/f'app-{index}'
        self.data.mkdir()
        ready = self.data/'perf-ready.json'
        env = dict(os.environ, VELELLA_DATA_DIR=str(self.data), VELELLA_RUNTIME_PATH=str(self.runtime),
                   VELELLA_PERF_READY=str(ready))
        log = (self.output/f'electron-{index}.log').open('w')
        start = time.monotonic()
        child = subprocess.Popen([str(self.desktop),str(REPO/'scripts/performance-desktop.cjs')],
                                 env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        log.close()
        self.desktop_pid = child.pid
        while not ready.exists():
            if child.poll() is not None: raise RuntimeError(f'Electron exited {child.returncode}')
            if time.monotonic()-start > 30: raise TimeoutError('Electron readiness >30s')
            time.sleep(.02)
        evidence = json.loads(ready.read_text())
        if not evidence.get('ready'): raise RuntimeError(str(evidence))
        elapsed = (time.monotonic()-start)*1000
        self.classify(processes())
        return elapsed, child

    def cleanup_processes(self):
        procs = processes()
        self.classify(procs)
        targets = {p:s for p,s in list(self.owned.items()) if p in procs and procs[p]['start']==s}
        for sig in (signal.SIGTERM, signal.SIGKILL):
            for pid, start in targets.items():
                current = processes().get(pid)
                if current and current['start']==start:
                    try: os.kill(pid,sig)
                    except ProcessLookupError: pass
            time.sleep(.3)
        self.desktop_pid = None

    def open_terminal(self):
        value = self.rpc('workbench.terminal.open',workspace_id=self.workspace,launch='terminal')
        terminal = dict(workspace_id=self.workspace,session_id=value['session_id'])
        time.sleep(.15)
        return terminal

    def workload(self, terminal, chunks=0, interval=0, timeout=15):
        token = uuid.uuid4().hex
        begin, end = ('PERF_BEGIN_'+token+'\r\n').encode(), ('\r\nPERF_END_'+token+'\r\n').encode()
        cursor = self.rpc('workbench.terminal.output',**terminal,since=0)['total']
        command = shlex.join(['/usr/bin/python3','-I',str(REPO/'scripts/performance-workload.py'),token,str(chunks),str(interval)])
        start = time.monotonic()
        self.rpc('workbench.terminal.input',**terminal,text=command,enter=True,input_kind='user')
        pending, began, count = b'', False, 0
        actual = hashlib.sha256()
        while time.monotonic()-start < timeout:
            value = self.rpc('workbench.terminal.output',**terminal,since=cursor)
            if value['from'] != cursor: raise RuntimeError('Output cursor discontinuity')
            block = base64.b64decode(value['data'],validate=True)
            if value['end']-cursor != len(block): raise RuntimeError('Output size mismatch')
            cursor = value['end']
            pending += block
            if not began:
                offset = pending.find(begin)
                if offset >= 0:
                    pending, began = pending[offset+len(begin):], True
                else:
                    pending = pending[-max(len(begin),8192):]
            if began:
                offset = pending.find(end)
                if offset >= 0:
                    payload = pending[:offset]
                    count += len(payload)
                    actual.update(payload)
                    expected = hashlib.sha256()
                    for _ in range(chunks): expected.update(b'Z'*65536)
                    if count != chunks*65536 or actual.hexdigest()!=expected.hexdigest():
                        raise AssertionError(f'Payload mismatch: {count} != {chunks*65536}')
                    return dict(ms=(time.monotonic()-start)*1000, bytes=count, sha256=actual.hexdigest(),verified=True)
                if len(pending)>len(end):
                    payload, pending = pending[:-len(end)],pending[-len(end):]
                    count += len(payload)
                    actual.update(payload)
            if cursor >= value['total']: time.sleep(.01)
        raise TimeoutError(f'PTY completion >{timeout}s; verified bytes so far={count}')

    def run(self):
        self.stage('startup')
        timings = []
        for index in range(2 if self.quick else 10):
            elapsed, child = self.launch(index)
            timings.append(elapsed)
            print('startup',index,round(elapsed,2),'ms',flush=True)
            if index != (1 if self.quick else 9):
                self.cleanup_processes()
                child.wait(timeout=5)
        self.results['cases']['startup'] = summary(timings)
        self.stage('warmup')
        time.sleep(2 if self.quick else 60)
        self.stage('idle')
        time.sleep(3 if self.quick else 120)
        self.stage('terminal_setup')
        project = self.root/'project'
        project.mkdir()
        self.rpc('project.add',path=str(project))
        self.workspace = self.rpc('workbench.snapshot')['workspaces'][0]['id']
        terminals = [self.open_terminal() for _ in range(8)]
        self.stage('terminal_latency')
        for _ in range(3): self.workload(terminals[0])
        timings = [self.workload(terminals[0])['ms'] for _ in range(5 if self.quick else 30)]
        self.results['cases']['terminal_latency'] = summary(timings)
        self.stage('concurrency')
        cases = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            for n in (1,4,8):
                for chunks in (0,16):
                    for repeat in range(1 if self.quick else 3):
                        start = time.monotonic()
                        futures = [pool.submit(self.workload,t,chunks) for t in terminals[:n]]
                        values = [f.result() for f in futures]
                        wall = time.monotonic()-start
                        cases.append(dict(sessions=n,chunks=chunks,repeat=repeat,wall_ms=wall*1000,
                            mib_s=sum(v['bytes'] for v in values)/1048576/wall,results=values))
        self.results['cases']['concurrency'] = cases
        self.stage('restricted')
        with (self.output/'restricted.log').open('w') as log:
            completed = subprocess.run(['/usr/bin/python3',str(REPO/'scripts/benchmark-restricted.py'),
                '--runtime',str(self.runtime),'--output',str(self.output/'restricted'),
                '--samples',str(5 if self.quick else 30),'--warmup','3'],stdout=log,stderr=subprocess.STDOUT,timeout=240)
        if completed.returncode:
            self.results['errors'].append('restricted benchmark failed; see restricted.log')
        if (self.output/'restricted/result.json').exists():
            self.results['cases']['restricted'] = json.loads((self.output/'restricted/result.json').read_text())
        self.stage('loaded_idle')
        time.sleep(2 if self.quick else 120)
        self.stage('soak')
        seconds = 5 if self.quick else 600
        probes = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
            futures = [pool.submit(self.workload,t,seconds,1,seconds+15) for t in terminals[:4]]
            while not all(f.done() for f in futures):
                probes.append(dict(time=time.time(),**self.workload(terminals[4])))
                time.sleep(1 if self.quick else 10)
            values = [f.result() for f in futures]
        self.results['cases']['soak'] = dict(seconds=seconds,sessions=values,probes=probes)
        self.stage('recovery')
        time.sleep(3 if self.quick else 120)
        self.results['cases']['post_soak_latency'] = summary([self.workload(terminals[4])['ms'] for _ in range(5 if self.quick else 30)])

    def finish(self):
        self.stage('cleanup')
        self.cleanup_processes()
        self.stop.set()
        self.thread.join(timeout=5)
        live = processes()
        survivors = [p for p,s in self.owned.items() if p in live and live[p]['start']==s and live[p]['cmd']]
        self.results['cleanup'] = dict(surviving_owned_pids=survivors)
        if survivors: self.results['errors'].append('Owned processes survived cleanup')
        else: shutil.rmtree(self.root)
        self.results['cleanup']['temporary_directory_removed'] = not self.root.exists()
        missing = [key for key in ('startup','terminal_latency','concurrency','restricted','soak','post_soak_latency')
                   if key not in self.results['cases']]
        if missing: self.results['errors'].append('Incomplete phases: '+', '.join(missing))
        self.results['source_unchanged'] = self.fingerprints()==self.results['source_hashes']
        self.results['runtime_unchanged'] = digest(self.runtime)==self.results['runtime_sha256']
        if not self.results['source_unchanged'] or not self.results['runtime_unchanged']:
            self.results['errors'].append('Source/runtime changed during measurement')
        self.results['resource_summary'] = {}
        for phase in sorted({r['phase'] for r in self.rows}):
            self.results['resource_summary'][phase] = {}
            for group in ('desktop','runtime','workload','orca'):
                rows = [r for r in self.rows if r['phase']==phase and r['group']==group]
                self.results['resource_summary'][phase][group] = dict(samples=len(rows),
                    mean_cpu=statistics.mean(r['cpu_percent'] for r in rows),
                    peak_rss_mib=max(r['rss_mib'] for r in rows),
                    first_rss_mib=rows[0]['rss_mib'],last_rss_mib=rows[-1]['rss_mib'])
        self.results.update(status='failed' if self.results['errors'] else 'passed',loadavg_after=os.getloadavg())
        (self.output/'result.json').write_text(json.dumps(self.results,indent=2))
        print('RESULT',self.results['status'],self.output/'result.json',flush=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--quick',action='store_true')
    args = parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=False)
    run = Run(args.output.resolve(),args.quick)
    try:
        run.run()
    except BaseException as error:
        import traceback
        traceback.print_exc()
        run.results['errors'].append(type(error).__name__+': '+str(error))
    finally:
        run.finish()
    raise SystemExit(0 if run.results['status']=='passed' else 1)

if __name__ == '__main__':
    main()
