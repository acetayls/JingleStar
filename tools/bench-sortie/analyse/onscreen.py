import ana, sys, numpy as np, collections
L=ana.load(sys.argv[1]); mk=None; res=collections.defaultdict(list); pend=None
for x in L:
    if x['k']=='mark': mk=x['label']
    if x['k']=='cmd' and isinstance(x['cmd'],dict) and 'Load' in x['cmd']: pend=(x['s'],mk)
    if x['k']=='ev' and x['ev']=='OnScreen' and pend:
        res[pend[1].split()[0]].append(((x['s']-pend[0])*1000, pend[1].split()[-1])); pend=None
for k,v in res.items():
    a=[t for t,_ in v]; print(k, 'n',len(a),'med',round(np.median(a),1),'max',round(max(a),1), 'min', round(min(a),1))
    if len(sys.argv)>2: print('   ', [(round(t),f[:12]) for t,f in v])
