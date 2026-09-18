const TAU = Math.PI * 2;
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const fract = (v) => v - Math.floor(v);
const noise = (n) => fract(Math.sin(n * 91.3458) * 47453.5453);

export const works = [
  ['processing-01', 'Orbital veil', 'Faithful port 01'],
  ['processing-02', 'Radiant seed', 'Faithful port 02'],
  ['processing-03', 'Lantern shell', 'Faithful port 03 · selected default'],
  ['processing-04', 'Moth aperture', 'Faithful port 04'],
  ['processing-05', 'Tidal rosette', 'Faithful port 05'],
  ['processing-06', 'Civic weather', 'Faithful port 06'],
  ['processing-07', 'Mandelbrot relief', 'Faithful P3D port 07'],
  ['living-garden', 'Nocturnal colony', 'One species derived from faithful ports 01, 02, and 04'],
  ['silk-orbit', 'Silk orbit', 'Derivative'],
  ['ink-coral', 'Ink coral', 'Derivative'],
  ['glass-pollen', 'Glass pollen', 'Derivative'],
  ['nocturne-fold', 'Nocturne fold', 'Derivative'],
  ['mandelbrot-moon', 'Mandelbrot moon', 'Derivative'],
];

function point(ctx, x, y, a = .42, size = 1) {
  if (!Number.isFinite(x + y)) return;
  ctx.globalAlpha = a;
  if (size <= 1.2) ctx.fillRect(x, y, size, size);
  else { ctx.beginPath(); ctx.arc(x, y, size / 2, 0, TAU); ctx.fill(); }
}

function plotOriginal(ctx, id, t, w, h, budget) {
  const sx = w / 400, sy = h / 400;
  ctx.save(); ctx.scale(sx, sy); ctx.fillStyle = '#f5f0e6';
  const domain=id==='processing-05'?20000:id==='processing-06'?40000:10000;
  const samples=Math.min(domain,budget), stride=domain/samples;
  for(let j=samples;j--;){const i=Math.floor(j*stride);
    if (id === 'processing-01') { const y=i/235,k=(4+Math.cos(i/9-t*2))*Math.cos(i/35),e=y/7-13,d=Math.hypot(k,e)+Math.sin(e/9+t/2)-4,c=d-t,q=2*Math.sin(k*3)-y/35*k*(9+k*Math.sin(Math.cos(e)*9-d*2+t)); point(ctx,q+40*Math.cos(c)+200,q*Math.sin(c)+d*35,.38); }
    if (id === 'processing-02') { const m=(i%4)*5,k=2*Math.cos(i*342),e=Math.sin(i*271)*2,d=Math.hypot(k,e)/1.6,c=d*d/9-t/8+m,p=5+2*Math.sin(d*8-t*3+m); point(ctx,k*p+9/d*Math.sin(k*2)+89*Math.sin(c)+200,79*Math.sin(c*2)+9/d*Math.sin(e*2)+e*p+200,.46); }
    if (id === 'processing-03') { const x=i,y=i/235,k=4*Math.cos(x/21),e=y/8-20,d=Math.hypot(k,e),c=d-t,q=3*Math.sin(k*2)+.3/(k||.01)+Math.sin(y/19)*k*(9+2*Math.sin(e*14-d*3+t*2)); point(ctx,q+50*Math.cos(c)+200,q*Math.sin(c)+d*39-475,.46,k*k>15?2:1); }
    if (id === 'processing-04') { const m=i%19,k=9*Math.cos(i*5)*Math.sin(i),e0=Math.cos(i*7)*Math.cos(i)*9,d=Math.pow(Math.hypot(k,e0),3)/999+4.6-Math.pow(Math.cos(t/4+m),3)/3;if(e0<=0)continue;const c=d/8-t/32+m,o=Math.sin(d*d-t+m); point(ctx,99*Math.sin(c)+k/Math.pow(3,o)+200,99*Math.cos(c/3)+d*39+Math.pow(e0,o),.38); }
    if (id === 'processing-05') { const y=i/598,k=(5+Math.sin(y))*Math.cos(i/7),e=y/5-11,d=Math.hypot(k,e)/.6-6,c=d/4-t/8+Math.cos(t+e)/9,q=99+d*Math.sin(t-d)+y/23*k*(3*Math.sin(e)+e*Math.sin(e*2)+Math.sin(d*4)); point(ctx,q*Math.sin(c)+200,q*Math.cos(c)+200,.34); }
    if (id === 'processing-06') { const x=i%200,y=i/200,k=x/8-12.5,e=y/8-12.5,o=Math.hypot(k,e)/12*Math.cos(Math.sin(k/2)*Math.cos(e/2)),d=5*Math.cos(o); point(ctx,(x+d*k*(Math.sin(d*2+t)+Math.sin(y*o*o)/9))/1.5+133,(y/3-d*40+19*Math.cos(d+t))*1.5+300,.18); }
  }
  ctx.restore();
}

