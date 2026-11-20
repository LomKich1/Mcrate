# Суммарное сравнение для переносов между файлами: cmpmv.py "старые через запятую" "новые через запятую"
import subprocess,sys,collections,re
def lines(t):
    out=[]
    for l in t.split('\n'):
        s=l.strip()
        if not s or s.startswith('//') or s.startswith('use ') : continue
        s=re.sub(r'\s*//.*$','',s)
        out.append(s)
    return collections.Counter(out)
old=collections.Counter(); new=collections.Counter()
for p in sys.argv[1].split(','):
    old+=lines(subprocess.run(['git','show','HEAD:'+p],capture_output=True,text=True,encoding='utf-8').stdout)
for p in sys.argv[2].split(','):
    new+=lines(open(p,encoding='utf-8').read())
for k,v in (old-new).items(): print('  было, нет теперь:',v,k)
for k,v in (new-old).items(): print('  новое:',v,k)
