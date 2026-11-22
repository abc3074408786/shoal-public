#!/usr/bin/env python3
"""Render a self-contained Chinese HTML report directly from baseline evidence."""
import argparse
import csv
import html
import json
from pathlib import Path
import statistics

def esc(value): return html.escape(str(value))
def number(value): return f'{value:.2f}' if isinstance(value,(int,float)) else esc(value)
def table(headers, rows):
    return '<table><thead><tr>'+''.join('<th>'+esc(v)+'</th>' for v in headers)+'</tr></thead><tbody>'+''.join(
        '<tr>'+''.join('<td>'+number(v)+'</td>' for v in row)+'</tr>' for row in rows)+'</tbody></table>'

def chart(rows, key, title):
    colors = {'desktop':'#2563eb','runtime':'#b45309','workload':'#16803c','orca':'#9333ea'}
    maximum = max(1,max(float(r[key]) for r in rows))
    duration = max(1,max(float(r['elapsed_s']) for r in rows))
    lines = []
    for group,color in colors.items():
        points = ' '.join(f"{50+float(r['elapsed_s'])/duration*850:.1f},{210-float(r[key])/maximum*180:.1f}"
                          for r in rows if r['group']==group)
        lines.append(f'<polyline fill="none" stroke="{color}" stroke-width="1.5" points="{points}"/>')
    labels = ''.join(f'<span style="color:{c}">{esc(g)}　</span>' for g,c in colors.items())
    return f'<h3>{esc(title)}</h3><p>{labels}</p><svg role="img" aria-label="{esc(title)}" viewBox="0 0 960 245"><path d="M50 25V210H900" fill="none" stroke="#aaa"/><text x="5" y="35">{maximum:.0f}</text><text x="15" y="210">0</text>'+''.join(lines)+f'<text x="50" y="237">0 秒</text><text x="810" y="237">{duration:.0f} 秒</text></svg>'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory',type=Path)
    args=parser.parse_args()
    directory=args.directory.resolve()
    result=json.loads((directory/'result.json').read_text())
    with (directory/'resources.csv').open() as stream: rows=list(csv.DictReader(stream))
    cases=result['cases']
    # Reject inconsistent report inputs instead of silently publishing mismatched numbers.
    for phase, groups in result['resource_summary'].items():
        for group, values in groups.items():
            samples=[r for r in rows if r['phase']==phase and r['group']==group]
            assert len(samples)==values['samples']
            assert abs(statistics.mean(float(r['cpu_percent']) for r in samples)-values['mean_cpu'])<1e-8
            assert max(float(r['rss_mib']) for r in samples)==values['peak_rss_mib']
    for key in ('startup','terminal_latency','post_soak_latency'):
        if key in cases:
            assert statistics.median(cases[key]['raw_ms'])==cases[key]['median_ms']
    status='完整执行，正确性验证通过' if result['status']=='passed' else '存在失败或不完整项目'
    parts=['<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Velella 性能基线</title><style>body{font:16px/1.7 system-ui,sans-serif;color:#17202c;background:#f6f7f9;max-width:1100px;margin:40px auto;padding:0 24px}h1{font-size:32px}h2{margin-top:40px}table{border-collapse:collapse;width:100%;background:white}th,td{padding:10px 14px;border-bottom:1px solid #ddd;text-align:left;font-variant-numeric:tabular-nums}th{background:#e9eef4}svg{background:white;width:100%}text{font:12px system-ui}code{overflow-wrap:anywhere}a{color:#135cc4}details{margin:20px 0}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:13px}</style><h1>Velella 整体性能基线</h1>',
        f'<p><strong>{status}</strong> · {"驱动快速自检，不是正式基线" if result["quick"] else "正式基线"} · {esc(result["started"])}</p>',
        '<p>测试对象：当前工作区的 Electron 生产前端与 Rust release。Orca 用于运行和展示；其资源单列，不能据此比较两款产品。通过表示执行和输出验证成功，不代表满足某个性能门槛。</p>',
        '<p><a href="result.json">原始结果 JSON</a> · <a href="resources.csv">逐秒资源 CSV</a> · <a href="run.log">运行日志</a> · <a href="restricted/result.json">受限执行原始结果</a></p>',
        '<h2>启动与终端响应</h2>',table(['指标','样本数','中位数 ms','P95 ms'],[
            [label,cases[key]['samples'],cases[key]['median_ms'],cases[key]['p95_ms']]
            for key,label in [('startup','启动至工作台 DOM + IPC 可用'),('terminal_latency','终端短命令'),('post_soak_latency','持续负载后短命令')] if key in cases]),
        '<p>启动使用新配置和新后台进程，但不清操作系统文件缓存。终端响应含 Python 进程启动、IPC 与输出读取，空轮询间隔 10 ms；不等同于输入至屏幕显示的延迟。P95 在少量样本下仅作描述。</p>',
        '<h2>资源采样</h2>',table(['阶段','进程组','样本数','平均 CPU %','峰值 RSS MiB','末尾 RSS MiB'],[
            [phase,group,v['samples'],v['mean_cpu'],v['peak_rss_mib'],v['last_rss_mib']]
            for phase in ('idle','concurrency','loaded_idle','soak','recovery') for group,v in result['resource_summary'].get(phase,{}).items()]),
        '<p>100% CPU 表示占满一个逻辑核。RSS 为组内进程求和，共享页可能重复计入；这不是唯一物理内存。短进程可能在一秒采样之间退出。恢复阶段仍保留测试终端，不能直接与零终端空闲值比较为泄漏。</p>',
        chart(rows,'cpu_percent','CPU 随时间变化（% / 单核）'),chart(rows,'rss_mib','RSS 随时间变化（MiB）'),
        '<h2>并发与输出完整性</h2>',table(['会话数','每会话数据 MiB','轮次','总耗时 ms','有效载荷 MiB/s','完整性'],[
            [v['sessions'],v['chunks']/16,v['repeat']+1,v['wall_ms'],v['mib_s'],'通过' if all(x['verified'] for x in v['results']) else '失败']
            for v in cases.get('concurrency',[])]),
        '<p>完整性以完成标记、字节数量及 SHA-256 同时验证。吞吐量是固定数据量端到端测值，不是最大可持续吞吐。</p>']
    restricted=cases.get('restricted',{})
    parts += ['<h2>受限执行开销</h2>',table(['用例','路径','中位数 ms','P95 ms'],[
        [name,path,v['median_ms'],v['p95_ms']] for name,paths in restricted.get('cases',{}).items() for path,v in paths.items()]),
        '<p>文件 API 对比的调用边界不同；受限执行脚本中的历史 debug 限定以本报告的 release 构建记录为准。</p>']
    soak=cases.get('soak')
    if soak:
        probes=[v['ms'] for v in soak['probes']]
        parts += ['<h2>持续负载</h2>',f'<p>{soak["seconds"]} 秒，4 个会话，每会话目标 64 KiB/s。输出校验：{sum(v["verified"] for v in soak["sessions"])}/4。并行短命令探针 {len(probes)} 次，中位数 {statistics.median(probes):.2f} ms，最大值 {max(probes):.2f} ms。</p>']
    missing=[key for key in ('startup','terminal_latency','concurrency','restricted','soak','post_soak_latency') if key not in cases]
    parts += ['<h2>失败、未测量与清理</h2>', '<p>'+esc('; '.join(result['errors']) or '无记录到的执行失败。')+'</p>',
        '<p>未完成项目：'+esc(', '.join(missing) or '无')+'。未测量：屏幕绘制延迟、交互帧率、真实模型速度、整机冷启动和长时间泄漏。</p>',
        '<p>清理结果：'+esc(json.dumps(result['cleanup'],ensure_ascii=False))+'</p>',
        '<details><summary>环境、版本与测量方法</summary><pre>'+esc(json.dumps({k:result[k] for k in ('platform','logical_cpus','git_head','runtime_sha256','source_unchanged','runtime_unchanged','loadavg_before','loadavg_after','method')},ensure_ascii=False,indent=2))+'</pre></details>',
        '</html>']
    (directory/'report.html').write_text('\n'.join(parts))
    (directory/'report-validation.json').write_text(json.dumps({'passed':True,'resource_rows':len(rows),'source':'result.json + resources.csv'}))
    print(directory/'report.html')

if __name__=='__main__': main()
