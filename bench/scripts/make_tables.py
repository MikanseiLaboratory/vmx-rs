import csv,sys
QN={'OMT_LQ':'Low','OMT_SQ':'Medium','OMT_HQ':'High'}
ORDER=['OMT_LQ','OMT_SQ','OMT_HQ']
def load(paths):
    rows=[]
    for p in paths:
        with open(p) as f:
            for r in csv.DictReader(f, fieldnames=None if open(p).readline().startswith('res,') else
                ['res','profile','image','q','impl','simd','op','threads','fps_median','fps_min','fps_max','bytes_per_frame','bitexact_vs_libvmx128']):
                if r['res']=='res' or r['profile'] not in QN: continue
                rows.append(r)
    return rows
def tables(rows, title):
    imps=['libvmx','libvmx-128bit','saul-vmx-codec','mikansei-vmx']
    hdr={'libvmx':'libvmx','libvmx-128bit':'libvmx 128-bit','saul-vmx-codec':'vmx-codec','mikansei-vmx':'vmx'}
    present=[i for i in imps if any(r['impl']==i for r in rows)]
    d={(r['res'],r['profile'],r['image'],r['impl'],r['op'],r['threads']):r for r in rows}
    keys=sorted({(r['res'],r['image'],r['profile']) for r in rows}, key=lambda k:(k[0],k[1]!='natural',ORDER.index(k[2])))
    ths=sorted({r['threads'] for r in rows}, key=int)
    out=[f"## {title}\n"]
    bad=[r for r in rows if r['bitexact_vs_libvmx128'] not in ('yes','ref','n/a','-','')]
    out.append(f"Rows: {len(d)}. Bit-exact vs libvmx 128-bit: {'all yes' if not bad else str(len(bad))+' rows not yes: '+str({r['bitexact_vs_libvmx128'] for r in bad})}. fps median of 7; **bold** = fastest on the row; ✗ = below 60 fps.\n")
    for op in ['encode','decode']:
        for th in ths:
            out.append(f"### {op.capitalize()}, {th} thread{'s' if th!='1' else ''}\n")
            out.append("| Resolution | Profile | Image | bytes/frame | "+" | ".join(hdr[i] for i in present)+" |")
            out.append("| --- | --- | --- | ---: | "+" | ".join('---:' for _ in present)+" |")
            for (res,img,prof) in keys:
                vals={}
                for i in present:
                    r=d.get((res,prof,img,i,op,th))
                    if r: vals[i]=float(r['fps_median'])
                if not vals: continue
                ref=d.get((res,prof,img,'libvmx','encode','1')) or next(r for k,r in d.items() if k[:3]==(res,prof,img))
                best=max(round(v) for v in vals.values())
                cells=[]
                for i in present:
                    if i not in vals: cells.append('—'); continue
                    s=f"{vals[i]:.0f}"+(" ✗" if vals[i]<60 else "")
                    cells.append(f"**{s}**" if round(vals[i])==best else s)
                out.append(f"| {res} | {prof} — {QN[prof]} ({ref['q']}) | {img} | {int(ref['bytes_per_frame']):,} | "+" | ".join(cells)+" |")
            out.append("")
    # ratio summary
    out.append("### vmx / libvmx (geomean over rows, min–max)\n")
    out.append("| Operation | Threads | vmx / libvmx | vmx-codec / libvmx |\n|---|---:|---:|---:|")
    import math
    for op in ['encode','decode']:
        for th in ths:
            res=[]
            for other in ['mikansei-vmx','saul-vmx-codec']:
                rs=[float(d[(k[0],k[2],k[1],other,op,th)]['fps_median'])/float(d[(k[0],k[2],k[1],'libvmx',op,th)]['fps_median'])
                    for k in keys if (k[0],k[2],k[1],other,op,th) in d and (k[0],k[2],k[1],'libvmx',op,th) in d]
                g=math.exp(sum(map(math.log,rs))/len(rs)) if rs else float('nan')
                res.append(f"{g:.2f} ({min(rs):.2f}–{max(rs):.2f})")
            out.append(f"| {op.capitalize()} | {th} | {res[0]} | {res[1]} |")
    out.append("")
    return "\n".join(out)
if __name__=='__main__':
    title=sys.argv[1]; print(tables(load(sys.argv[2:]),title))
