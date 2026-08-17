// Golden-value generator: extract mulberry32 + makeNoise + lattice3 from the original game and print reference values.
'use strict';
function mulberry32(seed){ let a = seed >>> 0; return function(){ a |= 0; a = (a + 0x6D2B79F5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
function makeNoise(seed){
  const rnd = mulberry32(seed);
  const perm = new Uint8Array(512); const p = [];
  for (let i = 0; i < 256; i++) p[i] = i;
  for (let i = 255; i > 0; i--){ const j = (rnd() * (i + 1)) | 0; [p[i], p[j]] = [p[j], p[i]]; }
  for (let i = 0; i < 512; i++) perm[i] = p[i & 255];
  function fade(t){ return t * t * t * (t * (t * 6 - 15) + 10); }
  function grad2(h, x, y){ switch (h & 3){ case 0: return x + y; case 1: return -x + y; case 2: return x - y; default: return -x - y; } }
  function n2(x, y){
    const X = Math.floor(x) & 255, Y = Math.floor(y) & 255;
    x -= Math.floor(x); y -= Math.floor(y);
    const u = fade(x), v = fade(y);
    const a = perm[X] + Y, b = perm[X + 1] + Y;
    const lerp = (a,b,t)=>a+(b-a)*t;
    return lerp(lerp(grad2(perm[a], x, y), grad2(perm[b], x - 1, y), u),
                lerp(grad2(perm[a + 1], x, y - 1), grad2(perm[b + 1], x - 1, y - 1), u), v);
  }
  function fbm2(x, y, oct = 4, lac = 2, gain = 0.5){
    let amp = 1, f = 1, sum = 0, norm = 0;
    for (let i = 0; i < oct; i++){ sum += n2(x * f, y * f) * amp; norm += amp; amp *= gain; f *= lac; }
    return sum / norm;
  }
  return { n2, fbm2 };
}
const seed = 7777;
function lattice3(x, y, z, salt) {
  let h = (seed ^ salt) >>> 0;
  h = Math.imul(h ^ x, 374761393);
  h = Math.imul(h ^ y, 217645177);
  h = Math.imul(h ^ z, 668265263);
  h = Math.imul(h ^ (h >>> 15), 2246822519);
  return ((h ^ (h >>> 13)) >>> 0) / 4294967296;
}
const out = [];
out.push('mulberry32(12345):');
{ const r = mulberry32(12345); for (let i = 0; i < 6; i++) out.push(r().toFixed(8)); }
const n = makeNoise(7777);
out.push('n2(1.5,2.5)=' + n.n2(1.5, 2.5).toFixed(8));
out.push('n2(100.25,-40.75)=' + n.n2(100.25, -40.75).toFixed(8));
out.push('fbm(12.5,7.5,4)=' + n.fbm2(12.5, 7.5, 4, 2, 0.5).toFixed(8));
out.push('fbm(-3.1,9.9,5)=' + n.fbm2(-3.1, 9.9, 5, 2, 0.5).toFixed(8));
out.push('lattice3(3,4,5,0xCAFE01)=' + lattice3(3, 4, 5, 0xCAFE01).toFixed(8));
out.push('lattice3(-10,2,33,0xCAFE01)=' + lattice3(-10, 2, 33, 0xCAFE01).toFixed(8));
// vnoise3 golden
function vnoise3(x, y, z, salt) {
  const ix = Math.floor(x), iy = Math.floor(y), iz = Math.floor(z);
  let fx = x - ix, fy = y - iy, fz = z - iz;
  fx = fx*fx*(3 - 2*fx); fy = fy*fy*(3 - 2*fy); fz = fz*fz*(3 - 2*fz);
  const c000 = lattice3(ix,iy,iz,salt), c100 = lattice3(ix+1,iy,iz,salt);
  const c010 = lattice3(ix,iy+1,iz,salt), c110 = lattice3(ix+1,iy+1,iz,salt);
  const c001 = lattice3(ix,iy,iz+1,salt), c101 = lattice3(ix+1,iy,iz+1,salt);
  const c011 = lattice3(ix,iy+1,iz+1,salt), c111 = lattice3(ix+1,iy+1,iz+1,salt);
  const x00 = c000 + (c100-c000)*fx, x10 = c010 + (c110-c010)*fx;
  const x01 = c001 + (c101-c001)*fx, x11 = c011 + (c111-c011)*fx;
  const y0 = x00 + (x10-x00)*fy, y1 = x01 + (x11-x01)*fy;
  return y0 + (y1-y0)*fz;
}
out.push('vnoise3(5.3,10.7,7.9,0xCAFE01)=' + vnoise3(5.3, 10.7, 7.9, 0xCAFE01).toFixed(8));
console.log(out.join('\n'));