function livingGarden(ctx,t,w,h,budget){
  const scale=Math.min(w,h)/400,count=24,samples=Math.max(220,Math.floor(budget/count));
  ctx.save();ctx.translate(w/2,h/2);ctx.scale(scale,scale);
  for(let organism=0;organism<count;organism++){
    const seed=organism*37.19,ring=Math.floor(organism/8),angle=organism/8*TAU+ring*.31;
    const ox=Math.cos(angle)*(24+ring*49),oy=Math.sin(angle)*(20+ring*40),size=.40+ring*.10+(organism%3)*.025;
    ctx.fillStyle='#f7f7f4';
    ctx.save();ctx.translate(ox,oy);ctx.rotate(angle*.18+Math.sin(t*.16+seed)*.10);ctx.scale(size,size);
    for(let j=0;j<samples;j++){
      const i=j*(10000/samples),phase=t*.48+seed,m=i%19;
      const k=8*Math.cos(i*5+seed*.01)*Math.sin(i),e=Math.cos(i*7)*Math.cos(i)*8;
      if(e<=-.7)continue;
      const d=Math.pow(Math.hypot(k,e),3)/1100+4.2-Math.pow(Math.cos(phase/4+m),3)/3;
      const c=d/8-phase/32+m,o=Math.sin(d*d-phase+m),veil=Math.sin(i/235/7-phase*.35)*10;
      const x=92*Math.sin(c)+k/Math.pow(3,o)+veil;
      const y=86*Math.cos(c/3)+d*36+Math.pow(Math.max(.08,e+1),o)-188;
      point(ctx,x,y,.58+(j%11===0?.34:0),j%29===0?2.8:1.55);
    }
    ctx.restore();
  }
  ctx.restore();
}

function mandelbrot(ctx, t, w, h, variant=false) {
  const step = Math.max(2, Math.floor(Math.min(w,h)/180));
  // processing-07 keeps the original fixed diagonal camera and lets the
  // escape-time relief grow on z. The moon variant is the rotating response.
  const ang = variant?t*.14:0, ca=Math.cos(ang), sa=Math.sin(ang);
  for(let py=0;py<h;py+=step) for(let px=0;px<w;px+=step){let x=px/w*800-400,y=py/h*800-400,zr=0,zi=0,n=0;for(;zr*zr+zi*zi<4&&n<400;n++){const q=zr*zr-zi*zi+x/400-1;zi=2*zr*zi+y/400;zr=q;}const z=-n*(variant?.75:(t*.6));if(variant){const rx=x*ca-y*sa,ry=x*sa+y*ca-z*.7;ctx.fillStyle=`hsla(${210+n*.35},36%,${clamp(22+n*.18,15,92)}%,.72)`;ctx.fillRect(w/2+rx*w/900,h/2+ry*h/900,step+1,step+1);}else{const vx=x-400,vy=y-400,vz=z-400,depth=-(vx+vy+vz)/Math.sqrt(3);if(depth<=1)continue;const focal=Math.min(w,h)*1.08,sx=(vx-vy)/Math.sqrt(2),sy=(vx+vy-2*vz)/Math.sqrt(6);ctx.fillStyle=`rgb(${n},${n},${n})`;ctx.fillRect(w/2+focal*sx/depth,h/2+focal*sy/depth,Math.max(1,step*.7),Math.max(1,step*.7));}}
}

