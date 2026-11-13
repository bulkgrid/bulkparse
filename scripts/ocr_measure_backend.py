#!/usr/bin/env python3
"""Benchmark-only PID registration; exec the configured helper without changing OCR.

Avoid enumerating unrelated host processes when measuring the supervised backend.
This adds one small Python wrapper startup; it is not used by production configs.
"""
import json
import os
import sys

if len(sys.argv)<5 or sys.argv[1]!='--pid-file' or sys.argv[3]!='--':
    raise SystemExit('expected --pid-file PATH -- EXECUTABLE [ARGS...]')
with open(sys.argv[2],'a',encoding='utf-8') as stream:
    stream.write(json.dumps({'pid':os.getpid(),'parent':os.getppid()})+'\n')
os.execv(sys.argv[4],sys.argv[4:])
