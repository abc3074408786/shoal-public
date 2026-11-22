#!/usr/bin/env python3
"""Paired real-daemon IPC benchmark with synthetic historical session metadata.

No live terminal or model is spawned. Both binaries read identical 4 KiB logs.
Alternating version order, three rounds, 8/128/512 stored sessions, empty/data
polls. Measures complete socket RPC, not screen rendering or process startup.
"""
import argparse
import base64
import hashlib
import html
import json
import math
import os
from pathlib import Path
import socket
import sqlite3
import statistics
import subprocess
import tempfile
import time

def rpc(root, request):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(15)
        connection.connect(str(root/'daemon.sock'))
        connection.sendall((json.dumps(request)+'\n').encode())
        data=bytearray()
        while b'\n' not in data:
            chunk=connection.recv(262144)
            if not chunk: raise RuntimeError('daemon closed connection')
            data.extend(chunk)
            if len(data)>16*1024*1024: raise RuntimeError('oversized response')
        result=json.loads(data.split(b'\n')[0])
    if not result.get('ok'): raise RuntimeError(str(result))
    return result

def stats(values):
    return {'median_ms':statistics.median(values),'p95_ms':sorted(values)[math.ceil(.95*len(values))-1],
            'samples':len(values),'raw_ms':values}

def cpu_seconds(pid):
    fields=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()
    return (int(fields[11])+int(fields[12]))/os.sysconf('SC_CLK_TCK')

