"""Owner-only IPC fixture for exact byte-rate tests after ConPTY.

ConPTY coalesces screen updates, so console writes are not a byte-rate oracle.
This replaces two STOPPED fixture hosts only; the other six remain real PTYs.
"""
import ctypes as c,json,msvcrt,os,pathlib,struct,sys,threading,time
from ctypes import wintypes as w
desc=json.loads(pathlib.Path(sys.argv[1]).read_text(encoding='utf-8-sig'))
root=pathlib.Path(desc['directory']);control=pathlib.Path(sys.argv[2]);identity=sys.argv[3]
assert 'terminal-stability' in str(root) and 'native-' in str(root)
owner=open(root/'owner.lock','a+b');owner.seek(0);msvcrt.locking(owner.fileno(),msvcrt.LK_NBLCK,1)
log=open(root/'terminal.bin','ab',buffering=0);geometry=open(root/'terminal.geometry','ab',buffering=0)
lock=threading.Lock();stopped=threading.Event();seq=0
status=json.loads((root/'status.json').read_text(encoding='utf-8-sig'))
status.update(pid=os.getpid(),state='waiting_input',exit=None,input_protocol=1,human_control=True)
def persist():
    temp=root/'fixture-status.tmp';temp.write_text(json.dumps(status));os.replace(temp,root/'status.json')
persist()
with lock:
    geometry.write(struct.pack('<QHHI',log.tell(),30,100,1));log.write(('\x1b[2J\x1b[HREADY_'+identity).encode())
def marker(row,text):log.write(('\x1b7\x1b['+str(row)+';1H'+text+'\x1b[K\x1b8').encode())
def produce():
    burst=None;loaded=False;next_write=time.monotonic();chunk=b'\x1b[0m'*16384
    while not stopped.is_set():
        try:state=json.loads(control.read_text())
        except (OSError,ValueError):state={}
        if state.get('burst') and state['burst']!=burst:
            burst=state['burst']
            for _ in range(256):
                with lock:log.write(chunk)
            with lock:marker(2,'BURST_DONE_'+identity)
        if state.get('until',0)>time.time():
            with lock:log.write(chunk)
            loaded=True;next_write+=.125;stopped.wait(max(0,next_write-time.monotonic()))
        else:
            if loaded:
                with lock:marker(2,'LOAD_DONE_'+identity)
                loaded=False
            next_write=time.monotonic();stopped.wait(.02)
threading.Thread(target=produce,daemon=True).start()
k=c.WinDLL('kernel32',use_last_error=True);a=c.WinDLL('advapi32',use_last_error=True)
class SA(c.Structure):_fields_=[('length',w.DWORD),('descriptor',w.LPVOID),('inherit',w.BOOL)]
k.CreateNamedPipeW.argtypes=[w.LPCWSTR,w.DWORD,w.DWORD,w.DWORD,w.DWORD,w.DWORD,w.DWORD,c.POINTER(SA)];k.CreateNamedPipeW.restype=w.HANDLE
k.ConnectNamedPipe.argtypes=[w.HANDLE,w.LPVOID];k.ConnectNamedPipe.restype=w.BOOL
k.ReadFile.argtypes=[w.HANDLE,w.LPVOID,w.DWORD,c.POINTER(w.DWORD),w.LPVOID]
k.WriteFile.argtypes=[w.HANDLE,w.LPCVOID,w.DWORD,c.POINTER(w.DWORD),w.LPVOID]
k.CloseHandle.argtypes=[w.HANDLE];k.DisconnectNamedPipe.argtypes=[w.HANDLE];k.FlushFileBuffers.argtypes=[w.HANDLE]
a.ConvertStringSecurityDescriptorToSecurityDescriptorW.argtypes=[w.LPCWSTR,w.DWORD,c.POINTER(w.LPVOID),w.LPVOID]
sd=w.LPVOID();assert a.ConvertStringSecurityDescriptorToSecurityDescriptorW('D:P(A;;GA;;;OW)',1,c.byref(sd),None)
security=SA(c.sizeof(SA),sd,False)
def serve(handle):
    global seq
    try:
        data=b'';buf=c.create_string_buffer(8192);count=w.DWORD()
        while not data.endswith(b'\n'):
            if not k.ReadFile(handle,buf,len(buf),c.byref(count),None) or not count.value:return
            data+=buf.raw[:count.value]
        request=json.loads(data);op=request['cmd'];response={'ok':True}
        with lock:
            if op=='status':response['status']={**status,'bytes':log.tell()}
            elif op=='input':
                if request.get('input_kind') not in ('protocol','pointer'):
                    seq+=1;marker(1,'ECHO_'+identity+'_'+str(seq))
            elif op=='resize':geometry.write(struct.pack('<QHHI',log.tell(),request['rows'],request['cols'],0))
            elif op=='kill':status['exit']=0;status['state']='exited';persist();stopped.set()
            elif op!='control':response={'ok':False,'error':'unsupported fixture operation'}
        payload=(json.dumps(response)+'\n').encode();k.WriteFile(handle,payload,len(payload),c.byref(count),None);k.FlushFileBuffers(handle)
    finally:k.DisconnectNamedPipe(handle);k.CloseHandle(handle)
def accept():
    while not stopped.is_set():
        handle=k.CreateNamedPipeW('\\\\.\\pipe\\'+desc['endpoint'],3,0,255,65536,65536,0,c.byref(security))
        assert handle not in (None,c.c_void_p(-1).value),c.get_last_error()
        connected=k.ConnectNamedPipe(handle,None) or c.get_last_error()==535
        if connected:threading.Thread(target=serve,args=(handle,),daemon=True).start()
        else:k.CloseHandle(handle)
threading.Thread(target=accept,daemon=True).start()
stopped.wait();time.sleep(.3)
