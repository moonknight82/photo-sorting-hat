#!/usr/bin/env python3
"""Index/planning benchmark without generating a million real media files.

Writes only under a temporary directory. This measures indexing/query/planning,
not camera decoding, source hashing, network storage, or copying throughput.
"""
import argparse
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
p=argparse.ArgumentParser();p.add_argument('--files',type=int,default=1_000_000);p.add_argument('--cli',type=Path,default=Path('target/release/photo-sorting-hat'));p.add_argument('--output',type=Path,default=Path('artifacts/benchmark.json'));args=p.parse_args()
cli=args.cli.resolve();result={'files':args.files,'scope':'Synthetic SQLite index, full recipe plan, keyset page; no media I/O'}
with tempfile.TemporaryDirectory(prefix='photo-hat-benchmark-') as temp:
    temp=Path(temp);db=temp/'index.sqlite'
    subprocess.run([str(cli),'--db',str(db),'status'],check=True,stdout=subprocess.DEVNULL)
    conn=sqlite3.connect(db)
    start=time.monotonic()
    meta=json.dumps({'ExifIFD:DateTimeOriginal':'2024:07:18 14:30:22','ExifIFD:OffsetTimeOriginal':'-03:00','IFD0:Model':'Synthetic Camera'})
    conn.executemany('INSERT INTO files(source,root,size,mtime,metadata,folders,companions,kind) VALUES(?,?,?,?,?,?,?,?)',((str(temp/'source'/f'image_{i}.jpg'),str(temp/'source'),1000000+i,'1700000000000000000',meta,'["Benchmark"]','[]','photo') for i in range(args.files)))
    conn.executemany('INSERT OR REPLACE INTO config VALUES(?,?)',[('scan_state','complete'),('output',str(temp/'out'))]);conn.commit();conn.close()
    result['index_seconds']=round(time.monotonic()-start,3)
    start=time.monotonic();subprocess.run([str(cli),'--db',str(db),'plan'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);result['plan_seconds']=round(time.monotonic()-start,3)
    start=time.monotonic();page=subprocess.check_output([str(cli),'--db',str(db),'list','--after',str(max(0,args.files-100)),'--limit','100']);result['page_seconds']=round(time.monotonic()-start,3);result['page_items']=len(json.loads(page));result['database_bytes']=db.stat().st_size
args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
