"""Deterministic PTY producer; markers are constructed here, never in shell echo."""
import os
import sys
import time

token, chunks, interval = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])

def write(data):
    while data:
        size = os.write(1, data)
        data = data[size:]

write(('PERF_BEGIN_' + token + '\n').encode())
started = time.monotonic()
for index in range(chunks):
    write(b'Z' * 65536)
    if interval:
        time.sleep(max(0, started + (index + 1) * interval - time.monotonic()))
write(('\nPERF_END_' + token + '\n').encode())