function derivative(ctx,id,t,w,h,budget){
  const cx=w/2,cy=h/2,scale=Math.min(w,h)/400;
  ctx.save(); ctx.translate(cx,cy); ctx.scale(scale,scale);
  for(let i=0;i<budget*1.4;i++){ const u=i/budget, a=u*TAU*9+t*.18, r=26+132*Math.pow(u,.72); let x,y,alpha=.28,size=1;
    if(id==='silk-orbit'){const fold=Math.sin(a*3-t)*18*(1-u);x=Math.cos(a)*r+Math.cos(a*.34)*fold;y=Math.sin(a)*r*.62+Math.sin(a*2.1+t)*13;alpha=.18+.28*u;size=1+2*(1-u);}
    else if(id==='ink-coral'){const branch=Math.sin(i*.071+t*.35);x=Math.cos(a*.37)*r*.72+branch*34;y=Math.sin(a*.51)*r*.68+Math.cos(i*.013)*16;alpha=.12+.28*noise(i);size=1.2;}
    else if(id==='glass-pollen'){const rr=r*(.55+.45*Math.sin(a*1.7+t));x=Math.cos(a)*rr;y=Math.sin(a)*rr;alpha=.1+.42*noise(i);size=1+4*Math.pow(noise(i+7),7);}
    else {const q=Math.sin(a*2+t)*Math.cos(a*.5-t*.2);x=Math.cos(a)*r*(.5+.45*q);y=Math.sin(a)*r*.8;alpha=.16+.3*u;size=1.4;}
    ctx.fillStyle=id==='glass-pollen'?'#e8b89d':id==='ink-coral'?'#b9d5ca':'#eadbc4';point(ctx,x,y,alpha,size);
  } ctx.restore();
}

export function mount(canvas, id, {interactive=true,animate=true}={}) {
  const ctx=canvas.getContext('2d',{alpha:false}); let parentVisible=true, intersecting=true, start=performance.now(), raf=0;
  const reduce=matchMedia('(prefers-reduced-motion: reduce)').matches;
  const active=()=>animate&&!reduce&&parentVisible&&intersecting&&!document.hidden;
  const schedule=()=>{if(active()&&!raf)raf=requestAnimationFrame(draw)};
  function resize(){const r=canvas.getBoundingClientRect(),d=Math.min(devicePixelRatio||1,2);canvas.width=Math.max(1,Math.round(r.width*d));canvas.height=Math.max(1,Math.round(r.height*d));ctx.setTransform(d,0,0,d,0,0);draw(performance.now(),true);}
  function draw(now,once=false){if(!once)raf=0;const w=canvas.clientWidth,h=canvas.clientHeight,sec=(now-start)/1000,rate={"processing-01":Math.PI*3/4,"processing-02":Math.PI,"processing-03":Math.PI/4,"processing-04":Math.PI*2,"processing-05":Math.PI*2,"processing-06":Math.PI*2/3,"living-garden":Math.PI/2}[id],t=rate?sec*rate:sec;ctx.globalAlpha=1;ctx.fillStyle=id==='living-garden'?'#050505':id==='processing-07'?'#fff':id==='processing-06'?'rgba(6,6,6,.376)':'#090b10';ctx.fillRect(0,0,w,h);ctx.globalCompositeOperation=(id==='processing-07'||id==='mandelbrot-moon'||id==='living-garden')?'source-over':'lighter';const budget=id==='living-garden'?52000:Math.round(clamp(w*h/42,3500,12000));if(id==='processing-07'||id==='mandelbrot-moon')mandelbrot(ctx,t,w,h,id==='mandelbrot-moon');else if(id==='living-garden')livingGarden(ctx,t,w,h,budget);else if(id.startsWith('processing-'))plotOriginal(ctx,id,t,w,h,budget);else derivative(ctx,id,t,w,h,budget);ctx.globalCompositeOperation='source-over';if(!once)schedule();}
  const stop=()=>{cancelAnimationFrame(raf);raf=0};
  const ro=new ResizeObserver(resize);ro.observe(canvas);const io=new IntersectionObserver(([e])=>{intersecting=e.isIntersecting;if(active())schedule();else stop()},{rootMargin:'80px'});io.observe(canvas);
  const visibility=()=>active()?schedule():stop();document.addEventListener('visibilitychange',visibility);
  const message=e=>{if(e.origin!==location.origin||e.data?.type!=='moss-visible')return;parentVisible=!!e.data.visible;visibility()};addEventListener('message',message);
  if(interactive) canvas.addEventListener('pointerdown',()=>{start=performance.now();draw(start,true)});
  resize(); schedule(); return()=>{ro.disconnect();io.disconnect();document.removeEventListener('visibilitychange',visibility);removeEventListener('message',message);stop()};
}
