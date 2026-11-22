"""Deterministic ConPTY program. It only reads its own fixture control file."""
import json,msvcrt,pathlib,sys,time,threading,os
control=pathlib.Path(sys.argv[1]);identity=sys.argv[2]
write_lock=threading.Lock()
def emit(text):
    with write_lock:sys.stdout.write(text);sys.stdout.flush()
def marker(row,text):emit('\x1b7\x1b['+str(row)+';1H'+text+'\x1b[K\x1b8')
def output():
    last_burst=None;last_history=None;sequence=0;loaded=False;replaying=False;frame=0
    def block():
        nonlocal sequence
        sequence+=1
        # Printable, changing rows survive ConPTY's screen differencing.
        # Repeating SGR escapes alone are swallowed by ConPTY, not throughput.
        rows=max(4,os.get_terminal_size().lines)
        return '\x1b[3;'+str(rows)+'r\x1b['+str(rows)+';1H'+''.join((str(sequence)+' '+str(i)+' '+identity+' ').ljust(79,'x')+'\n' for i in range(819))
    while True:
        try:state=json.loads(control.read_text())
        except (OSError,ValueError):state={}
        if state.get('replay_until',0)>time.time():
            frame+=1;replaying=True
            # Fixed TUI redraw, Unicode and ANSI workload; no model/network wait.
            emit('\x1b7\x1b[4;1H\x1b[0K\x1b[36mAI replay frame '+str(frame)+'\x1b[0m'
                 '\x1b[5;1H\x1b[0K\u4e2d\u6587 \U0001f680 e\u0301 code review'
                 '\x1b[6;1H\x1b[0K\x1b[32m+ const result = 42;\x1b[0m'
                 '\x1b[7;1H\x1b[0KProgress: '+str(frame%100)+'%\x1b8')
        elif replaying:
            marker(2,'REPLAY_DONE_'+identity);replaying=False
        if state.get('history') and state['history']!=last_history:
            last_history=state['history']
            emit('\x1b[r')
            for start in range(0,1200,20):
                emit(''.join(('HISTORY_'+identity+'_'+str(i)).ljust(75,'x')+'\r\n' for i in range(start,start+20)))
                time.sleep(.02)
            marker(2,'HISTORY_DONE_'+identity)
        if state.get('burst') and state['burst']!=last_burst:
            last_burst=state['burst']
            for _ in range(256):emit(block())
            marker(2,'BURST_DONE_'+identity)
        if state.get('until',0)>time.time():emit(block());loaded=True
        elif loaded:marker(2,'LOAD_DONE_'+identity);loaded=False
        time.sleep(.05 if replaying else .125)
threading.Thread(target=output,daemon=True).start()
emit('\x1b[2J\x1b[HREADY_'+identity)
seq=0
while True:
    if msvcrt.kbhit():
        key=msvcrt.getwch()
        if key=='\x03':break
        if key in ('\x00','\xe0'):msvcrt.getwch();continue
        seq+=1;marker(1,'ECHO_'+identity+'_'+str(seq))
    else:time.sleep(.001)
