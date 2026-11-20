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
            out.append("| Resolution | Profile | Image | bytes/frame | "+" | ".join(hdr[i] for i in present)+" | Fastest |")
            out.append("| --- | --- | --- | ---: | "+" | ".join('---:' for _ in present)+" | --- |")
            for (res,img,prof) in keys:
                vals={}
                for i in present:
                    r=d.get((res,prof,img,i,op,th))
                    if r: vals[i]=float(r['fps_median'])
                if not vals: continue
                ref=d.get((res,prof,img,'libvmx','encode','1')) or next(r for k,r in d.items() if k[:3]==(res,prof,img))
                best=max(round(v) for v in vals.values())
                cells=[]
                winners=[]
                for i in present:
                    if i not in vals: cells.append('—'); continue
                    s=f"{vals[i]:.0f}"+(" ✗" if vals[i]<60 else "")
                    if round(vals[i])==best:
                        cells.append(f"**{s}**")
                        winners.append(hdr[i])
                    else:
                        cells.append(s)
                out.append(f"| {res} | {prof} — {QN[prof]} ({ref['q']}) | {img} | {int(ref['bytes_per_frame']):,} | "+" | ".join(cells)+f" | {', '.join(winners)} |")
            out.append("")
    # ratio summary. Fastest is the highest geometric mean of fps
    # (libvmx's ratio is 1). Bold the winner's printed ratio.
    import math
    label={'libvmx':'libvmx','libvmx-128bit':'libvmx 128-bit','saul-vmx-codec':'vmx-codec','mikansei-vmx':'vmx'}
    def ratios(op, th, impl, img=None):
        rs=[]
        for res,image,prof in keys:
            if img and image!=img: continue
            a=(res,prof,image,impl,op,th); b=(res,prof,image,'libvmx',op,th)
            if a in d and b in d: rs.append(float(d[a]['fps_median'])/float(d[b]['fps_median']))
        return rs
    def cell(rs):
        g=math.exp(sum(map(math.log,rs))/len(rs))
        return g, f"{g:.2f} ({min(rs):.2f}–{max(rs):.2f})"
    def fastest(op, th, img=None):
        scores={}
        for impl in present:
            rs=ratios(op, th, impl, img)
            if rs: scores[impl]=round(cell(rs)[0], 2)
        best=max(scores.values())
        return [label[i] for i,v in scores.items() if v==best]
    out.append("### vmx / libvmx (geomean over rows, min–max)\n")
    out.append("| Operation | Threads | vmx / libvmx | vmx-codec / libvmx | Fastest |\n|---|---:|---:|---:|---|")
    for op in ['encode','decode']:
        for th in ths:
            shown=[]
            for other in ['mikansei-vmx','saul-vmx-codec']:
                text=cell(ratios(op, th, other))[1]
                if label[other] in fastest(op, th): text=f"**{text}**"
                shown.append(text)
            out.append(f"| {op.capitalize()} | {th} | {shown[0]} | {shown[1]} | {', '.join(fastest(op, th))} |")
    out.append("")
    out.append("vmx / libvmx by image. Fastest is over every implementation present.\n")
    out.append("| Operation | Threads | natural | bars | Fastest (natural) | Fastest (bars) |\n|---|---:|---:|---:|---|---|")
    for op in ['encode','decode']:
        for th in ths:
            cells=[]
            names=[]
            for img in ['natural','bars']:
                text=cell(ratios(op, th, 'mikansei-vmx', img))[1]
                win=fastest(op, th, img)
                if 'vmx' in win: text=f"**{text}**"
                cells.append(text); names.append(', '.join(win))
            out.append(f"| {op.capitalize()} | {th} | {cells[0]} | {cells[1]} | {names[0]} | {names[1]} |")
    out.append("")
    return "\n".join(out)
if __name__=='__main__':
    title=sys.argv[1]; print(tables(load(sys.argv[2:]),title))