def measure(runtime, directory, samples):
    results=[]
    with tempfile.TemporaryDirectory(prefix='vl-lookup-bench-') as temporary:
        root=Path(temporary);data=root/'data';data.mkdir();work=root/'work';work.mkdir()
        log=root/'recording';log.mkdir();payload=b'Z'*4096;(log/'terminal.bin').write_bytes(payload)
        with directory.open('w') as stream:
            child=subprocess.Popen([str(runtime),'daemon',str(data)],stdin=subprocess.DEVNULL,stdout=stream,stderr=subprocess.STDOUT)
        try:
            deadline=time.monotonic()+15
            while True:
                try: rpc(data,{'cmd':'snapshot'});break
                except (OSError,RuntimeError):
                    if child.poll() is not None or time.monotonic()>deadline: raise
                    time.sleep(.05)
            rpc(data,{'cmd':'project.add','path':str(work)})
            workspace=rpc(data,{'cmd':'workbench.snapshot'})['workspaces'][0]['id']
            with sqlite3.connect(data/'workbench.db') as db:
                for count in (8,128,512):
                    db.execute('DELETE FROM sessions')
                    for index in range(count):
                        sid=f'history-{index}'
                        document=dict(id=sid,workspace_id=workspace,title=f'Stored session {index}',kind='terminal',task_id=None,native_id=None,
                            descriptor=dict(id=sid,endpoint=str(root/'unused.sock'),directory=str(log),spawn=dict(program='/bin/sh',args=[],cwd=str(work),
                                env={'TERM':'xterm-256color','LANG':'C.UTF-8'},adapter='generic',sandbox={'mode':'native'})))
                        db.execute('INSERT INTO sessions(id,workspace_id,document) VALUES(?,?,?)',(sid,workspace,json.dumps(document)))
                    db.commit()
                    for since in (4096,0):
                        request={'cmd':'workbench.terminal.output','workspace_id':workspace,'session_id':f'history-{count-1}','since':since}
                        values=[];before_cpu=None
                        for index in range(-20,samples):
                            if index==0: before_cpu=cpu_seconds(child.pid)
                            started=time.perf_counter_ns();response=rpc(data,request);elapsed=(time.perf_counter_ns()-started)/1e6
                            assert response['from']==since and response['end']==response['total']==4096
                            assert base64.b64decode(response['data'],validate=True)==payload[since:]
                            if index>=0: values.append(elapsed)
                        results.append(dict(sessions=count,kind='empty' if since else '4KiB',verified=True,
                            daemon_cpu_ms_per_request=(cpu_seconds(child.pid)-before_cpu)*1000/samples,**stats(values)))
        finally:
            child.terminate()
            try: child.wait(timeout=5)
            except subprocess.TimeoutExpired: child.kill();child.wait(timeout=5)
    return results

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before',type=Path,required=True)
    parser.add_argument('--after',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--samples',type=int,default=300)
    args=parser.parse_args()
    if not 30<=args.samples<=2000: parser.error('samples must be 30..2000')
    args.output.mkdir(parents=True,exist_ok=False)
    versions={name:getattr(args,name).resolve(strict=True) for name in ('before','after')}
    result=dict(status='running',method=__doc__,samples_per_round=args.samples,rounds=[],summary=[],
                binaries={name:{'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for name,path in versions.items()},loadavg_before=os.getloadavg())
    try:
        for round_index in range(3):
            order=['before','after'] if round_index%2==0 else ['after','before']
            for name in order:
                print('round',round_index+1,name,flush=True)
                cases=measure(versions[name],args.output/f'{round_index}-{name}.log',args.samples)
                result['rounds'].append(dict(round=round_index+1,version=name,cases=cases))
        for count in (8,128,512):
            for kind in ('empty','4KiB'):
                row=dict(sessions=count,kind=kind)
                for name in versions:
                    cases=[case for r in result['rounds'] if r['version']==name for case in r['cases'] if case['sessions']==count and case['kind']==kind]
                    row[name]=stats([x for case in cases for x in case['raw_ms']])
                    row[name]['daemon_cpu_ms_per_request']=statistics.mean(case['daemon_cpu_ms_per_request'] for case in cases)
                row['median_reduction_percent']=(1-row['after']['median_ms']/row['before']['median_ms'])*100
                result['summary'].append(row)
        result['status']='passed'
    except BaseException as error:
        result['status']='failed';result['error']=repr(error);raise
    finally:
        result['loadavg_after']=os.getloadavg()
        (args.output/'result.json').write_text(json.dumps(result,indent=2))
    rows=''.join(f'<tr><td>{r["sessions"]}</td><td>{r["kind"]}</td><td>{r["before"]["median_ms"]:.3f}</td><td>{r["after"]["median_ms"]:.3f}</td><td>{r["median_reduction_percent"]:.1f}%</td><td>{r["before"]["p95_ms"]:.3f} → {r["after"]["p95_ms"]:.3f}</td></tr>' for r in result['summary'])
    page='''<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Velella 终端查询优化</title><style>body{max-width:1050px;margin:48px auto;padding:20px;font:16px/1.7 system-ui;color:#17202c;background:#f6f7f9}table{border-collapse:collapse;width:100%;background:white}th,td{padding:12px;border-bottom:1px solid #ddd;text-align:left;font-variant-numeric:tabular-nums}th{background:#e9eef4}pre{white-space:pre-wrap}</style><h1>Velella 终端会话查询优化</h1><p>单个终端输出读取由“加载并反序列化全部会话”改为主键查询。工作区归属、归档限制和输出游标校验仍保留。无会话数据缓存，无数据库迁移。</p><table><tr><th>历史会话数</th><th>输出</th><th>优化前中位 ms</th><th>优化后中位 ms</th><th>耗时减少</th><th>P95 前 → 后 ms</th></tr>'''+rows+'''</table><p>测试使用两个实际 release daemon、隔离临时数据库及确定性的历史会话记录。每种条件三轮，版本执行顺序交替，每轮先预热 20 次；所有响应逐字节验证。并未启动 512 个真实终端。计时包含完整本机 socket RPC，不代表 UI 绘制或真实模型性能。</p><p>所有数据及每轮样本见 <a href="result.json">原始 JSON</a>。本报告是针对本次热点的前后对比，不替代完整桌面基线。</p></html>'''
    (args.output/'report.html').write_text(page)
    print(json.dumps([{k:v for k,v in row.items() if k not in ('before','after')} for row in result['summary']],indent=2),flush=True)

if __name__=='__main__': main()
