import subprocess,sys,collections,re
def lines(t):
    out=[]
    for l in t.split('\n'):
        s=l.strip()
        if not s or s.startswith('//') or s.startswith('use ') : continue
        s=re.sub(r'\s*//.*$','',s)  # хвостовые комментарии
        out.append(s)
    return collections.Counter(out)
for p in sys.argv[1:]:
    old=subprocess.run(['git','show','HEAD:'+p],capture_output=True,text=True).stdout
    new=open(p,encoding='utf-8').read()
    a,b=lines(old),lines(new)
    print('==',p)
    for k,v in (a-b).items(): print('  было, нет теперь:',v,k)
    for k,v in (b-a).items(): print('  новое:',v,k)
