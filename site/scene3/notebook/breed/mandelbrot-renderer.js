(() => {
  const canvas=document.getElementById('field'),status=document.getElementById('status'),depth=document.getElementById('depth');
  const reduce=matchMedia('(prefers-reduced-motion: reduce)').matches, gl=canvas.getContext('webgl2',{alpha:false,antialias:false}), fallback=gl?null:canvas.getContext('2d',{alpha:false});
  const MAX=4096, RATE=2.2, HORIZON=13; // an immediately legible dive, asymptotically bounded inside the tested float-delta horizon
  let logZoom=reduce?7.5:0,parentVisible=true,intersecting=true,raf=0,last=performance.now(),lastDraw=0;
  const vertex=`#version 300 es
  in vec2 p;out vec2 uv;void main(){uv=p*.5+.5;gl_Position=vec4(p,0,1);}`;
  const fragment=`#version 300 es
  precision highp float;precision highp sampler2D;in vec2 uv;out vec4 outColor;
  uniform vec2 size;uniform float logZoom;uniform sampler2D orbit;
  vec3 palette(float x){vec3 a=vec3(.48,.39,.54),b=vec3(.48,.43,.40),c=vec3(1.,.74,.58),d=vec3(.02,.13,.28);return a+b*cos(6.283185*(c*x+d));}
  void main(){
    vec2 p=(uv-.5)*vec2(size.x/size.y,1.);float scale=exp(-logZoom);vec2 dc=p*3.1*scale;
    vec2 dz=vec2(0),z=vec2(0);float n=0.,trap=8.;
    for(int i=0;i<4096;i++){vec2 zr=texelFetch(orbit,ivec2(i,0),0).rg;z=zr+dz;trap=min(trap,min(abs(z.x),abs(z.y)));if(dot(z,z)>256.)break;dz=vec2(2.*(zr.x*dz.x-zr.y*dz.y),2.*(zr.x*dz.y+zr.y*dz.x))+vec2(dz.x*dz.x-dz.y*dz.y,2.*dz.x*dz.y)+dc;n+=1.;}
    float escaped=step(n,4095.);float smoothN=n-log2(log2(max(dot(z,z),1.0001)));float band=fract(smoothN*.037+logZoom*.017);
    float interior=clamp(-log(max(trap,1e-8))/18.420681,0.,1.);vec3 exterior=palette(band);vec3 inside=mix(vec3(.035,.028,.09),palette(.58+interior*.3),.38+.5*interior);
    vec3 col=mix(inside,exterior,escaped);col+=.12*palette(.12)*exp(-18.*trap);outColor=vec4(pow(max(col,0.),vec3(.82)),1.);
  }`;
  const Q=160n,S=1n<<Q,ten=10n**48n;
  const fixed=s=>{const neg=s[0]==='-',u=neg?s.slice(1):s,[w,f='']=u.split('.'),v=BigInt(w)*S+(BigInt((f+'0'.repeat(48)).slice(0,48))*S)/ten;return neg?-v:v};
  const mul=(a,b)=>(a*b)/S,cx=fixed('-0.743643887037158704752191506114774'),cy=fixed('0.131825904205311970493132056385139');
  let zx=0n,zy=0n;const ref=new Float32Array(MAX*2);for(let i=0;i<MAX;i++){ref[i*2]=Number(zx)/Number(S);ref[i*2+1]=Number(zy)/Number(S);const nx=mul(zx,zx)-mul(zy,zy)+cx,ny=2n*mul(zx,zy)+cy;zx=nx;zy=ny}
  let program=null,loc=null;
  if(gl){const shader=(type,src)=>{const s=gl.createShader(type);gl.shaderSource(s,src);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw Error(gl.getShaderInfoLog(s));return s};program=gl.createProgram();gl.attachShader(program,shader(gl.VERTEX_SHADER,vertex));gl.attachShader(program,shader(gl.FRAGMENT_SHADER,fragment));gl.linkProgram(program);if(!gl.getProgramParameter(program,gl.LINK_STATUS))throw Error(gl.getProgramInfoLog(program));const buf=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buf);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,1,-1,-1,1,1,1]),gl.STATIC_DRAW);const pos=gl.getAttribLocation(program,'p');gl.enableVertexAttribArray(pos);gl.vertexAttribPointer(pos,2,gl.FLOAT,false,0,0);const tex=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,tex);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.NEAREST);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.NEAREST);gl.texImage2D(gl.TEXTURE_2D,0,gl.RG32F,MAX,1,0,gl.RG,gl.FLOAT,ref);loc={size:gl.getUniformLocation(program,'size'),zoom:gl.getUniformLocation(program,'logZoom'),orbit:gl.getUniformLocation(program,'orbit')}}
  const active=()=>parentVisible&&intersecting&&!document.hidden;
  function fallbackDraw(){const w=180,h=82;if(canvas.width!==w||canvas.height!==h){canvas.width=w;canvas.height=h}const im=fallback.createImageData(w,h),scale=3.1*Math.exp(-Math.min(logZoom,12));for(let y=0;y<h;y++)for(let x=0;x<w;x++){const cr=-.7436438870371587+(x/w-.5)*scale*w/h,ci=.131825904205312+(y/h-.5)*scale;let zr=0,zi=0,n=0;for(;n<384&&zr*zr+zi*zi<256;n++){const q=zr*zr-zi*zi+cr;zi=2*zr*zi+ci;zr=q}const o=(y*w+x)*4,t=n/384;im.data[o]=45+Math.floor(205*t);im.data[o+1]=18+Math.floor(105*(1-t));im.data[o+2]=70+Math.floor(165*t);im.data[o+3]=255}fallback.putImageData(im,0,0)}
  function draw(now){if(!gl||!program){if(fallback){fallbackDraw();depth.textContent='log zoom '+logZoom.toFixed(1);status.textContent=reduce?'paused · reduced motion':'running · canvas fallback';lastDraw=now}return;}const dpr=Math.min(1.35,devicePixelRatio||1),rect=canvas.getBoundingClientRect(),w=Math.max(1,Math.round(rect.width*dpr)),h=Math.max(1,Math.round(rect.height*dpr));if(canvas.width!==w||canvas.height!==h){canvas.width=w;canvas.height=h;gl.viewport(0,0,w,h)}gl.useProgram(program);gl.uniform2f(loc.size,w,h);gl.uniform1f(loc.zoom,logZoom);gl.uniform1i(loc.orbit,0);gl.drawArrays(gl.TRIANGLE_STRIP,0,4);depth.textContent='log zoom '+logZoom.toFixed(1);status.textContent=reduce?'paused · reduced motion':'running';lastDraw=now}
  function schedule(){if(active()&&!reduce&&!raf)raf=requestAnimationFrame(frame)}
  function stop(label='paused · offscreen'){cancelAnimationFrame(raf);raf=0;status.textContent=label}
  function frame(now){raf=0;if(!active())return stop();const dt=Math.max(0,Math.min(.1,(now-last)/1000));last=now;logZoom+=dt*RATE*(1-logZoom/HORIZON);if(now-lastDraw>=32)draw(now);schedule()}
  function visibility(){if(active()){last=performance.now();draw(last);schedule()}else stop()}
  addEventListener('message',e=>{if(e.origin!==location.origin||!['moss-visible','moss-notebook-visible'].includes(e.data?.type))return;parentVisible=e.data.visible!==false;visibility()});
  document.addEventListener('visibilitychange',visibility);addEventListener('resize',()=>draw(performance.now()));new IntersectionObserver(([e])=>{intersecting=e.isIntersecting;visibility()},{threshold:.01}).observe(canvas);new ResizeObserver(()=>draw(performance.now())).observe(canvas);
  draw(performance.now());schedule();
  window.__notebook={pause:()=>{parentVisible=false;visibility()},resume:()=>{parentVisible=true;visibility()},setLogZoom:v=>{logZoom=Math.max(0,Number(v)||0);draw(performance.now())},state:()=>({visible:active(),logZoom,reduced:reduce,referenceOrbit:MAX,precision:'Q160 fixed-point reference + float perturbation',testedHorizon:HORIZON,initialRate:RATE,trajectory:'asymptotic inward zoom'})};
})();
