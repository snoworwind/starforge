/* ============================================================
   STARFORGE - humanoid.js
   统一人形建模管线：SVG 剖面挤出 + 关节骨骼化
   村民 / 空间站站员 / 玩家化身 / 捏人预览共用同一套建模与动画，
   保证风格统一；四肢（臂/腿）有独立关节，可随行走摆动。
   ============================================================ */
'use strict';

const Humanoid = (() => {
  const S = 0.0172;   // SVG 48×104 坐标 → 世界单位（全高 ≈ 1.79，与既有村民一致）
  const H = 100;      // 靴底对应的 SVG y（作为地面基准）

  // ---------- 部件定义（路径 d / 挤出厚度 depth / z 偏移 / 关节 pivot [x,y]）----------
  const PARTS = {
    // 头组（关节在颈底 y≈18）
    hair:     { d: 'M15,4 Q24,-2 33,4 L34,10 L14,10 Z', depth: 13.5, pivot: [24, 18] },
    face:     { d: 'M15,6 Q24,2 33,6 L33,15 Q24,19 15,15 Z', depth: 13, pivot: [24, 18] },
    neck:     { d: 'M21,15 L27,15 L27,20 L21,20 Z', depth: 13, pivot: [24, 18] },
    hairBack: { d: 'M15,6 Q24,2 33,6 L33,14 Q24,18 15,14 Z', depth: 2.5, z: 5.0, pivot: [24, 18] },
    eyeL:     { d: 'M18.6,9.5 L21.4,9.5 L21.4,12.4 L18.6,12.4 Z', depth: 2.5, z: -7.3, pivot: [24, 18], basic: true },
    eyeR:     { d: 'M26.6,9.5 L29.4,9.5 L29.4,12.4 L26.6,12.4 Z', depth: 2.5, z: -7.3, pivot: [24, 18], basic: true },
    // 躯干组（关节在胯部 y≈56）
    torso:    { d: 'M13,19 L35,19 Q38,20 38,26 L36,54 L12,54 L10,26 Q10,20 13,19 Z', depth: 12, pivot: [24, 56] },
    trim:     { d: 'M22,19 L26,19 L26,54 L22,54 Z', depth: 12.5, pivot: [24, 56] },
    belt:     { d: 'M12,54 L36,54 L36,58 L12,58 Z', depth: 13, pivot: [24, 56] },
    // 手臂组（关节在肩 y≈20）
    armL:     { d: 'M7,21 L13,20 L12,47 L6,46 Z', depth: 12, pivot: [13, 20] },
    handL:    { d: 'M6,46 L12,47 L11.5,53 L5.8,52 Z', depth: 12, pivot: [13, 20] },
    armR:     { d: 'M35,20 L41,21 L42,46 L36,47 Z', depth: 12, pivot: [35, 20] },
    handR:    { d: 'M36,47 L42,46 L42.2,52 L36.5,53 Z', depth: 12, pivot: [35, 20] },
    // 腿组（关节在胯 y≈56）
    legL:     { d: 'M13,58 L22.4,58 L21.4,93 L14,93 Z', depth: 11, pivot: [17.5, 56] },
    bootL:    { d: 'M13.4,93 L21.6,93 L22,100 L12.4,100 Z', depth: 12, pivot: [17.5, 56] },
    legR:     { d: 'M25.6,58 L35,58 L34,93 L26.6,93 Z', depth: 11, pivot: [30.5, 56] },
    bootR:    { d: 'M26.4,93 L34.6,93 L35.6,100 L26,100 Z', depth: 12, pivot: [30.5, 56] },
  };
  // 发型附加路径（加入头组；长发的薄片贴后脑 +Z）
  const HAIR_STYLES = {
    none:   [],
    short:  [],
    long:   [{ d: 'M14,8 L34,8 L34,36 Q34,40 30,40 L18,40 Q14,40 14,36 Z', depth: 2.5, z: 5.5 }],
    pony:   [{ d: 'M19,12 Q30,8 34,17 L37,36 L33,41 L28,33 L26,16 Z', depth: 2.5, z: 5.5 }],
    mohawk: [{ d: 'M19,5 L29,5 L27,-4 L24,0 L21,-4 Z', depth: 13 }],
    bun:    [{ d: 'M19,2 A5,5 0 1,1 29,2 A5,5 0 1,1 19,2 Z', depth: 13 }],
  };

  // ---------- 建人：opt = { skin, hair, hairStyle, suit, trim, pants, boots, glove, belt, helmet, visor, badge } ----------
  function build(opt){
    opt = opt || {};
    const skin  = opt.skin  || '#e8c49a';
    const hairC = opt.hair  || '#4a3018';
    const suit  = opt.suit  || '#4a5a6e';
    const trimC = opt.trim  || suit;
    const pants = opt.pants || '#33404c';
    const boots = opt.boots || '#1e262e';
    const glove = opt.glove || '#2e3640';
    const beltC = opt.belt  || '#22303a';
    const eyeC  = '#20262e';

    const root = new THREE.Group();
    const rig = {};
    const joint = (name, x, y) => {
      const j = new THREE.Group();
      j.position.set(x * S, (H - y) * S, 0);
      root.add(j);
      rig[name] = j;
      return j;
    };
    const head = joint('head', 24, 18);
    const torso = joint('torso', 24, 56);
    const armL = joint('armL', 13, 20);
    const armR = joint('armR', 35, 20);
    const legL = joint('legL', 17.5, 56);
    const legR = joint('legR', 30.5, 56);
    torso.userData.baseY = (H - 56) * S;   // 躯干起伏动画围绕原始高度

    // 部件清单：路径 → 填充色 → 归属关节
    const P = [];
    const add = (key, fill, group) => P.push({ d: PARTS[key].d, fill, depth: PARTS[key].depth, z: PARTS[key].z, pivot: PARTS[key].pivot, basic: PARTS[key].basic, group });
    const useHair = opt.hairStyle !== 'none';
    if (useHair) add('hair', hairC, head);
    add('face', skin, head);
    add('neck', skin, head);
    for (const hs of (HAIR_STYLES[opt.hairStyle] || HAIR_STYLES.short)) P.push({ d: hs.d, fill: hairC, depth: hs.depth, z: hs.z, pivot: PARTS.hair.pivot, group: head });
    if (useHair) add('hairBack', hairC, head);
    add('eyeL', eyeC, head);
    add('eyeR', eyeC, head);
    add('torso', suit, torso);
    if (opt.trimOn) add('trim', trimC, torso);
    add('belt', beltC, torso);
    add('armL', suit, armL); add('handL', glove, armL);
    add('armR', suit, armR); add('handR', glove, armR);
    add('legL', pants, legL); add('bootL', boots, legL);
    add('legR', pants, legR); add('bootR', boots, legR);

    if (typeof THREE.SVGLoader !== 'function'){ root.userData.rig = rig; return root; }
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 104">'
      + P.map(p => `<path fill="${p.fill}" d="${p.d}"/>`).join('') + '</svg>';
    try {
      const data = new THREE.SVGLoader().parse(svg);
      const mats = {};
      for (let i = 0; i < data.paths.length && i < P.length; i++){
        const p = P[i];
        const shapes = THREE.SVGLoader.createShapes(data.paths[i]);
        if (!shapes.length) continue;
        if (!mats[p.fill]) mats[p.fill] = p.basic
          ? new THREE.MeshBasicMaterial({ color: new THREE.Color(p.fill) })
          : new THREE.MeshLambertMaterial({ color: new THREE.Color(p.fill) });
        const geo = new THREE.ExtrudeGeometry(shapes, { depth: p.depth, bevelEnabled: false, curveSegments: 8 });
        geo.translate(-p.pivot[0], -p.pivot[1], p.z !== undefined ? p.z : -p.depth / 2);
        const mesh = new THREE.Mesh(geo, mats[p.fill]);
        mesh.scale.set(S, -S, S);
        p.group.add(mesh);
      }
    } catch(e){ console.warn('[humanoid svg]', e); }

    // ---- 玩家专属配件（世界单位，直接挂关节；盔甲/护目镜/喷气背包）----
    if (opt.helmet){
      const hm = new THREE.MeshLambertMaterial({ color: new THREE.Color(suit) });
      const helmet = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.44, 0.48), hm);
      helmet.position.set(0, (18 - 10) * S, 0);
      head.add(helmet);
      if (opt.visor){
        const visorM = new THREE.MeshLambertMaterial({ color: new THREE.Color(opt.visor), emissive: new THREE.Color(opt.visor).multiplyScalar(0.25) });
        const visor = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.15, 0.04), visorM);
        visor.position.set(0, (18 - 10.5) * S, -0.25);
        head.add(visor);
      }
    }
    if (opt.badge){
      const badge = new THREE.Mesh(
        new THREE.BoxGeometry(0.13, 0.13, 0.03),
        new THREE.MeshBasicMaterial({ color: new THREE.Color(trimC) }));
      badge.position.set(0, (56 - 37) * S, -0.165);
      torso.add(badge);
    }
    if (opt.jetpack){
      const pack = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.44, 0.15),
        new THREE.MeshLambertMaterial({ color: new THREE.Color(opt.jetpack === true ? '#1d3a52' : opt.jetpack) }));
      pack.position.set(0, (56 - 38) * S, 0.19);
      torso.add(pack);
      for (const sx of [-0.09, 0.09]){
        const tank = new THREE.Mesh(new THREE.CylinderGeometry(0.045, 0.045, 0.42, 6),
          new THREE.MeshLambertMaterial({ color: new THREE.Color('#8fa8b8') }));
        tank.position.set(sx, (56 - 38) * S, 0.3);
        torso.add(tank);
      }
    }

    root.rig = rig;   // 关节引用挂普通属性：userData 会在 clone(true) 时被 JSON 序列化（Group 循环引用会抛错）
    return root;
  }

  // ---------- 动画：walk 权重平滑过渡；四肢交替摆动 + 躯干起伏 + 待机呼吸 ----------
  function animate(g, dt, moving, speed){
    const rig = (g.userData && g.userData.rig) || g.rig;
    if (!rig) return;
    let a = g.userData.hanim;
    if (!a) a = g.userData.hanim = { k: 0, t: Math.random() * 6 };
    const target = moving ? 1 : 0;
    a.k += (target - a.k) * Math.min(1, dt * 7);
    const sp = Math.max(0.5, speed || 1);
    a.t += dt * (2.2 + sp * 2.6) * (0.5 + a.k);
    const s = Math.sin(a.t);
    const amp = Math.min(0.62, 0.3 + sp * 0.22);
    const k = a.k;
    rig.legL.rotation.x = s * amp * k;
    rig.legR.rotation.x = -s * amp * k;
    rig.armL.rotation.x = -s * amp * 0.85 * k;
    rig.armR.rotation.x = s * amp * 0.85 * k;
    rig.torso.rotation.x = 0.05 * k + Math.sin(a.t * 2) * 0.02 * k;
    rig.torso.position.y = rig.torso.userData.baseY + (Math.abs(s) * 0.03) * k;
    rig.head.rotation.x = -Math.sin(a.t * 2) * 0.035 * k;
    const breath = Math.sin(a.t * 0.55) * 0.016 * (1 - k);
    rig.torso.scale.y = 1 + breath;
  }

  // 待机时清零摆动（对话/静止用）
  function rest(g){
    const rig = (g.userData && g.userData.rig) || g.rig;
    if (!rig) return;
    for (const k of ['head', 'armL', 'armR', 'legL', 'legR']){
      if (rig[k]) rig[k].rotation.x *= 0.9;
    }
    if (rig.torso){
      rig.torso.rotation.x *= 0.9;
      const b = rig.torso.userData.baseY !== undefined ? rig.torso.userData.baseY : rig.torso.position.y;
      rig.torso.position.y = b + (rig.torso.position.y - b) * 0.9;
      rig.torso.scale.y = 1 + (rig.torso.scale.y - 1) * 0.9;
    }
  }

  return { build, animate, rest, S, HEIGHT: 104 * S };
})();
window.Humanoid = Humanoid;
